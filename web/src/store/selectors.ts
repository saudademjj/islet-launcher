import { useEffect, useState } from "react";
import { useShallow } from "zustand/react/shallow";

import { call } from "../api/backend";
import type { Account, Instance, SystemMemory } from "../api/types";
import { useStore } from "./store";
import type { InstanceState, State, Store } from "./types";

/** Subscribe to a few fields; re-renders only when one of them changes. */
export function usePick<T extends object>(selector: (s: Store) => T): T {
  return useStore(useShallow(selector));
}

export function instanceState(instance: Instance, s: Pick<State, "runningIds" | "launchPending" | "install" | "installQueue">): InstanceState {
  if (s.runningIds.includes(instance.id)) return "running";
  if (s.launchPending === instance.id) return "starting";
  if (s.install?.instanceId === instance.id) return "installing";
  if (s.installQueue.some((queued) => queued.id === instance.id)) return "queued";
  return instance.installed ? "ready" : "notInstalled";
}

export { findInstance } from "./types";

export function useActiveInstance(): Instance | null {
  return useStore((s) => s.instances.find((i) => i.id === s.settings.activeInstanceId) ?? null);
}

/** Automatic memory for one instance, counting its mods the way a launch
    does; falls back to the machine-wide value until the answer arrives. */
export function useAutoMemory(instance: Instance | null): number | null {
  const fallback = useStore((s) => s.systemMemory?.recommendedMb ?? null);
  // A change in the open instance's mod list can move the recommendation.
  const mods = useStore((s) => s.localMods.length);
  const [mb, setMb] = useState<{ id: string; value: number } | null>(null);
  const id = instance?.id ?? null;
  useEffect(() => {
    if (!id) return;
    let live = true;
    call<SystemMemory>("system_memory", { id })
      .then((memory) => live && setMb({ id, value: memory.recommendedMb }))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [id, mods]);
  return mb && mb.id === id ? mb.value : fallback;
}

/** Offline accounts open once a Microsoft account that owns the game has
    signed in; the core checks the same. */
export function hasOwner(accounts: Account[]): boolean {
  return accounts.some((a) => a.provider === "microsoft" && a.ownership === true);
}

export function useHasOwner(): boolean {
  return useStore((s) => hasOwner(s.accounts));
}

export function useActiveAccount(): Account | null {
  return useStore((s) => s.accounts.find((a) => a.id === s.settings.activeAccountId) ?? null);
}

export function overlayOpen(s: Pick<State, "modal" | "logsOpen" | "auth" | "authBusy">): boolean {
  return s.modal !== null || s.logsOpen || s.auth !== null || s.authBusy;
}
