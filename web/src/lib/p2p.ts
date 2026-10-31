// P2P rooms: the room's status, polled from the backend while a room is open
// or being set up, so the sidebar, status bar and multiplayer page agree.

import { useEffect } from "react";
import { create } from "zustand";

import { call, toError } from "../api/backend";
import type { LanGame, P2pDefaults, P2pLevel, P2pStatus } from "../api/types";
import { useStore } from "../store";
import { t } from "./i18n";

const NODES_KEY = "p2p-nodes";

/** The user's own shared nodes, or null for the built-in list. */
export function loadNodes(): string[] | null {
  try {
    const saved = JSON.parse(localStorage.getItem(NODES_KEY) ?? "null");
    return Array.isArray(saved) && saved.length ? saved.map(String) : null;
  } catch {
    return null;
  }
}

export function saveNodes(nodes: string[] | null) {
  try {
    if (nodes?.length) localStorage.setItem(NODES_KEY, JSON.stringify(nodes));
    else localStorage.removeItem(NODES_KEY);
  } catch {
    /* the built-in list still works */
  }
}

/** Mirrors the backend's check of a node address. */
export const validNode = (url: string) => /^(tcp|udp|ws|wss|quic|wg|txt|srv|http|https|faketcp):\/\/[A-Za-z0-9.\-_:/[\]@%?=&~+]+$/.test(url) && url.length <= 200;

/** Room codes are 21 Crockford base32 characters, shown in three groups of
    seven; look-alikes are read the way the backend reads them. */
export function formatCode(text: string): string {
  const clean = text
    .toUpperCase()
    .replace(/O/g, "0")
    .replace(/[IL]/g, "1")
    .replace(/[^0-9A-HJKMNP-TV-Z]/g, "")
    .slice(0, 21);
  return clean.match(/.{1,7}/g)?.join("-") ?? "";
}

export const codeComplete = (code: string) => code.replace(/-/g, "").length === 21;

type Tone = "accent" | "warn" | "danger" | "neutral";

export const LEVELS: Record<P2pLevel, { label: string; tone: Tone; pulse: boolean }> = {
  good: { label: t("连接良好", "Good"), tone: "accent", pulse: false },
  fair: { label: t("连接一般", "Fair"), tone: "warn", pulse: false },
  starting: { label: t("正在连接", "Connecting"), tone: "warn", pulse: true },
  poor: { label: t("连接较差", "Poor"), tone: "danger", pulse: false },
  down: { label: t("连接中断", "Down"), tone: "danger", pulse: false },
  idle: { label: t("等待中", "Waiting"), tone: "neutral", pulse: false },
};

interface P2pStore {
  status: P2pStatus | null;
  defaults: P2pDefaults | null;
  /** A host or join call is in flight. */
  busy: boolean;
  error: string;
  /** The card a command asked for; the page focuses it once. */
  intent: "host" | "join" | null;
  focus(card: "host" | "join"): void;
  refresh(): Promise<void>;
  loadDefaults(): Promise<void>;
  detect(): Promise<LanGame[]>;
  host(port: number, name: string): Promise<boolean>;
  join(code: string, name: string): Promise<boolean>;
  stop(): Promise<void>;
  clearError(): void;
}

let polling = false;

export const useP2p = create<P2pStore>()((set, get) => {
  const open = async (command: string, args: Record<string, unknown>) => {
    set({ busy: true, error: "" });
    try {
      await call(command, { ...args, nodes: loadNodes() ?? [] });
      return true;
    } catch (e) {
      set({ error: toError(e).message });
      return false;
    } finally {
      set({ busy: false });
      await get().refresh();
      // The component may have been downloaded just now.
      void get().loadDefaults();
    }
  };
  return {
    status: null,
    defaults: null,
    busy: false,
    error: "",
    intent: null,
    focus(card) {
      set({ intent: card });
      useStore.getState().setPage("multiplayer");
    },
    async refresh() {
      // A slow poll (a ping waits up to 3 s) must not pile up behind itself.
      if (polling) return;
      polling = true;
      try {
        set({ status: await call<P2pStatus>("p2p_status") });
      } catch {
        /* the next poll tries again */
      } finally {
        polling = false;
      }
    },
    async loadDefaults() {
      try {
        set({ defaults: await call<P2pDefaults>("p2p_defaults") });
      } catch {
        /* optional hints */
      }
    },
    detect: () => call<LanGame[]>("p2p_detect_lan"),
    host: (port, name) => open("p2p_host", { port, name }),
    join: (code, name) => open("p2p_join", { code, name }),
    async stop() {
      try {
        await call("p2p_stop");
      } catch (e) {
        set({ error: toError(e).message });
      }
      await get().refresh();
    },
    clearError: () => set({ error: "" }),
  };
});

/** Whether a room is open or being set up. */
export const roomLive = (s: Pick<P2pStore, "busy" | "status">) => s.busy || !!s.status?.active || !!s.status?.preparing;

/** Mounted once: reads the status at start, then every 2 s while live, and
    every half second while the room is being set up so a short download
    still shows its progress. */
export function useP2pPolling() {
  const live = useP2p(roomLive);
  const setting = useP2p((s) => s.busy || !!s.status?.preparing);
  useEffect(() => {
    void useP2p.getState().refresh();
  }, []);
  useEffect(() => {
    if (!live) return;
    const timer = setInterval(() => void useP2p.getState().refresh(), setting ? 500 : 2000);
    return () => clearInterval(timer);
  }, [live, setting]);
}
