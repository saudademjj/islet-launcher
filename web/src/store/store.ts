// App state and actions. Components subscribe to the slices they draw, so a
// burst of log lines re-renders the log panel and nothing else.

import { create } from "zustand";

import { call } from "../api/backend";
import { createContentSlice } from "./contentSlice";
import { createEventHandler } from "./events";
import { createLaunchSlice } from "./launchSlice";
import { createUiSlice } from "./uiSlice";
import { initialState, type Store } from "./types";

export const useStore = create<Store>()((set, get) => ({
  ...initialState,
  handleEvent: createEventHandler(set, get),
  ...createUiSlice(set, get),
  ...createLaunchSlice(set, get),
  ...createContentSlice(set, get),
}));

// The backend asks about unsaved edits before quitting from the tray, the
// window or the app menu, so it is told whenever there are some.
let reportedUnsaved = false;
useStore.subscribe((s) => {
  const unsaved = Boolean(s.formGuard || s.pageGuard);
  if (unsaved === reportedUnsaved) return;
  reportedUnsaved = unsaved;
  void call("set_unsaved_changes", { unsaved }).catch(() => {});
});
