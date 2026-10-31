import { useEffect, useState } from "react";

import { call } from "../api/backend";
import type { Instance, WorldList } from "../api/types";
import { useStore } from "../store";

/** An instance's singleplayer worlds, reloaded after each session since
    playing changes them, and after file operations (import, rename …). */
export function useWorlds(instance: Instance | null): WorldList | null {
  const [list, setList] = useState<{ id: string; value: WorldList } | null>(null);
  const id = instance?.id ?? null;
  const installed = instance?.installed ?? false;
  const played = instance?.lastLaunchedAt ?? null;
  const playSeconds = instance?.playSeconds ?? 0;
  const changed = useStore((s) => s.filesChanged);
  useEffect(() => {
    if (!id) return;
    let live = true;
    call<WorldList>("list_worlds", { id })
      .then((value) => live && setList({ id, value }))
      .catch(() => live && setList({ id, value: { worlds: [], quickPlay: false } }));
    return () => {
      live = false;
    };
  }, [id, installed, played, playSeconds, changed]);
  return list && list.id === id ? list.value : null;
}
