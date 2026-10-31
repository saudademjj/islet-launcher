import { describe, expect, it } from "vitest";

import { faces, parts } from "./SkinModel";

describe("3D skin geometry", () => {
  it("unfolds a box in the game's texture layout", () => {
    // The head: front at (8,8), back at (24,8), top (8,0), bottom (16,0).
    const [front, back, right, left, top, bottom] = faces([8, 8, 8], [0, 0]).map((f) => f.uv);
    expect(front).toEqual([8, 8, 8, 8]);
    expect(back).toEqual([24, 8, 8, 8]);
    expect(right).toEqual([0, 8, 8, 8]);
    expect(left).toEqual([16, 8, 8, 8]);
    expect(top).toEqual([8, 0, 8, 8]);
    expect(bottom).toEqual([16, 0, 8, 8]);
  });

  it("puts each face half the box away from the centre", () => {
    const outs = faces([4, 12, 2], [0, 0]).map((f) => f.out);
    expect(outs).toEqual([1, 1, 2, 2, 6, 6]);
  });

  it("builds classic and slim modern skins with every overlay", () => {
    const classic = parts(false, false, true);
    expect(classic).toHaveLength(12);
    expect(classic.filter((b) => b.inflate).length).toBe(6);
    const slimArms = parts(true, false, true).filter((b) => b.size[1] === 12 && Math.abs(b.at[0]) > 4);
    expect(slimArms.every((b) => b.size[0] === 3)).toBe(true);
    // Slim arms sit against the body: 4 + 3/2 from the centre.
    expect(slimArms.map((b) => Math.abs(b.at[0]))).toContain(5.5);
  });

  it("mirrors old skins' right limbs onto the left and has no jacket", () => {
    const legacy = parts(true, true, false);
    expect(legacy).toHaveLength(6);
    expect(legacy.filter((b) => b.mirror).map((b) => b.uv)).toEqual([[40, 16], [0, 16]]);
    // Old skins are never slim.
    expect(legacy.filter((b) => b.uv[0] === 40).every((b) => b.size[0] === 4)).toBe(true);
  });
});
