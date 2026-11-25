import { backend, call, toError } from "../api/backend";
import type { LocalMod, ModInsight, PackLists } from "../api/types";
import { filterCategories } from "../lib/contentFilters";
import { t } from "../lib/i18n";
import { beginOperation, emptySearch, type Actions, type ModSearch, type SliceGet, type SliceSet } from "./types";

// Reads of the manage panel's lists are numbered: a slow answer for an
// earlier instance, or an earlier read, never replaces a newer one.
let modsRead = 0;
let packsRead = 0;
let keySeq = 0;
const newKey = () => `k${++keySeq}-${Date.now()}`;

type ContentActionKeys =
  | "fileOperation"
  | "reloadLocalMods"
  | "reloadPacks"
  | "inspectMods"
  | "updateMods"
  | "refreshLoaderVersions"
  | "searchMods"
  | "setBrowseKind"
  | "loadModVersions"
  | "installOptifine"
  | "installMod"
  | "chooseFiles"
  | "chooseSavePath"
  | "relocate";

export function createContentSlice(set: SliceSet, get: SliceGet): Pick<Actions, ContentActionKeys> {
  return {
    async fileOperation(id, operation, quiet) {
      if (get().fileBusy) return;
      const state = await beginOperation(set, async () => {
        try {
          await call("file_operation", { id, operation });
          return { task: null };
        } catch (error) {
          get().toast(toError(error).message, "error");
          return null;
        }
      });
      if (state === "running" && !quiet) get().toast(t("正在处理文件…", "Working on files…"));
    },

    async reloadLocalMods() {
      const modal = get().modal;
      if (modal?.kind !== "manage") return;
      const { id } = modal;
      const read = ++modsRead;
      const mods = await call<LocalMod[]>("list_mods", { id }).catch(() => []);
      const now = get().modal;
      if (read === modsRead && now?.kind === "manage" && now.id === id) set({ localMods: mods });
    },

    async reloadPacks() {
      const modal = get().modal;
      if (modal?.kind !== "manage") return;
      const { id } = modal;
      const read = ++packsRead;
      const packs = await call<PackLists>("list_packs", { id }).catch(() => null);
      const now = get().modal;
      if (read === packsRead && now?.kind === "manage" && now.id === id) set({ localPacks: packs ?? { resourcepacks: [], shaderpacks: [] } });
    },

    async inspectMods() {
      const modal = get().modal;
      if (modal?.kind !== "manage") return;
      const id = modal.id;
      set({ modInsights: { ...get().modInsights, id, loading: true, error: null } });
      try {
        const list = await call<ModInsight[]>("inspect_mods", { id });
        if (get().modInsights.id !== id) return;
        set({ modInsights: { id, byFile: Object.fromEntries(list.map((item) => [item.file, item])), loading: false, error: null, checked: true } });
      } catch (error) {
        if (get().modInsights.id !== id) return;
        set({ modInsights: { ...get().modInsights, loading: false, error: toError(error).message, checked: true } });
      }
    },

    async updateMods(id, versions) {
      if (versions.length === 0) return;
      const state = await beginOperation(set, async () => {
        const task = await get().run<string>("update_mods", { id, versions });
        return task ? { task } : null;
      });
      if (state === "running") get().toast(versions.length > 1 ? t(`正在更新 ${versions.length} 个模组…`, `Updating ${versions.length} mods…`) : t("正在更新模组…", "Updating mod…"));
    },

    refreshLoaderVersions(loader, minecraft) {
      const key = newKey();
      if (loader === "vanilla" || !minecraft) {
        set({ loader: { key, versions: [], loading: false, error: null } });
        return;
      }
      set({ loader: { key, versions: [], loading: true, error: null } });
      void call("load_loader_versions", { key, loader, minecraft }).catch((e) =>
        set({ loader: { key, versions: [], loading: false, error: toError(e).message } }),
      );
    },

    searchMods(id, query, offset, kind, refine) {
      const key = newKey();
      const current = get().modSearch;
      const nextKind = kind ?? current.kind;
      const source = refine?.source ?? current.source;
      const same = nextKind === current.kind && source === current.source;
      const base: ModSearch = same ? { ...current, key, offset, loading: true, error: null } : { ...emptySearch(nextKind, source), key, offset, loading: true };
      const next: ModSearch = { ...base, filters: refine?.filters ?? base.filters, sort: refine?.sort ?? base.sort };
      set({
        modSearch: next,
        modVersions: { key: "", project: null, versions: [], loading: false },
      });
      // CurseForge has its own category numbers; the Modrinth tags stay off there.
      const categories = source === "modrinth" ? filterCategories(nextKind, next.filters) : [];
      void call("search_mods", { id, key, query, offset, kind: nextKind, categories, sort: next.sort, source }).catch((e) => {
        if (get().modSearch.key === key) set({ modSearch: { ...get().modSearch, loading: false, error: toError(e).message } });
      });
    },

    setBrowseKind(kind) {
      set({ modSearch: emptySearch(kind, get().modSearch.source), modVersions: { key: "", project: null, versions: [], loading: false } });
    },

    loadModVersions(id, project) {
      const key = newKey();
      set({ modVersions: { key, project, versions: [], loading: true } });
      void call("load_mod_versions", { id, key, project, kind: get().modSearch.kind }).catch((e) => {
        if (get().modVersions.key !== key) return;
        set({ modVersions: { key, project, versions: [], loading: false } });
        get().toast(toError(e).message, "error");
      });
    },

    async installOptifine(id) {
      const state = await beginOperation(set, async () => {
        const task = await get().run<string>("install_optifine", { id });
        return task ? { task } : null;
      });
      if (state === "running") get().toast(t("正在下载 OptiFine…", "Downloading OptiFine…"));
    },

    async installMod(id, version, save) {
      const kind = get().modSearch.kind;
      const state = await beginOperation(set, async () => {
        const task = kind === "mod" ? await get().run<string>("install_mod", { id, version }) : await get().run<string>("install_pack", { id, version, kind, save: save ?? null });
        return task ? { task } : null;
      });
      if (state === "running") get().toast(kind === "mod" ? t("正在下载模组及前置…", "Downloading mod and dependencies…") : t("正在下载文件…", "Downloading files…"));
    },

    async chooseFiles(purpose) {
      const options = {
        mods: { title: t("添加模组", "Add mods"), multiple: true, filters: [{ name: t("模组", "Mods"), extensions: ["jar", "litemod"] }] },
        restore: { title: t("恢复备份", "Restore backup"), filters: [{ name: t("备份", "Backups"), extensions: ["zip"] }] },
        pack: { title: t("选择整合包", "Choose modpack"), filters: [{ name: t("整合包（Modrinth .mrpack / CurseForge .zip）", "Modpack (Modrinth .mrpack / CurseForge .zip)"), extensions: ["mrpack", "zip"] }] },
        storage: { title: t("选择空文件夹作为下载与游戏数据目录", "Choose an empty folder for downloads and game data"), directory: true },
        java: { title: t("选择 Java", "Choose Java") },
        "java-root": { title: t("选择要扫描的 Java 目录", "Choose a folder to scan for Java"), directory: true },
        resourcePacks: { title: t("添加资源包", "Add resource packs"), multiple: true, filters: [{ name: t("资源包", "Resource packs"), extensions: ["zip"] }] },
        shaders: { title: t("添加光影", "Add shader packs"), multiple: true, filters: [{ name: t("光影包", "Shader packs"), extensions: ["zip"] }] },
        datapacks: { title: t("添加数据包", "Add datapacks"), multiple: true, filters: [{ name: t("数据包", "Datapacks"), extensions: ["zip"] }] },
        saveZip: { title: t("导入存档", "Import save"), filters: [{ name: t("存档压缩包", "Save archive"), extensions: ["zip"] }] },
        saveFolder: { title: t("选择本地存档文件夹", "Choose a local save folder"), directory: true },
        gameDir: { title: t("选择游戏目录（.minecraft、版本文件夹或其他启动器目录）", "Choose a game folder (.minecraft, version folder, or another launcher)"), directory: true },
        serverDir: { title: t("选择一个空文件夹放服务端", "Choose an empty folder for the server"), directory: true },
        sshKey: { title: t("选择 SSH 私钥", "Choose an SSH private key") },
        skin: { title: t("选择皮肤图片", "Choose skin image"), filters: [{ name: t("皮肤 PNG", "Skin PNG"), extensions: ["png"] }] },
      }[purpose];
      try {
        return await backend().pick(options);
      } catch (error) {
        get().toast(t(`无法打开文件选择器：${toError(error).message}`, `Couldn't open the file chooser: ${toError(error).message}`), "error");
        return [];
      }
    },

    async chooseSavePath(title, defaultName, extension) {
      try {
        return await backend().pickSave({ title, defaultPath: defaultName, filters: [{ name: extension.toUpperCase(), extensions: [extension] }] });
      } catch (error) {
        get().toast(t(`无法打开保存位置：${toError(error).message}`, `Couldn't open the save dialog: ${toError(error).message}`), "error");
        return null;
      }
    },

    async relocate() {
      const [dir] = await get().chooseFiles("storage");
      if (!dir) return;
      const state = await beginOperation(set, async () => {
        try {
          await call("relocate_storage", { destination: dir });
          return { task: null };
        } catch (error) {
          get().toast(toError(error).message, "error");
          return null;
        }
      });
      if (state === "running") get().toast(t("正在复制数据，完成后自动切换；请勿关闭启动器。", "Copying data; it switches over when done. Don't close the launcher."));
    },
  };
}
