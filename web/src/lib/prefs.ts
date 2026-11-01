import { create } from "zustand";

// Small per-viewer display preferences. They only change how the launcher
// looks on this machine, so they live in this WebView's storage.
const DEV_KEY = "mc-launcher.dev-info";
const INTRO_KEY = "mc-launcher.intro";

/** A stored switch; `fallback` until it has been turned either way. */
function read(key: string, fallback = false): boolean {
  try {
    const value = localStorage.getItem(key);
    return value === null ? fallback : value === "1";
  } catch {
    return fallback;
  }
}

function write(key: string, on: boolean) {
  try {
    localStorage.setItem(key, on ? "1" : "0");
  } catch {
    // Storage can be unavailable; the choice still applies until restart.
  }
}

export const usePrefs = create<{ devInfo: boolean; intro: boolean; setDevInfo(on: boolean): void; setIntro(on: boolean): void }>((set) => ({
  devInfo: read(DEV_KEY),
  intro: read(INTRO_KEY, true),
  setDevInfo(on) {
    write(DEV_KEY, on);
    set({ devInfo: on });
  },
  setIntro(on) {
    write(INTRO_KEY, on);
    set({ intro: on });
  },
}));

/** A remembered choice among a few values, such as how a list is shown. */
export function readChoice<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  try {
    const value = localStorage.getItem(key);
    return allowed.includes(value as T) ? (value as T) : fallback;
  } catch {
    return fallback;
  }
}

export function saveChoice(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // The choice still applies until the page reloads.
  }
}

// Favorites used to live here; they now belong to each world. Stars left
// in this WebView move over once, then the old key is dropped.
const FAVORITES_KEY = "mc-launcher.favorite-worlds";
export async function migrateFavorites(worlds: { id: string; favorite: boolean; group: string | null }[], organize: (id: string, favorite: boolean, group: string | null) => Promise<boolean>) {
  let stored: unknown;
  try {
    stored = JSON.parse(localStorage.getItem(FAVORITES_KEY) ?? "null");
  } catch {
    stored = null;
  }
  if (!Array.isArray(stored)) return;
  let ok = true;
  for (const world of worlds) {
    if (stored.includes(world.id) && !world.favorite) ok = (await organize(world.id, true, world.group)) && ok;
  }
  try {
    if (ok) localStorage.removeItem(FAVORITES_KEY);
  } catch {
    // Trying again next start is harmless.
  }
}
