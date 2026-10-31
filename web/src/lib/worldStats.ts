import { useEffect } from "react";
import { create } from "zustand";

import { call } from "../api/backend";
import type { Instance, LocalMod, WorldList } from "../api/types";
import { useStore } from "../store";

// How much is in a world, for its card: saves, and mods when it loads any.
// Both are local reads; they refresh after each session, and when the
// manage panel (where mods and saves are added and removed) closes.

export interface WorldStats {
  saves: number;
  mods: number | null;
  /** How many of the mods are enabled. */
  modsOn: number | null;
}

const useStats = create<{ stats: Record<string, WorldStats & { key: string }>; load(instance: Instance, key: string): Promise<void> }>((set) => ({
  stats: {},
  async load(instance, key) {
    const [saves, mods] = await Promise.all([
      call<WorldList>("list_worlds", { id: instance.id }).then((list) => list.worlds.length).catch(() => 0),
      instance.loader === "vanilla" ? Promise.resolve(null) : call<LocalMod[]>("list_mods", { id: instance.id }).catch(() => null),
    ]);
    const counts = { mods: mods?.length ?? null, modsOn: mods ? mods.filter((mod) => mod.enabled).length : null };
    set((s) => ({ stats: { ...s.stats, [instance.id]: { saves, ...counts, key } } }));
  },
}));

useStore.subscribe((state, previous) => {
  const was = previous.modal?.kind === "manage" ? previous.modal.id : null;
  if (!was || (state.modal?.kind === "manage" && state.modal.id === was)) return;
  const instance = state.instances.find((i) => i.id === was);
  const known = useStats.getState().stats[was];
  if (instance?.installed && known) void useStats.getState().load(instance, known.key);
});

/** Null until read, and for worlds that are not installed. */
export function useWorldStats(instance: Instance): WorldStats | null {
  const key = `${instance.installed}:${instance.lastLaunchedAt ?? 0}:${instance.playSeconds}:${instance.loader}`;
  const stats = useStats((s) => s.stats[instance.id]);
  useEffect(() => {
    if (instance.installed && stats?.key !== key) void useStats.getState().load(instance, key);
    // The key captures every change that matters.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [instance.id, key]);
  return instance.installed && stats ? stats : null;
}
