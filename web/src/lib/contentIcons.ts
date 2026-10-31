import { useEffect, useState } from "react";
import { create } from "zustand";

import { call } from "../api/backend";
import type { LocalMod, PackInsight, PackLists } from "../api/types";

// Real pictures for local content. Mods carry their icon inside the jar, so
// it is read locally and shows at once, offline too. Resource packs usually
// carry pack.png (listed with the pack); shader packs never carry one, so
// their files are identified on Modrinth by hash and take the project's icon.

/** Icons from inside the mod jars, by file name; re-read when the list changes. */
export function useModIcons(id: string, mods: LocalMod[]): Record<string, string> {
  const [icons, setIcons] = useState<{ key: string; value: Record<string, string> } | null>(null);
  const key = `${id}:${mods.map((m) => `${m.name}/${m.bytes}`).join("|")}`;
  useEffect(() => {
    if (mods.length === 0) return;
    let live = true;
    call<Record<string, string>>("mod_icons", { id })
      .then((value) => live && setIcons({ key, value }))
      .catch(() => live && setIcons({ key, value: {} }));
    return () => {
      live = false;
    };
  }, [key]);
  // Keep the previous icons while a changed list is read again.
  return icons?.value ?? {};
}

const usePackStore = create<{
  byInstance: Record<string, { key: string; byFile: Record<string, PackInsight> }>;
  load(id: string, key: string): Promise<void>;
}>((set, get) => ({
  byInstance: {},
  async load(id, key) {
    if (get().byInstance[id]?.key === key) return;
    // Mark it first, so a second caller does not ask again.
    set((s) => ({ byInstance: { ...s.byInstance, [id]: { key, byFile: s.byInstance[id]?.byFile ?? {} } } }));
    try {
      const list = await call<PackInsight[]>("inspect_packs", { id });
      const byFile = Object.fromEntries(list.map((item) => [`${item.kind}:${item.file}`, item]));
      if (get().byInstance[id]?.key === key) set((s) => ({ byInstance: { ...s.byInstance, [id]: { key, byFile } } }));
    } catch {
      // Offline or Modrinth unreachable: the packs still show, with their own pictures.
    }
  },
}));

/** Modrinth's knowledge of the instance's zipped packs, by `kind:file`. */
export function usePackInsights(id: string, packs: PackLists | null): Record<string, PackInsight> {
  const zips = packs
    ? [...packs.resourcepacks.map((p) => ({ ...p, kind: "resourcepack" })), ...packs.shaderpacks.map((p) => ({ ...p, kind: "shader" }))].filter((p) => !p.folder)
    : [];
  const key = zips.map((p) => `${p.kind}:${p.name}/${p.bytes}`).join("|");
  const byFile = usePackStore((s) => s.byInstance[id]?.byFile);
  useEffect(() => {
    if (packs && zips.length > 0) void usePackStore.getState().load(id, key);
  }, [id, key]);
  return byFile ?? {};
}
