import { tauriBackend } from "./tauri";
import type { ErrorPayload, LauncherEvent } from "./types";

/** True inside the Tauri window; a plain browser gets the demo backend. */
export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export interface PickOptions {
  title: string;
  directory?: boolean;
  multiple?: boolean;
  filters?: { name: string; extensions: string[] }[];
}

export interface Backend {
  call<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  subscribe(handler: (event: LauncherEvent) => void): Promise<() => void>;
  /** Native file/folder chooser; resolves to [] when cancelled. */
  pick(options: PickOptions): Promise<string[]>;
  /** Native "save as" chooser; null when cancelled. */
  pickSave(options: { title: string; defaultPath: string; filters?: { name: string; extensions: string[] }[] }): Promise<string | null>;
  /** The window starts hidden so it never flashes an empty page. */
  reveal(): void;
  /** The native window title, in the interface language. */
  setWindowTitle(title: string): void;
  /** Keeps the native title bar (traffic lights) in step with the app theme. */
  setWindowTheme(theme: "light" | "dark" | null): void;
  /** The buttons of the title bar the page draws on Windows. */
  windowAction(action: WindowAction): void;
  /** Reports whether the window is maximized, now and after every resize. */
  onMaximized(handler: (maximized: boolean) => void): Promise<() => void>;
}

export type WindowAction = "minimize" | "toggleMaximize" | "close";

let current: Backend | null = null;

export async function loadBackend(): Promise<Backend> {
  if (!current) {
    // The real backend is bundled up front (it is on the startup path); the
    // demo backend only loads in a plain browser.
    current = inTauri ? tauriBackend : (await import("./mock")).mockBackend;
  }
  return current;
}

export function backend(): Backend {
  if (!current) throw new Error("backend not loaded");
  return current;
}

export function call<T = void>(command: string, args?: Record<string, unknown>): Promise<T> {
  return backend().call<T>(command, args);
}

/** Commands reject with the Rust ErrorPayload; anything else is wrapped. */
export function toError(error: unknown): ErrorPayload {
  if (error && typeof error === "object" && "message" in error) {
    const e = error as Partial<ErrorPayload>;
    return { code: e.code ?? "error", message: String(e.message), detail: e.detail ?? null };
  }
  return { code: "error", message: String(error), detail: null };
}
