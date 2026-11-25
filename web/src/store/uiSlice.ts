import { backend, call, toError } from "../api/backend";
import type { Instance, Release, Snapshot, SystemMemory } from "../api/types";
import { copyToClipboard } from "../lib/clipboard";
import { isEnglish, lang, preference, rememberLanguage, resolve, t } from "../lib/i18n";
import { emptySearch, findInstance, MOTION_KEY, noInsights, type Actions, type SliceGet, type SliceSet } from "./types";

let toastSeq = 0;

type UiActionKeys =
  | "init"
  | "launchFromShortcut"
  | "checkUpdate"
  | "loadSystemMemory"
  | "checkLogin"
  | "applySnapshot"
  | "refresh"
  | "setPage"
  | "toast"
  | "dismissToast"
  | "copy"
  | "open"
  | "openUrl"
  | "setReduceMotion"
  | "run"
  | "mutate"
  | "openModal"
  | "openSettings"
  | "closeModal"
  | "setFormGuard"
  | "setPageGuard"
  | "leavePage"
  | "setManageTab"
  | "leave"
  | "resumeQuit"
  | "cancelScheduledQuit"
  | "openCreate"
  | "openManage"
  | "openRecycle"
  | "openLogs"
  | "reloadLogs"
  | "closeLogs"
  | "toggleLogs";

