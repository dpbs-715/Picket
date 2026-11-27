import type { Change } from '../state/workbench';
import { splitPath } from '../state/workbench';

export interface DirectoryTreeNode {
  id: string;
  segments: string[];
  label: string;
  fullPath: string;
  isRootGroup: boolean;
  children: DirectoryTreeNode[];
  files: Change[];
  allFiles: Change[];
}

interface RawDirNode {
  segment: string;
  fullPath: string;
  children: Map<string, RawDirNode>;
  files: Change[];
}

export const TREE_MAX_VISUAL_DEPTH = 4;

export function treeIndentPx(depth: number): number {
  if (depth <= 0) return 12;
  const clamped = Math.min(depth, TREE_MAX_VISUAL_DEPTH);
  return 28 + (clamped - 1) * 12;
}

export function treeGuideOffsets(depth: number): number[] {
  const clamped = Math.min(Math.max(depth, 0), TREE_MAX_VISUAL_DEPTH);
  const offsets: number[] = [];
  for (let level = 0; level < clamped; level += 1) {
    offsets.push(treeIndentPx(level) + 7);
  }
  return offsets;
}

export function splitFileNameExt(name: string): { stem: string; ext: string } {
  const dot = name.lastIndexOf('.');
  if (dot <= 0) return { stem: name, ext: '' };
  const ext = name.slice(dot);
  if (ext.length > 8) return { stem: name, ext: '' };
  return { stem: name.slice(0, dot), ext };
}

export function formatDirectoryLabel(
  segments: readonly string[],
  maxChars = 38,
): { prefix: string; leaf: string } {
  if (segments.length === 0) return { prefix: '', leaf: '' };
  if (segments.length === 1) return { prefix: '', leaf: segments[0] };

  const full = segments.join('/');
  const leaf = segments[segments.length - 1];
  if (segments.length === 2 || full.length <= maxChars) {
    return { prefix: segments.slice(0, -1).join('/'), leaf };
  }

  const keepTwoHead = segments[0] === 'src'
    && segments.length >= 4
    && (segments[1] === 'main' || segments[1] === 'test');
  const headCount = keepTwoHead ? 2 : 1;
  const head = keepTwoHead ? `${segments[0]}/${segments[1]}` : segments[0];
  const remainingMiddle = segments.length - headCount - 1;
  const parentOfLeaf = remainingMiddle >= 2 ? segments[segments.length - 2] : '';

  if (parentOfLeaf) {
    const withParent = `${head}/…/${parentOfLeaf}`;
    if (withParent.length + 1 + leaf.length <= maxChars + 6) {
      return { prefix: withParent, leaf };
    }
  }

  return { prefix: `${head}/…`, leaf };
}

function compactRawNode(node: RawDirNode): DirectoryTreeNode {
  let current = node;
  const segments = [current.segment];
  while (current.files.length === 0 && current.children.size === 1) {
    const [onlyChild] = current.children.values();
    segments.push(onlyChild.segment);
    current = onlyChild;
  }

  const children = [...current.children.values()].map(compactRawNode);
  const files = [...current.files];
  const allFiles = [
    ...children.flatMap((child) => child.allFiles),
    ...files,
  ];

  return {
    id: current.fullPath,
    segments,
    label: segments.join('/'),
    fullPath: current.fullPath,
    isRootGroup: false,
    children,
    files,
    allFiles,
  };
}

export function buildDirectoryTree(
  files: readonly Change[],
  rootDirectoryLabel: string,
): DirectoryTreeNode[] {
  const topLevelOrder: Array<{ kind: 'root' } | { kind: 'dir'; segment: string }> = [];
  const rootFiles: Change[] = [];
  const topDirs = new Map<string, RawDirNode>();
  let hasRootEntry = false;

  for (const file of files) {
    const { directory } = splitPath(file.path);
    if (!directory) {
      if (!hasRootEntry) {
        hasRootEntry = true;
        topLevelOrder.push({ kind: 'root' });
      }
      rootFiles.push(file);
      continue;
    }

    const parts = directory.split('/').filter(Boolean);
    const first = parts[0];
    let current = topDirs.get(first);
    if (!current) {
      current = { segment: first, fullPath: first, children: new Map(), files: [] };
      topDirs.set(first, current);
      topLevelOrder.push({ kind: 'dir', segment: first });
    }

    let cursor: RawDirNode = current;
    let acc = first;
    for (let idx = 1; idx < parts.length; idx += 1) {
      const segment = parts[idx];
      acc = `${acc}/${segment}`;
      let next: RawDirNode | undefined = cursor.children.get(segment);
      if (!next) {
        next = { segment, fullPath: acc, children: new Map(), files: [] };
        cursor.children.set(segment, next);
      }
      cursor = next;
    }
    cursor.files.push(file);
  }

  const result: DirectoryTreeNode[] = [];
  for (const entry of topLevelOrder) {
    if (entry.kind === 'root') {
      result.push({
        id: '__root__',
        segments: [rootDirectoryLabel],
        label: rootDirectoryLabel,
        fullPath: rootDirectoryLabel,
        isRootGroup: true,
        children: [],
        files: rootFiles,
        allFiles: rootFiles,
      });
    } else {
      const raw = topDirs.get(entry.segment);
      if (raw) {
        result.push(compactRawNode(raw));
      }
    }
  }

  return result;
}

export function collectVisibleTreePaths(
  nodes: readonly DirectoryTreeNode[],
  collapsedKeys: ReadonlySet<string>,
  keyPrefix: string,
): string[] {
  const paths: string[] = [];
  const walk = (list: readonly DirectoryTreeNode[]) => {
    for (const node of list) {
      if (collapsedKeys.has(`${keyPrefix}:${node.id}`)) continue;
      walk(node.children);
      for (const file of node.files) {
        paths.push(file.path);
      }
    }
  };
  walk(nodes);
  return paths;
}
