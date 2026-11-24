import { describe, expect, it } from 'vitest';
import {
  computeChangesDragScrollDelta,
  resolveContextMenuSelection,
  resolveRowSelectionOnClick,
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