export function createUiSlice(set: SliceSet, get: SliceGet): Pick<Actions, UiActionKeys> {
  return {
    async init() {
      const [, snapshot] = await Promise.all([
        backend().subscribe((event) => get().handleEvent(event)),
        call<Snapshot>("bootstrap"),
      ]);
      // The saved language wins; the page started with the cached one, so
      // reload once if they differ (a reset WebView, or a first run).
      if (snapshot.settings.language !== preference) {
        rememberLanguage(snapshot.settings.language);
        if (resolve(snapshot.settings.language) !== lang && !sessionStorage.getItem("mc-launcher.language-reload")) {
          sessionStorage.setItem("mc-launcher.language-reload", "1");
          window.location.reload();
          return;
        }
      }
      void call("set_language", { english: isEnglish }).catch(() => {});
      // A reload drops any edits the backend was told about.
      void call("set_unsaved_changes", { unsaved: false }).catch(() => {});
      void call<boolean>("quit_scheduled").then((quitScheduled) => set({ quitScheduled })).catch(() => {});
      if (isEnglish) backend().setWindowTitle("Islet");
      get().applySnapshot(snapshot);
      set({ ready: true });
      void get().checkLogin();
      get().loadSystemMemory();
      if (snapshot.settings.checkUpdates) void get().checkUpdate(true);
      // Started from a world's desktop shortcut.
      const shortcut = await call<string | null>("take_startup_launch").catch(() => null);
      if (shortcut) get().launchFromShortcut(shortcut);
    },

    launchFromShortcut(id) {
      const s = get();
      if (!s.instances.some((i) => i.id === id)) return s.toast(t("快捷方式对应的世界已经不在了", "The world for this shortcut no longer exists"), "error");
      s.setActiveInstance(id);
      s.setPage("home");
      if (!s.runningIds.includes(id)) s.primaryAction(id);
    },

    async checkUpdate(quiet) {
      try {
        const release = await call<Release | null>("check_update");
        set({ update: release });
        if (release) get().toast(t(`有新版本 ${release.version}，在状态栏右下角查看`, `Version ${release.version} is available; see the bottom right of the status bar`), "info");
        else if (!quiet) get().toast(t("已经是最新版本", "You're on the latest version"), "success");
      } catch (error) {
        if (!quiet) get().toast(toError(error).message, "error");
      }
    },

    loadSystemMemory() {
      if (get().systemMemory) return;
      void call<SystemMemory>("system_memory")
        .then((memory) => set({ systemMemory: memory }))
        .catch(() => {});
    },

    async checkLogin() {
      const ok = await call<boolean>("login_configured").catch(() => true);
      set({ loginConfigured: ok });
    },

    applySnapshot(snapshot) {
      set({
        settings: snapshot.settings,
        accounts: snapshot.accounts,
        instances: snapshot.instances.filter((i) => !i.server),
        serverProjects: snapshot.instances.filter((i) => i.server),
        appInfo: snapshot.appInfo,
      });
    },

    async refresh() {
      try {
        get().applySnapshot(await call<Snapshot>("bootstrap"));
      } catch (error) {
        get().toast(toError(error).message, "error");
      }
    },

    setPage(page) {
      const { pageGuard, page: current } = get();
      if (pageGuard && page !== current) return set({ leavingPage: page });
      if (page === "settings") {
        if (get().javaRuntimes.length === 0) get().scanJava();
        get().loadSystemMemory();
      }
      set({ page });
    },

    toast(message, kind = "info", action) {
      const id = ++toastSeq;
      // Each toast times itself out (see Toasts), holding while pointed at.
      set({ toasts: [...get().toasts.slice(-2), { id, message, kind, action }] });
    },

    dismissToast(id) {
      set({ toasts: get().toasts.filter((toast) => toast.id !== id) });
    },

    async copy(text, label) {
      if (!text) return get().toast(t("没有可复制的内容", "Nothing to copy"));
      const ok = await copyToClipboard(text);
      get().toast(ok ? t(`${label}已复制`, `${label} copied`) : t("复制失败", "Couldn't copy"), ok ? "success" : "error");
    },

    async open(location) {
      await get().run("open_location", { location });
    },

    async openUrl(url) {
      await get().run("open_url", { url });
    },

    setReduceMotion(value) {
      try {
        localStorage.setItem(MOTION_KEY, value ? "1" : "0");
      } catch {
        // per-viewer preference only
      }
      set({ reduceMotion: value });
    },

    async run<T>(command: string, args?: Record<string, unknown>) {
      try {
        return await call<T>(command, args);
      } catch (error) {
        get().toast(toError(error).message, "error");
        return undefined;
      }
    },

    async mutate(command, args, success) {
      const snapshot = await get().run<Snapshot>(command, args);
      if (!snapshot) return false;
      get().applySnapshot(snapshot);
      if (success) get().toast(success, "success");
      return true;
    },

    openModal(modal) {
      set({ modal });
    },

    openSettings(section) {
      set({ page: "settings", settingsSection: section ?? null });
    },

    closeModal() {
      const { modal, formGuard, leaving } = get();
      // A second Esc while the question shows means "keep editing".
      if (leaving) return set({ leaving: null });
      if (modal?.kind === "manage" && formGuard) return set({ leaving: "close" });
      set({ modal: modal?.kind === "shot" && modal.back ? modal.back : null, formGuard: null });
    },

    setFormGuard(formGuard) {
      set({ formGuard, ...(formGuard ? {} : { leaving: null }) });
    },

    setPageGuard(pageGuard) {
      set({ pageGuard, ...(pageGuard ? {} : { leavingPage: null }) });
    },

    async leavePage(save) {
      const { leavingPage, pageGuard } = get();
      if (!leavingPage) return;
      // A failed save has already said why; the edits stay on screen.
      if (save && pageGuard && !(await pageGuard.save())) return set({ leavingPage: null });
      // The page stays up while quitting asks about running work, so the
      // discarded edits must leave the form too.
      if (!save && leavingPage === "quit") pageGuard?.discard?.();
      set({ pageGuard: null, leavingPage: null });
      if (leavingPage === "quit") get().resumeQuit();
      else get().setPage(leavingPage);
    },

    setManageTab(tab) {
      const { modal, formGuard } = get();
      if (modal?.kind !== "manage" || modal.tab === tab) return;
      if (formGuard) return set({ leaving: tab });
      set({ modal: { ...modal, tab } });
    },

    async leave(save) {
      const { leaving, formGuard } = get();
      if (!leaving) return;
      // A failed save has already said why; the edits stay on screen.
      if (save && formGuard && !(await formGuard.save())) return set({ leaving: null });
      set({ formGuard: null, leaving: null });
      const modal = get().modal;
      if (leaving === "quit") get().resumeQuit();
      else if (leaving === "close") get().closeModal();
      else if (modal?.kind === "manage") set({ modal: { ...modal, tab: leaving } });
    },

    resumeQuit() {
      const { formGuard, pageGuard } = get();
      void call("request_quit", { unsaved: Boolean(formGuard || pageGuard) }).catch(() => {});
    },

    cancelScheduledQuit() {
      set({ quitScheduled: false });
      void call("cancel_scheduled_quit").catch(() => {});
    },

    openCreate(version) {
      const s = get();
      if (!s.versionsLoaded) s.loadVersions(false);
      set({ modal: { kind: "create", version: typeof version === "string" ? version : undefined }, loader: { key: "", versions: [], loading: false, error: null } });
    },

    openManage(id, tab = "general", query = "", kind) {
      const instance = findInstance(get(), id);
      // Vanilla cannot load mods, so its downloads start at resource packs.
      const browseKind = kind ?? (instance?.loader === "vanilla" ? "resourcepack" : "mod");
      set({
        modal: { kind: "manage", id, tab },
        formGuard: null,
        leaving: null,
        localMods: [],
        localPacks: null,
        modInsights: noInsights,
        modQuery: query,
        modSearch: emptySearch(browseKind),
        modVersions: { key: "", project: null, versions: [], loading: false },
      });
      void get().reloadLocalMods();
      void get().reloadPacks();
      if (get().javaRuntimes.length === 0) get().scanJava();
      if (query.trim()) get().searchMods(id, query.trim(), 0, browseKind);
    },

    async openRecycle() {
      const list = await get().run<Instance[]>("recycled_instances");
      set({ recycled: list ?? [], modal: { kind: "recycle" } });
    },

    async openLogs(id) {
      const s = get();
      const live = s.runningIds.includes(id) || s.launchPending === id;
      if (!live || s.logsInstance !== id) {
        const lines = await s.run<string[]>("read_log", { id });
        if (!lines) return;
        set({ logs: lines });
      }
      set({ logsInstance: id, logsOpen: true });
    },

    async reloadLogs() {
      const id = get().logsInstance;
      if (!id) return;
      const lines = await get().run<string[]>("read_log", { id });
      if (lines) set({ logs: lines });
    },

    closeLogs() {
      set({ logsOpen: false });
    },

    toggleLogs() {
      const s = get();
      if (s.logsOpen) return s.closeLogs();
      if (s.modal || s.auth || s.authBusy) return;
      const target = s.running ?? s.settings.activeInstanceId;
      if (target) void s.openLogs(target);
    },
  };
}
