import { convertFileSrc } from "@tauri-apps/api/core";
import { useEffect } from "react";
import { create } from "zustand";

import { call } from "../api/backend";
import type { Instance, Screenshot } from "../api/types";
import { useCovers } from "./biome";

interface ShotStore {
  lists: Record<string, Screenshot[] | undefined>;
  load(id: string): Promise<void>;
  recycle(id: string, name: string): Promise<void>;
}

export const useShots = create<ShotStore>((set) => ({
  lists: {},
  async load(id) {
    try {
      const list = await call<Screenshot[]>("list_screenshots", { id });
      set((s) => ({ lists: { ...s.lists, [id]: list } }));
      dropMissingCover(id, list);
    } catch {
      set((s) => ({ lists: { ...s.lists, [id]: [] } }));
    }
  },
  async recycle(id, name) {
    await call("recycle_screenshot", { id, name });
    set((s) => ({ lists: { ...s.lists, [id]: s.lists[id]?.filter((shot) => shot.name !== name) } }));
    if (useCovers.getState().covers[id]?.name === name) useCovers.getState().setCover(id, null);
  },
}));

/** A cover whose screenshot is gone (deleted here or outside the launcher)
    gives the world its scenery back. Only a list that was read counts, and
    the list holds the newest pictures only: a cover older than all of them
    may simply be past its end, so it stays. */
function dropMissingCover(id: string, list: Screenshot[]) {
  const cover = useCovers.getState().covers[id];
  if (!cover || list.some((shot) => shot.name === cover.name)) return;
  const oldest = list[list.length - 1];
  if (!oldest || cover.modifiedMs >= oldest.modifiedMs) useCovers.getState().setCover(id, null);
}

/** The instance's screenshots, newest first; reloaded after each session. */
export function useScreenshots(instance: Instance | null): Screenshot[] | null {
  const id = instance?.id ?? null;
  const played = instance?.lastLaunchedAt ?? null;
  const playSeconds = instance?.playSeconds ?? 0;
  const list = useShots((s) => (id ? s.lists[id] : undefined));
  useEffect(() => {
    if (id) void useShots.getState().load(id);
  }, [id, played, playSeconds]);
  return list ?? null;
}

/** A small cached thumbnail, or with `full` the picture itself. The time in
    the URL makes a replaced file a new URL. */
export function shotUrl(id: string, shot: Screenshot, full = false): string {
  if (shot.demoUrl) return shot.demoUrl;
  return `${convertFileSrc(`${id}/${shot.name}`, "mcshot")}?v=${shot.modifiedMs}${full ? "&full" : ""}`;
}

/** "2026-10-28_20.15.03.png" was taken at that time; other names use the file time. */
export function shotTime(shot: Screenshot): number {
  const m = shot.name.match(/^(\d{4})-(\d{2})-(\d{2})_(\d{2})\.(\d{2})\.(\d{2})/);
  if (!m) return shot.modifiedMs;
  return new Date(+m[1], +m[2] - 1, +m[3], +m[4], +m[5], +m[6]).getTime();
}
