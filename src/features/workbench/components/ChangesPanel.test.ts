import { describe, expect, it } from 'vitest';
import type { Change } from '../state/workbench';
import {
  LOCAL_KEEP_DEFAULT_HEIGHT_PX,
  LOCAL_KEEP_MIN_HEIGHT_PX,
  VIRTUALIZE_OVERSCAN_ROWS,
  VIRTUALIZE_ROW_THRESHOLD,
  buildDirectoryTree,
  clampLocalKeepHeight,
  collectVisibleTreePaths,
  computeChangesDragScrollDelta,
  computeVirtualWindow,
  flattenChangesForRender,
  formatDirectoryLabel,
  resolveContextMenuSelection,
  resolveRowSelectionOnClick,
  splitFileNameExt,
} from './ChangesPanel';

describe('ChangesPanel drag-to-edge auto-scroll delta calculation', () => {
  const rect = { top: 100, bottom: 308, height: 208 };

  it('returns 0 when list does not overflow or pointer stays in the middle area', () => {
    expect(
      computeChangesDragScrollDelta(305, rect, { scrollTop: 0, scrollHeight: 208, clientHeight: 208 }),
    ).toBe(0);
    expect(
      computeChangesDragScrollDelta(200, rect, { scrollTop: 120, scrollHeight: 900, clientHeight: 208 }),
    ).toBe(0);
  });

  it('accelerates smoothly downward near the bottom edge and clamps at maxScrollTop', () => {
    const scroll = { scrollTop: 100, scrollHeight: 900, clientHeight: 208 };
    const gentle = computeChangesDragScrollDelta(280, rect, scroll);
    const fast = computeChangesDragScrollDelta(306, rect, scroll);
    expect(gentle).toBeGreaterThan(0);
    expect(fast).toBeGreaterThan(gentle);
    expect(fast).toBeLessThanOrEqual(14);

    // Clamps cleanly to remaining scrollable distance:
    expect(
      computeChangesDragScrollDelta(308, rect, { scrollTop: 687, scrollHeight: 900, clientHeight: 208 }),
    ).toBe(5);
    expect(
      computeChangesDragScrollDelta(308, rect, { scrollTop: 692, scrollHeight: 900, clientHeight: 208 }),
    ).toBe(0);
  });

  it('accelerates smoothly upward near the top edge and clamps at 0', () => {
    const scroll = { scrollTop: 240, scrollHeight: 900, clientHeight: 208 };
    const gentle = computeChangesDragScrollDelta(128, rect, scroll);
    const fast = computeChangesDragScrollDelta(102, rect, scroll);
    expect(gentle).toBeLessThan(0);
    expect(fast).toBeLessThan(gentle);
    expect(fast).toBeGreaterThanOrEqual(-14);

    // Clamps cleanly when near top:
    expect(
      computeChangesDragScrollDelta(100, rect, { scrollTop: 4, scrollHeight: 900, clientHeight: 208 }),
    ).toBe(-4);
    expect(
      computeChangesDragScrollDelta(100, rect, { scrollTop: 0, scrollHeight: 900, clientHeight: 208 }),
    ).toBe(0);
  });

  it('does not prematurely scroll when starting on a top or bottom visible row and dragging inward', () => {
    const scroll = { scrollTop: 200, scrollHeight: 900, clientHeight: 208 };
    // Grabbing top visible row (startY=114 < topThreshold 132) and dragging downward (clientY=122) -> 0:
    expect(computeChangesDragScrollDelta(122, rect, scroll, 1, { startY: 114 })).toBe(0);
    // Moving upward past startY-4 (clientY=108) -> scrolls upward:
    expect(computeChangesDragScrollDelta(108, rect, scroll, 1, { startY: 114 })).toBeLessThan(0);
    // After leaving top edge once (leftTopEdge=true), re-entering top zone at clientY=122 scrolls upward:
    expect(computeChangesDragScrollDelta(122, rect, scroll, 1, { startY: 114, leftTopEdge: true })).toBeLessThan(0);

    // Grabbing bottom visible row (startY=294 > bottomThreshold 276) and dragging upward (clientY=286) -> 0:
    expect(computeChangesDragScrollDelta(286, rect, scroll, 1, { startY: 294 })).toBe(0);
    // Moving downward past startY+4 (clientY=300) -> scrolls downward:
    expect(computeChangesDragScrollDelta(300, rect, scroll, 1, { startY: 294 })).toBeGreaterThan(0);
  });

  it('stops auto-scrolling when pointer moves far outside the changes panel', () => {
    const scroll = { scrollTop: 200, scrollHeight: 900, clientHeight: 208 };
    expect(computeChangesDragScrollDelta(10, rect, scroll)).toBe(0);
    expect(computeChangesDragScrollDelta(420, rect, scroll)).toBe(0);
  });
});

