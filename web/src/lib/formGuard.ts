import { useEffect, useRef } from "react";

import { useStore } from "../store";

/** Registers a tab's unsaved edits, so closing the dialog or switching
    tabs asks before throwing them away. */
export function useFormGuard(dirty: boolean, save: () => Promise<boolean>) {
  const latest = useRef(save);
  latest.current = save;
  useEffect(() => {
    useStore.getState().setFormGuard(dirty ? { save: () => latest.current() } : null);
  }, [dirty]);
  useEffect(() => () => useStore.getState().setFormGuard(null), []);
}
