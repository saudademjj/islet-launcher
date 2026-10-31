import { describe, expect, it } from "vitest";

import { filterCategories, MODPACK_GROUPS, modpackChoice } from "./contentFilters";

describe("modpack filters", () => {
  it("keeps choices that exist on the source", () => {
    expect(modpackChoice("modrinth", { loader: "fabric", category: "optimization" })).toEqual({ loader: "fabric", category: "optimization" });
    expect(modpackChoice("curseforge", { loader: "fabric", category: "skyblock" })).toEqual({ loader: "fabric", category: "skyblock" });
  });

  it("drops a category the other source lacks when switching", () => {
    expect(modpackChoice("curseforge", { loader: "forge", category: "optimization" })).toEqual({ loader: "forge" });
    expect(modpackChoice("modrinth", { category: "skyblock" })).toEqual({});
  });

  it("ignores unknown groups and values", () => {
    expect(modpackChoice("modrinth", { loader: "rift", sort: "downloads" })).toEqual({});
  });

  it("offers loader and type groups with unique values", () => {
    expect(MODPACK_GROUPS.map((g) => g.id)).toEqual(["loader", "category"]);
    for (const group of MODPACK_GROUPS) expect(new Set(group.options.map((o) => o.value)).size).toBe(group.options.length);
  });
});

describe("content filters", () => {
  it("sends only known tags, in group order", () => {
    expect(filterCategories("mod", { nope: "x" })).toEqual([]);
    const tags = filterCategories("mod", { category: "magic" });
    expect(tags.every((tag) => typeof tag === "string")).toBe(true);
  });
});
