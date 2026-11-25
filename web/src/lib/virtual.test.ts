import { describe, expect, it } from "vitest";

import { indexAtOffset, rowOffset, totalHeight, visibleRange } from "./virtual";

describe("virtual list geometry", () => {
  it("computes fixed-height offsets and ranges without an expanded row", () => {
    expect(totalHeight(0, 56)).toBe(0);
    expect(totalHeight(1000, 56)).toBe(56_000);
    expect(rowOffset(10, 56)).toBe(560);
    expect(indexAtOffset(0, 1000, 56)).toBe(0);
    expect(indexAtOffset(560, 1000, 56)).toBe(10);
    expect(indexAtOffset(999_999, 1000, 56)).toBe(999);
    expect(visibleRange(560, 280, 1000, 56, 4)).toEqual({ start: 6, end: 20 });
  });

  it("shifts rows below an expanded row without moving rows at or above it", () => {
    const rowHeight = 56;
    const expandedIndex = 5;
    const extraHeight = 200;

    expect(totalHeight(20, rowHeight, expandedIndex, extraHeight)).toBe(20 * 56 + 200);
    expect(rowOffset(0, rowHeight, expandedIndex, extraHeight)).toBe(0);
    expect(rowOffset(5, rowHeight, expandedIndex, extraHeight)).toBe(280);
    expect(rowOffset(6, rowHeight, expandedIndex, extraHeight)).toBe(6 * 56 + 200);
    expect(rowOffset(19, rowHeight, expandedIndex, extraHeight)).toBe(19 * 56 + 200);
  });

  it("maps scroll offsets before, inside, and after an expanded row", () => {
    const rowHeight = 56;
    const expandedIndex = 5;
    const extraHeight = 200; // row 5 spans [280, 536)

    expect(indexAtOffset(279, 50, rowHeight, expandedIndex, extraHeight)).toBe(4);
    expect(indexAtOffset(280, 50, rowHeight, expandedIndex, extraHeight)).toBe(5);
    expect(indexAtOffset(450, 50, rowHeight, expandedIndex, extraHeight)).toBe(5);
    expect(indexAtOffset(535, 50, rowHeight, expandedIndex, extraHeight)).toBe(5);
    expect(indexAtOffset(536, 50, rowHeight, expandedIndex, extraHeight)).toBe(6);
    expect(indexAtOffset(536 + 56 * 2, 50, rowHeight, expandedIndex, extraHeight)).toBe(8);
  });

  it("computes bounded visible windows when scrolling across an expanded row", () => {
    const rowHeight = 56;
    const expandedIndex = 5;
    const extraHeight = 200; // row 5 spans [280, 536)

    // Viewport starts inside the expanded row (y=350) and ends at y=700 (row 8)
    expect(visibleRange(350, 350, 1000, rowHeight, 2, expandedIndex, extraHeight)).toEqual({
      start: 3,
      end: 11,
    });
    // Out-of-bounds expandedIndex is ignored safely
    expect(totalHeight(10, rowHeight, 12, extraHeight)).toBe(560);
    expect(visibleRange(0, 200, 10, rowHeight, 2, 12, extraHeight)).toEqual({ start: 0, end: 6 });
  });
});
