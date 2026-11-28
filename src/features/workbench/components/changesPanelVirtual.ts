import { useEffect, useState, type RefObject } from 'react';
import { splitPath, type Change } from '../state/workbench';
import type { DirectoryTreeNode } from './changesPanelTree';

export const VIRTUALIZE_ROW_THRESHOLD = 120;
export const VIRTUALIZE_OVERSCAN_ROWS = 16;
export const ROW_HEIGHT_LIST_ROOT_PX = 28;
export const ROW_HEIGHT_LIST_NESTED_PX = 40;
export const ROW_HEIGHT_TREE_FILE_PX = 26;
export const ROW_HEIGHT_TREE_DIR_PX = 26;
export const ROW_HEIGHT_TREE_MODULE_DIR_PX = 28;
const DEFAULT_FALLBACK_VIEWPORT_PX = 480;

export type VirtualChangeRow =
  | {
      kind: 'file';
      key: string;
      file: Change;
      grouped: boolean;
      treeDepth?: number;
      estimatedHeightPx: number;
    }
  | {
      kind: 'dir';
      key: string;
      node: DirectoryTreeNode;
      depth: number;
      isModuleHead: boolean;
      isCollapsed: boolean;
      estimatedHeightPx: number;
    };

export interface VirtualWindow {
  virtualized: boolean;
  startIndex: number;
  endIndex: number;
  topSpacerPx: number;
  bottomSpacerPx: number;
  totalHeightPx: number;
}

export function flattenChangesForRender(options: {
  view: 'list' | 'tree';
  files: readonly Change[];
  tree: readonly DirectoryTreeNode[];
  collapsedDirs: ReadonlySet<string>;
  keyPrefix: string;
}): VirtualChangeRow[] {
  if (options.view === 'list') {
    return options.files.map((file) => {
      const { directory } = splitPath(file.path);
      return {
        kind: 'file',
        key: file.id,
        file,
        grouped: false,
        estimatedHeightPx: directory ? ROW_HEIGHT_LIST_NESTED_PX : ROW_HEIGHT_LIST_ROOT_PX,
      };
    });
  }

  const rows: VirtualChangeRow[] = [];
  const visit = (nodes: readonly DirectoryTreeNode[], depth: number) => {
    for (const node of nodes) {
      const nodeKey = `${options.keyPrefix}:${node.id}`;
      const isCollapsed = options.collapsedDirs.has(nodeKey);
      const isModuleHead = depth === 0 && node.children.length > 0;
      rows.push({
        kind: 'dir',
        key: nodeKey,
        node,
        depth,
        isModuleHead,
        isCollapsed,
        estimatedHeightPx: isModuleHead && rows.length > 0
          ? ROW_HEIGHT_TREE_MODULE_DIR_PX
          : ROW_HEIGHT_TREE_DIR_PX,
      });
      if (!isCollapsed) {
        if (node.children.length > 0) {
          visit(node.children, depth + 1);
        }
        for (const file of node.files) {
          rows.push({
            kind: 'file',
            key: file.id,
            file,
            grouped: true,
            treeDepth: depth + 1,
            estimatedHeightPx: ROW_HEIGHT_TREE_FILE_PX,
          });
        }
      }
    }
  };
  visit(options.tree, 0);
  return rows;
}

function findFirstRowAtOffset(offsets: readonly number[], count: number, offsetPx: number): number {
  let lo = 0;
  let hi = count;
  while (lo < hi) {
    const mid = (lo + hi) >>> 1;
    if (offsets[mid + 1] <= offsetPx) {
      lo = mid + 1;
    } else {
      hi = mid;
    }
  }
  return Math.min(lo, count);
}

function findEndRowAfterOffset(offsets: readonly number[], count: number, offsetPx: number): number {
  let lo = 0;
  let hi = count;
  while (lo < hi) {
    const mid = (lo + hi) >>> 1;
    if (offsets[mid] < offsetPx) {
      lo = mid + 1;
    } else {
      hi = mid;
    }
  }
  return Math.min(lo, count);
}

export function computeVirtualWindow(
  rows: readonly Pick<VirtualChangeRow, 'estimatedHeightPx'>[],
  scrollTop: number,
  viewportHeight: number,
  options?: { threshold?: number; overscan?: number },
): VirtualWindow {
  const count = rows.length;
  const threshold = options?.threshold ?? VIRTUALIZE_ROW_THRESHOLD;
  const overscan = options?.overscan ?? VIRTUALIZE_OVERSCAN_ROWS;

  const offsets = new Array<number>(count + 1);
  offsets[0] = 0;
  for (let i = 0; i < count; i += 1) {
    offsets[i + 1] = offsets[i] + Math.max(1, rows[i].estimatedHeightPx);
  }
  const totalHeightPx = offsets[count];

  if (count === 0 || count <= threshold) {
    return {
      virtualized: false,
      startIndex: 0,
      endIndex: count,
      topSpacerPx: 0,
      bottomSpacerPx: 0,
      totalHeightPx,
    };
  }

  const safeViewport = viewportHeight > 0 ? viewportHeight : DEFAULT_FALLBACK_VIEWPORT_PX;
  const maxScrollTop = Math.max(0, totalHeightPx - safeViewport);
  const safeScrollTop = Math.max(0, Math.min(scrollTop, maxScrollTop));

  const firstVisible = findFirstRowAtOffset(offsets, count, safeScrollTop);
  const lastVisibleExclusive = Math.max(
    firstVisible + 1,
    findEndRowAfterOffset(offsets, count, safeScrollTop + safeViewport),
  );

  const startIndex = Math.max(0, firstVisible - overscan);
  const endIndex = Math.min(count, lastVisibleExclusive + overscan);

  return {
    virtualized: true,
    startIndex,
    endIndex,
    topSpacerPx: offsets[startIndex],
    bottomSpacerPx: Math.max(0, totalHeightPx - offsets[endIndex]),
    totalHeightPx,
  };
}

export function useVirtualScroll(containerRef: RefObject<HTMLDivElement | null>, enabled: boolean) {
  const [metrics, setMetrics] = useState({ scrollTop: 0, viewportHeight: 0 });

  useEffect(() => {
    if (!enabled) return;
    const el = containerRef.current;
    if (!el) return;

    let rafId: number | null = null;
    const sync = () => {
      rafId = null;
      const nextTop = Math.round(el.scrollTop);
      const nextHeight = Math.round(el.clientHeight);
      setMetrics((prev) => (
        prev.scrollTop === nextTop && prev.viewportHeight === nextHeight
          ? prev
          : { scrollTop: nextTop, viewportHeight: nextHeight }
      ));
    };

    const scheduleSync = () => {
      if (rafId !== null) return;
      if (typeof window !== 'undefined' && typeof window.requestAnimationFrame === 'function') {
        rafId = window.requestAnimationFrame(sync);
      } else {
        sync();
      }
    };

    sync();
    el.addEventListener('scroll', scheduleSync, { passive: true });
    let observer: ResizeObserver | null = null;
    if (typeof ResizeObserver !== 'undefined') {
      observer = new ResizeObserver(scheduleSync);
      observer.observe(el);
    }

    return () => {
      el.removeEventListener('scroll', scheduleSync);
      observer?.disconnect();
      if (rafId !== null && typeof window !== 'undefined' && typeof window.cancelAnimationFrame === 'function') {
        window.cancelAnimationFrame(rafId);
      }
    };
  }, [containerRef, enabled]);

  return metrics;
}