describe('ChangesPanel IDEA-style row selection and batch context menu', () => {
  const orderedPaths = ['a.ts', 'b.ts', 'c.ts', 'd.ts', 'e.ts'];

  it('supports plain single-click, Cmd+Click toggle multi-select, and Shift+Click range selection', () => {
    // Plain click selects single row and sets anchor:
    const step1 = resolveRowSelectionOnClick({
      currentSelected: [],
      anchorPath: null,
      clickedPath: 'b.ts',
      orderedPaths,
      metaOrCtrl: false,
      shift: false,
    });
    expect(step1).toEqual({ selectedPaths: ['b.ts'], anchorPath: 'b.ts' });

    // Shift+Click downward selects contiguous range b.ts..d.ts and preserves anchor b.ts:
    const step2 = resolveRowSelectionOnClick({
      currentSelected: step1.selectedPaths,
      anchorPath: step1.anchorPath,
      clickedPath: 'd.ts',
      orderedPaths,
      metaOrCtrl: false,
      shift: true,
    });
    expect(step2).toEqual({ selectedPaths: ['b.ts', 'c.ts', 'd.ts'], anchorPath: 'b.ts' });

    // Subsequent Shift+Click upward adjusts range from preserved anchor b.ts -> a.ts..b.ts:
    const step3 = resolveRowSelectionOnClick({
      currentSelected: step2.selectedPaths,
      anchorPath: step2.anchorPath,
      clickedPath: 'a.ts',
      orderedPaths,
      metaOrCtrl: false,
      shift: true,
    });
    expect(step3).toEqual({ selectedPaths: ['a.ts', 'b.ts'], anchorPath: 'b.ts' });

    // Cmd+Click adds e.ts and updates anchor to e.ts:
    const step4 = resolveRowSelectionOnClick({
      currentSelected: step3.selectedPaths,
      anchorPath: step3.anchorPath,
      clickedPath: 'e.ts',
      orderedPaths,
      metaOrCtrl: true,
      shift: false,
    });
    expect(step4).toEqual({ selectedPaths: ['a.ts', 'b.ts', 'e.ts'], anchorPath: 'e.ts' });

    // Cmd+Click on already-selected b.ts toggles it off:
    const step5 = resolveRowSelectionOnClick({
      currentSelected: step4.selectedPaths,
      anchorPath: step4.anchorPath,
      clickedPath: 'b.ts',
      orderedPaths,
      metaOrCtrl: true,
      shift: false,
    });
    expect(step5).toEqual({ selectedPaths: ['a.ts', 'e.ts'], anchorPath: 'b.ts' });
  });

  it('preserves multi-selection when right-clicking a selected row and resets to single row otherwise', () => {
    const inside = resolveContextMenuSelection({
      currentSelected: ['d.ts', 'b.ts'],
      anchorPath: 'b.ts',
      clickedPath: 'd.ts',
      orderedPaths,
    });
    expect(inside).toEqual({
      selectedPaths: ['d.ts', 'b.ts'],
      anchorPath: 'b.ts',
      targetPaths: ['b.ts', 'd.ts'],
    });

    const outside = resolveContextMenuSelection({
      currentSelected: ['b.ts', 'd.ts'],
      anchorPath: 'b.ts',
      clickedPath: 'e.ts',
      orderedPaths,
    });
    expect(outside).toEqual({
      selectedPaths: ['e.ts'],
      anchorPath: 'e.ts',
      targetPaths: ['e.ts'],
    });
  });
});

describe('ChangesPanel Local Keep vertical height clamping', () => {
  it('enforces minimum height and preserves normal-changes headroom on desktop and compact viewports', () => {
    expect(clampLocalKeepHeight(20, 540)).toBe(LOCAL_KEEP_MIN_HEIGHT_PX);
    expect(clampLocalKeepHeight(LOCAL_KEEP_DEFAULT_HEIGHT_PX, 540)).toBe(180);
    expect(clampLocalKeepHeight(500, 540)).toBe(380);
    expect(clampLocalKeepHeight(300, 320)).toBe(160);
    expect(clampLocalKeepHeight(240, 0)).toBe(240);
  });
});

