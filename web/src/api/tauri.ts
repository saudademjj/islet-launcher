import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open, save } from "@tauri-apps/plugin-dialog";

import type { Backend } from "./backend";
import type { LauncherEvent } from "./types";

export const tauriBackend: Backend = {
  call: (command, args) => invoke(command, args),

  subscribe: (handler) =>
    listen<LauncherEvent>("launcher-event", (event) => handler(event.payload)),

  async pick(options) {
    const result = await open({
      title: options.title,
      directory: options.directory ?? false,
      multiple: options.multiple ?? false,
      filters: options.filters,
    });
    if (!result) return [];
    return Array.isArray(result) ? result : [result];
  },

  async pickSave(options) {
    return (await save({ title: options.title, defaultPath: options.defaultPath, filters: options.filters })) ?? null;
  },

  reveal() {
    const window = getCurrentWindow();
    window.show().then(() => window.setFocus()).catch(() => {});
  },

  setWindowTitle(title) {
    getCurrentWindow().setTitle(title).catch(() => {});
  },

  setWindowTheme(theme) {
    getCurrentWindow().setTheme(theme).catch(() => {});
  },

  windowAction(action) {
    // Closing goes through the same checks as the native button (a running
    // download or game keeps the launcher alive).
    const window = getCurrentWindow();
    const done = action === "minimize" ? window.minimize() : action === "toggleMaximize" ? window.toggleMaximize() : window.close();
    done.catch(() => {});
  },

  async onMaximized(handler) {
    const window = getCurrentWindow();
    const update = () => {
      window.isMaximized().then(handler, () => {});
    };
    update();
    return window.onResized(update);
  },
};
