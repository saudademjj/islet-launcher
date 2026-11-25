import { describe, expect, it } from "vitest";

// Relative import to the zero-dependency WOFF2 verifier at repo root.
import { checkFontSubset } from "../../../../scripts/check-font.mjs";

describe("pixel font subset", () => {
  it("contains every CJK and fullwidth UI character used in production code", () => {
    const result = checkFontSubset();
    expect(result.total).toBeGreaterThan(500);
    expect(result.glyphCount).toBeGreaterThan(result.total);
    expect(result.missing).toEqual([]);
  });
});
