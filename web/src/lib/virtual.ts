/**
 * Pure O(1) window & offset calculations for a fixed-row virtual list that
 * optionally has a single variable-height expanded row.
 */

export interface VirtualRange {
  start: number;
  end: number;
}

function activeExtra(count: number, expandedIndex: number | null | undefined, extraHeight: number): number {
  if (expandedIndex == null || expandedIndex < 0 || expandedIndex >= count || extraHeight <= 0) return 0;
  return extraHeight;
}

/** Top offset in pixels for `index`. */
export function rowOffset(
  index: number,
  rowHeight: number,
  expandedIndex?: number | null,
  extraHeight = 0,
): number {
  const shift = expandedIndex != null && expandedIndex >= 0 && extraHeight > 0 && index > expandedIndex ? extraHeight : 0;
  return index * rowHeight + shift;
}

/** Total scrollable content height in pixels. */
export function totalHeight(
  count: number,
  rowHeight: number,
  expandedIndex?: number | null,
  extraHeight = 0,
): number {
  if (count <= 0) return 0;
  return count * rowHeight + activeExtra(count, expandedIndex, extraHeight);
}

/** Row index containing vertical scroll `offset` (clamped to `[0, count - 1]`). */
export function indexAtOffset(
  offset: number,
  count: number,
  rowHeight: number,
  expandedIndex?: number | null,
  extraHeight = 0,
): number {
  if (count <= 0 || rowHeight <= 0) return 0;
  const y = Math.max(0, offset);
  const extra = activeExtra(count, expandedIndex, extraHeight);
  if (extra === 0 || expandedIndex == null) {
    return Math.min(count - 1, Math.floor(y / rowHeight));
  }
  const expandedTop = expandedIndex * rowHeight;
  const expandedBottom = expandedTop + rowHeight + extra;
  if (y < expandedTop) return Math.floor(y / rowHeight);
  if (y < expandedBottom) return expandedIndex;
  return Math.min(count - 1, expandedIndex + 1 + Math.floor((y - expandedBottom) / rowHeight));
}

/** Half-open `[start, end)` range of rows to mount for the current viewport. */
export function visibleRange(
  scrollTop: number,
  viewportHeight: number,
  count: number,
  rowHeight: number,
  overscan: number,
  expandedIndex?: number | null,
  extraHeight = 0,
): VirtualRange {
  if (count <= 0 || rowHeight <= 0) return { start: 0, end: 0 };
  const first = indexAtOffset(scrollTop, count, rowHeight, expandedIndex, extraHeight);
  const bottom = scrollTop + Math.max(0, viewportHeight);
  const last = indexAtOffset(bottom, count, rowHeight, expandedIndex, extraHeight);
  return {
    start: Math.max(0, first - overscan),
    end: Math.min(count, last + 1 + overscan),
  };
}
