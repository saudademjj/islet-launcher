import { beforeEach, describe, expect, it, vi } from "vitest";

import { migrateFavorites } from "./prefs";

const KEY = "mc-launcher.favorite-worlds";
const worlds = [
  { id: "a", favorite: false, group: "生存" },
  { id: "b", favorite: true, group: null },
  { id: "c", favorite: false, group: null },
];

describe("moving old favorites to the worlds", () => {
  beforeEach(() => localStorage.clear());

  it("stars listed worlds once, keeping their group, then drops the key", async () => {
    localStorage.setItem(KEY, JSON.stringify(["a", "b", "gone"]));
    const organize = vi.fn(async () => true);
    await migrateFavorites(worlds, organize);
    expect(organize).toHaveBeenCalledTimes(1);
    expect(organize).toHaveBeenCalledWith("a", true, "生存");
    expect(localStorage.getItem(KEY)).toBeNull();
  });

  it("keeps the key to try again when saving fails", async () => {
    localStorage.setItem(KEY, JSON.stringify(["a", "c"]));
    const organize = vi.fn(async (id: string) => id === "c");
    await migrateFavorites(worlds, organize);
    expect(organize).toHaveBeenCalledTimes(2);
    expect(localStorage.getItem(KEY)).not.toBeNull();
  });

  it("does nothing without old favorites or with a broken value", async () => {
    const organize = vi.fn(async () => true);
    await migrateFavorites(worlds, organize);
    localStorage.setItem(KEY, "{oops");
    await migrateFavorites(worlds, organize);
    expect(organize).not.toHaveBeenCalled();
  });
});