function makeChange(id: string, path: string): Change {
  return { id, path, status: 'M', range: { kind: 'unstaged', added: 1, removed: 0 } };
}

describe('ChangesPanel hierarchical compressed directory tree', () => {
  it('compacts single-child chains across multi-module Java paths and keeps root files grouped', () => {
    const files: Change[] = [
      makeChange('1', 'inspur-scm-aggregation-service/src/main/java/com/inspur/industry/module/aggregation/vo/LimsIndicatorsSaveVO.java'),
      makeChange('2', 'inspur-scm-aggregation-service/src/main/java/com/inspur/industry/module/aggregation/service/ShipOrderReportService.java'),
      makeChange('3', 'inspur-scm-aggregation-service/src/test/java/com/inspur/industry/module/aggregation/service/ShipSpendTimeSummaryServiceTest.java'),
      makeChange('4', 'inspur-scm-work-inf/src/main/java/com/inspur/industry/scm/work/bean/LimsIndicatorsSaveDTO.java'),
      makeChange('5', 'README.md'),
    ];

    const tree = buildDirectoryTree(files, '(root)');
    expect(tree.map((node) => node.label)).toEqual([
      'inspur-scm-aggregation-service/src',
      'inspur-scm-work-inf/src/main/java/com/inspur/industry/scm/work/bean',
      '(root)',
    ]);

    const agg = tree[0];
    expect(agg.allFiles.map((f) => f.id)).toEqual(['1', '2', '3']);
    expect(agg.children.map((node) => node.label)).toEqual([
      'main/java/com/inspur/industry/module/aggregation',
      'test/java/com/inspur/industry/module/aggregation/service',
    ]);
    expect(agg.children[0].children.map((node) => node.label)).toEqual(['vo', 'service']);
    expect(tree[1].files.map((f) => f.id)).toEqual(['4']);
    expect(tree[2].isRootGroup).toBe(true);
    expect(tree[2].files.map((f) => f.id)).toEqual(['5']);
  });

  it('keeps short paths intact and middle-elides deep paths while preserving head and leaf segments', () => {
    expect(formatDirectoryLabel(['inspur-meta-starter', 'src', 'main', 'resources'])).toEqual({
      prefix: 'inspur-meta-starter/src/main',
      leaf: 'resources',
    });
    expect(formatDirectoryLabel(['src', 'main', 'java', 'com', 'inspur', 'industry', 'module', 'aggregation'])).toEqual({
      prefix: 'src/main/…/module',
      leaf: 'aggregation',
    });
    expect(formatDirectoryLabel(['src', 'test', 'java', 'com', 'inspur', 'industry', 'module', 'aggregation', 'service'])).toEqual({
      prefix: 'src/test/…/aggregation',
      leaf: 'service',
    });
    expect(formatDirectoryLabel(['inspur-scm-work-inf', 'src', 'main', 'java', 'com', 'inspur', 'industry', 'scm', 'work', 'bean'])).toEqual({
      prefix: 'inspur-scm-work-inf/…/work',
      leaf: 'bean',
    });
    expect(formatDirectoryLabel(['vo'])).toEqual({ prefix: '', leaf: 'vo' });
  });

  it('excludes files inside collapsed branches from visible tree paths and splits short extensions cleanly', () => {
    const files: Change[] = [
      makeChange('1', 'mod-a/src/a.ts'),
      makeChange('2', 'mod-a/test/b.ts'),
      makeChange('3', 'mod-b/c.ts'),
    ];
    const tree = buildDirectoryTree(files, '(root)');
    const collapsed = new Set<string>(['wt:normal:mod-a']);
    expect(collectVisibleTreePaths(tree, collapsed, 'wt:normal')).toEqual(['mod-b/c.ts']);

    expect(splitFileNameExt('InspectPendingMatchServiceImplTest.java')).toEqual({
      stem: 'InspectPendingMatchServiceImplTest',
      ext: '.java',
    });
    expect(splitFileNameExt('Dockerfile')).toEqual({ stem: 'Dockerfile', ext: '' });
    expect(splitFileNameExt('.gitignore')).toEqual({ stem: '.gitignore', ext: '' });
  });
});

