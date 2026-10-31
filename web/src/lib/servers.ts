import { useEffect } from "react";
import { create } from "zustand";

import { call, toError } from "../api/backend";
import type { Instance, ServerEdit, ServerEntry, ServerStatus } from "../api/types";

/** A server's live status. Pings happen when a list is shown or refreshed,
    never on a timer, so an idle launcher sends nothing. */
export interface Ping {
  status: ServerStatus | null;
  error: string | null;
  loading: boolean;
  at: number;
}

interface ServerStore {
  /** Server lists by instance id; undefined until loaded. */
  lists: Record<string, ServerEntry[] | undefined>;
  pings: Record<string, Ping>;
  load(id: string): Promise<void>;
  edit(id: string, change: ServerEdit): Promise<boolean>;
  ping(addresses: string[], force?: boolean): void;
}

const FRESH_MS = 60_000;
const PARALLEL = 4;

export const useServers = create<ServerStore>((set, get) => ({
  lists: {},
  pings: {},

  async load(id) {
    try {
      const list = await call<ServerEntry[]>("list_servers", { id });
      set((s) => ({ lists: { ...s.lists, [id]: list } }));
    } catch {
      set((s) => ({ lists: { ...s.lists, [id]: [] } }));
    }
  },

  async edit(id, change) {
    const list = await call<ServerEntry[]>("edit_server", { id, change });
    set((s) => ({ lists: { ...s.lists, [id]: list } }));
    return true;
  },

  ping(addresses, force = false) {
    const now = Date.now();
    const queue = [...new Set(addresses)].filter((address) => {
      const ping = get().pings[address];
      return !ping || (!ping.loading && (force || now - ping.at > FRESH_MS));
    });
    if (queue.length === 0) return;
    set((s) => {
      const pings = { ...s.pings };
      for (const address of queue) pings[address] = { status: pings[address]?.status ?? null, error: null, loading: true, at: now };
      return { pings };
    });
    const worker = async () => {
      for (let address = queue.shift(); address !== undefined; address = queue.shift()) {
        const target = address;
        let next: Ping;
        try {
          next = { status: await call<ServerStatus>("ping_server", { address: target }), error: null, loading: false, at: Date.now() };
        } catch (error) {
          next = { status: null, error: toError(error).message, loading: false, at: Date.now() };
        }
        set((s) => ({ pings: { ...s.pings, [target]: next } }));
      }
    };
    for (let i = 0; i < Math.min(PARALLEL, queue.length); i++) void worker();
  },
}));

/** The instance's server list, reloaded after each session (the game edits
    it), with a status ping for each server. */
export function useServerList(instance: Instance | null, autoPing = true): ServerEntry[] | null {
  const id = instance?.id ?? null;
  const played = instance?.lastLaunchedAt ?? null;
  const playSeconds = instance?.playSeconds ?? 0;
  const list = useServers((s) => (id ? s.lists[id] : undefined));
  useEffect(() => {
    if (id) void useServers.getState().load(id);
  }, [id, played, playSeconds]);
  useEffect(() => {
    if (autoPing && list?.length) useServers.getState().ping(list.map((server) => server.address));
  }, [autoPing, list]);
  return list ?? null;
}

export function usePing(address: string): Ping | null {
  return useServers((s) => s.pings[address] ?? null);
}

/** Five pixel bars like the game's list: fewer as the round trip grows. */
export function signalBars(ping: Ping | null): number {
  const ms = ping?.status?.latencyMs;
  if (ms == null) return 0;
  return ms < 150 ? 5 : ms < 300 ? 4 : ms < 600 ? 3 : ms < 1000 ? 2 : 1;
}
