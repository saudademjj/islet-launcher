import { describe, expect, it } from "vitest";

import { aliasesOf, matchItem, matchWord, queryWords } from "./search";

describe("search", () => {
  it("splits queries into lower-case words", () => {
    expect(queryWords("  Sodium  FABRIC ")).toEqual(["sodium", "fabric"]);
  });

  it("ranks a prefix above a match inside the text", () => {
    const prefix = matchWord("sodium extra", "sod");
    const inside = matchWord("extra sodium", "sod");
    expect(prefix && inside && prefix.score > inside.score).toBe(true);
    expect(prefix?.marks).toEqual([0, 1, 2]);
  });

  it("matches Chinese by pinyin and initials", () => {
    expect(matchWord("机械动力", "jixie")).not.toBeNull();
    expect(matchWord("机械动力", "jxdl")?.marks).toEqual([0, 1, 2, 3]);
  });

  it("allows loose matches only in labels", () => {
    expect(matchWord("sodium", "sdm")).not.toBeNull();
    expect(matchWord("sodium", "sdm", false)).toBeNull();
    expect(matchWord("sodium", "zz")).toBeNull();
  });

  it("needs every word to match somewhere", () => {
    expect(matchItem("生存 1.21", ["fabric"], ["生存", "fabric"])).not.toBeNull();
    expect(matchItem("生存 1.21", ["fabric"], ["生存", "forge"])).toBeNull();
  });

  it("knows mods by both names", () => {
    expect(aliasesOf("我的机械动力世界").toLowerCase()).toContain("create");
  });
});