describe('ChangesPanel virtualized windowing for large change sets', () => {
  it('stays non-virtualized at or below threshold while tracking exact mixed row heights', () => {
    const files: Change[] = [
      makeChange('root-1', 'README.md'),
      makeChange('nested-1', 'src/app.ts'),
      makeChange('nested-2', 'src/main.ts'),
    ];
    const rows = flattenChangesForRender({
      view: 'list',
      files,
      tree: [],
      collapsedDirs: new Set(),
      keyPrefix: 'wt:normal',
    });
    expect(rows.map((row) => row.estimatedHeightPx)).toEqual([28, 40, 40]);

    const win = computeVirtualWindow(rows, 0, 400);
    expect(win).toEqual({
      virtualized: false,
      startIndex: 0,
      endIndex: 3,
      topSpacerPx: 0,
      bottomSpacerPx: 0,
      totalHeightPx: 108,
    });
  });

  it('windows 500 mixed list rows with exact spacer preservation across top, middle, and bottom scroll', () => {
    const files: Change[] = Array.from({ length: 500 }, (_, i) => (
      makeChange(`f-${i}`, i % 5 === 0 ? `file-${i}.md` : `packages/mod-${i % 10}/file-${i}.ts`)
    ));
    const rows = flattenChangesForRender({
      view: 'list',
      files,
      tree: [],
      collapsedDirs: new Set(),
      keyPrefix: 'wt:normal',
    });
    expect(rows.length).toBeGreaterThan(VIRTUALIZE_ROW_THRESHOLD);

    for (const scrollTop of [0, 4200, 12000, 999999]) {
      const win = computeVirtualWindow(rows, scrollTop, 480);
      expect(win.virtualized).toBe(true);
      expect(win.startIndex).toBeGreaterThanOrEqual(0);
      expect(win.endIndex).toBeLessThanOrEqual(rows.length);
      expect(win.endIndex - win.startIndex).toBeGreaterThan(0);
      expect(win.endIndex - win.startIndex).toBeLessThanOrEqual(20 + VIRTUALIZE_OVERSCAN_ROWS * 2);

      const renderedHeight = rows
        .slice(win.startIndex, win.endIndex)
        .reduce((sum, row) => sum + row.estimatedHeightPx, 0);
      expect(win.topSpacerPx + renderedHeight + win.bottomSpacerPx).toBe(win.totalHeightPx);
    }

    // Zero viewport height falls back safely instead of collapsing to empty render:
    const fallbackWin = computeVirtualWindow(rows, 0, 0);
    expect(fallbackWin.virtualized).toBe(true);
    expect(fallbackWin.startIndex).toBe(0);
    expect(fallbackWin.endIndex).toBeGreaterThan(10);
  });

  it('flattens large directory trees depth-first and omits descendants of collapsed directories', () => {
    const files: Change[] = Array.from({ length: 240 }, (_, i) => (
      makeChange(`t-${i}`, `mod-${Math.floor(i / 40)}/sub-${i % 2}/file-${i}.ts`)
    ));
    const tree = buildDirectoryTree(files, '(root)');
    const expandedRows = flattenChangesForRender({
      view: 'tree',
      files,
      tree,
      collapsedDirs: new Set(),
      keyPrefix: 'wt:normal',
    });
    expect(expandedRows.length).toBeGreaterThan(240);
    expect(expandedRows.some((row) => row.kind === 'dir')).toBe(true);
    expect(expandedRows.filter((row) => row.kind === 'file').length).toBe(240);

    const collapsed = new Set<string>(['wt:normal:mod-0', 'wt:normal:mod-1']);
    const collapsedRows = flattenChangesForRender({
      view: 'tree',
      files,
      tree,
      collapsedDirs: collapsed,
      keyPrefix: 'wt:normal',
    });
    expect(collapsedRows.filter((row) => row.kind === 'file').length).toBe(160);
    // Still virtualizes smoothly even when scrollTop was deep before collapsing:
    const win = computeVirtualWindow(collapsedRows, 99999, 400);
    expect(win.virtualized).toBe(true);
    expect(win.endIndex).toBe(collapsedRows.length);
    expect(win.bottomSpacerPx).toBe(0);
  });
});
