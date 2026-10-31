// App state and actions. Components subscribe to the slices they draw, so a
// burst of log lines re-renders the log panel and nothing else.

import { useEffect, useState } from "react";
import { create } from "zustand";
import { useShallow } from "zustand/react/shallow";

import { isEnglish, lang, preference, rememberLanguage, resolve } from "./lib/i18n";
import { backend, call, toError } from "./api/backend";
import type {
  Release,
  Account,
  AppInfo,
  ContentKind,
  CrashAnalysis,
  FileOp,
  Instance,
  JavaInstallation,
  LauncherEvent,
  LoaderVersion,
  LocalMod,
  Location,
  LoginStart,
  ModInsight,
  ModProject,
  ModVersion,
  PackLists,
  Settings,
  Snapshot,
  SystemMemory,
  VersionSummary,
} from "./api/types";
import { copyToClipboard } from "./lib/clipboard";
import { filterCategories, type FilterChoice, type SearchSort } from "./lib/contentFilters";
import { t } from "./lib/i18n";

export type Page = "home" | "instances" | "multiplayer" | "accounts" | "settings";

// install_instance starts the task before it returns its id, so a task that
// finishes at once can report before the UI knows the id. Such final events
// wait here until installInstance has recorded the task.
const earlyInstallEnds = new Map<string, LauncherEvent>();

// Background operations (file work, mod and pack installs, Java downloads,
// moving storage) end with one event that names no task, and it can arrive
// before the command that started them returns. Each end bumps this, so the
// starter can tell its operation is already over.
let operationEnds = 0;

// Reads of the manage panel's lists are numbered: a slow answer for an
// earlier instance, or an earlier read, never replaces a newer one.
let modsRead = 0;
let packsRead = 0;

// A login can end (a failed device code request, say) before start_login
// returns its id. Such ends wait here; ended ids are remembered so a late
// return does not mark them pending again. A cancel bumps the attempt.
const earlyLoginEnds = new Map<string, LauncherEvent>();
const endedLogins = new Set<string>();
let loginAttempt = 0;

/** Starts a background operation with the files marked busy from the
    start, so its end event clears them even when it comes before `start`
    returns. "running" when it is still going, "done" when it already ended,
    "failed" when it never started. */
async function beginOperation(start: () => Promise<{ task: string | null } | null>): Promise<"running" | "done" | "failed"> {
  const ends = operationEnds;
  useStore.setState({ fileBusy: true });
  const started = await start();
  if (operationEnds !== ends) return started ? "done" : "failed";
  if (!started) {
    useStore.setState({ fileBusy: false });
    return "failed";
  }
  if (started.task) useStore.setState({ operationTask: started.task });
  return "running";
}
export type ManageTab = "general" | "options" | "mods" | "packs" | "browse" | "data" | "servers" | "shots";
/** A tab's way to save its edits; resolves false when saving failed. */
/** `discard` puts the saved values back; a page that stays on screen after
    its edits were thrown away (quitting was cancelled) needs it. */
export type FormGuard = { save: () => Promise<boolean>; discard?: () => void };

type ModSearch = {
  key: string;
  kind: ContentKind;
  hits: ModProject[];
  total: number;
  offset: number;
  loading: boolean;
  error: string | null;
  /** Narrowing chosen on the download page; reset when the kind changes. */
  filters: FilterChoice;
  sort: SearchSort;
  /** Where results come from; CurseForge needs an API key in settings. */
  source: SearchSource;
};
export type SearchSource = "modrinth" | "curseforge";
const emptySearch = (kind: ContentKind = "mod", source: SearchSource = "modrinth"): ModSearch => ({ key: "", kind, hits: [], total: 0, offset: 0, loading: false, error: null, filters: {}, sort: "relevance", source });

export type ModInsights = { id: string | null; byFile: Record<string, ModInsight>; loading: boolean; error: string | null; checked: boolean };
const noInsights: ModInsights = { id: null, byFile: {}, loading: false, error: null, checked: false };
export type Modal =
  /** `version` preselects a Minecraft version (from the command palette). */
  | { kind: "create"; version?: string }
  | { kind: "manage"; id: string; tab: ManageTab }
  | { kind: "crash"; instanceId: string; code: number | null; analysis: CrashAnalysis }
  | { kind: "recycle" }
  | { kind: "deleteInstance"; instance: Instance }
  | { kind: "addOffline" }
  | { kind: "palette" }
  /** `back` is the dialog to return to when the viewer closes. */
  | { kind: "shot"; id: string; name: string; back?: Modal }
  | { kind: "deleteAccount"; account: Account }
  /** One save of a world: rename, back up, export, datapacks. */
  | { kind: "save"; id: string; folder: string }
  /** A world's mod config files; `query` filters the list at first. */
  | { kind: "configs"; id: string; query?: string }
  | { kind: "import" }
  | { kind: "exportPack"; id: string }
  | { kind: "addYggdrasil" }
  | { kind: "tunnel"; id: string }
  /** Which group a world sits in on the worlds page. */
  | { kind: "group"; id: string }
  | { kind: "skin"; accountId: string }
  | { kind: "quit" }
  | { kind: "modpacks" }
  /** Asks before stopping a running game, or offers to force it once a stop is under way. */
  | { kind: "stopGame"; id: string };

/** The last thing that went wrong, shown on the stage of the world it happened to. */
export type LastError = { instanceId: string | null; message: string; code?: string; detail?: string | null; what: "install" | "launch" | "crash" };

export type ToastKind = "info" | "success" | "error";
export interface Toast {
  id: number;
  message: string;
  kind: ToastKind;
  /** One thing to do about it, e.g. open the log. */
  action?: { label: string; run: () => void };
}

export interface InstallState {
  taskId: string;
  instanceId: string;
  stage: string;
  done: number;
  total: number;
  bytes: number;
  totalBytes: number;
  /** Smoothed bytes per second for the current stage. */
  speed: number | null;
  sample: { at: number; bytes: number } | null;
}

export type FilePurpose =
  | "mods"
  | "restore"
  | "pack"
  | "storage"
  | "java"
  | "java-root"
  | "resourcePacks"
  | "shaders"
  | "datapacks"
  | "saveZip"
  | "saveFolder"
  | "gameDir"
  | "skin";

export type InstanceState = "running" | "starting" | "installing" | "queued" | "ready" | "notInstalled";

const MAX_LOG_LINES = 5000;
const MOTION_KEY = "mc-launcher.reduce-motion";

function readReduceMotion(): boolean {
  try {
    return localStorage.getItem(MOTION_KEY) === "1";
  } catch {
    return false;
  }
}

interface State {
  ready: boolean;
  page: Page;
  settings: Settings;
  accounts: Account[];
  instances: Instance[];
  appInfo: AppInfo;
  systemMemory: SystemMemory | null;

  versions: VersionSummary[];
  latest: { release: string; snapshot: string } | null;
  versionsLoaded: boolean;
  versionsLoading: boolean;
  versionsFromCache: boolean;

  javaRuntimes: JavaInstallation[];
  javaScanning: boolean;

  install: InstallState | null;
  launchAfterInstall: string | null;
  /** The world to start once the sign-in it was waiting on finishes. */
  launchAfterLogin: string | null;
  /** Worlds waiting for the running download; `launch` starts the game after. */
  installQueue: { id: string; launch: boolean }[];
  /** The game started last that is still running (what the home page and
      status bar follow); `runningIds` has every running world. */
  running: string | null;
  runningIds: string[];
  launchPending: string | null;
  /** Worlds the user asked the launcher to stop. */
  stopping: string[];
  /** Wall-clock start of each world's launch, used to bound crash analysis. */
  launchSince: Record<string, number>;
  /** Bumped for every launch attempt; stale crash analyses compare it. */
  launchToken: number;
  /** Result of the last install/repair, used by the crash dialog. */
  installOutcome: { instanceId: string; ok: boolean } | null;
  /** How each world's last game this session ended: true when it closed
      normally or was stopped, false when it crashed. */
  lastExits: Record<string, boolean>;
  lastError: LastError | null;

  logs: string[];
  logsInstance: string | null;
  logsOpen: boolean;

  auth: LoginStart | null;
  authBusy: boolean;
  loginPending: string | null;
  showAzureHelp: boolean;
  /** A Client ID is set, or one was built into this release. */
  loginConfigured: boolean;

  modal: Modal | null;
  /** Edits on a manage tab that aren't saved yet; leaving the tab asks first. */
  formGuard: FormGuard | null;
  /** Where the player tried to go while edits were unsaved; "quit" when
      quitting the launcher asked first. */
  leaving: "close" | "quit" | ManageTab | null;
  /** Unsaved edits on a page (settings); switching pages asks first. */
  pageGuard: FormGuard | null;
  /** The page the player tried to open while those edits were unsaved, or
      "quit" when quitting the launcher asked first. */
  leavingPage: Page | "quit" | null;
  /** The launcher quits by itself once games and work finish. */
  quitScheduled: boolean;
  toasts: Toast[];
  fileBusy: boolean;
  /** Counts finished file operations; lists of files reload when it moves. */
  filesChanged: number;
  /** Whether the last file operation succeeded. */
  lastOperationOk: boolean;
  /** A newer launcher, once a check found one. */
  update: Release | null;
  operationTask: string | null;

  loader: { key: string; versions: LoaderVersion[]; loading: boolean; error: string | null };
  modSearch: ModSearch;
  modVersions: { key: string; project: string | null; versions: ModVersion[]; loading: boolean };
  /** Initial query used when the crash dialog opens the download page. */
  modQuery: string;
  localMods: LocalMod[];
  /** Names, icons and updates for local mods, from their hashes on Modrinth. */
  modInsights: ModInsights;
  localPacks: PackLists | null;
  recycled: Instance[];

  reduceMotion: boolean;
}

interface Actions {
  init(): Promise<void>;
  /** A desktop shortcut asked for this world: make it current and start it. */
  launchFromShortcut(id: string): void;
  /** Looks for a newer launcher; `quiet` stays silent unless one is found. */
  checkUpdate(quiet?: boolean): Promise<void>;
  handleEvent(event: LauncherEvent): void;
  refresh(): Promise<void>;
  applySnapshot(snapshot: Snapshot): void;
  checkLogin(): Promise<void>;
  loadSystemMemory(): void;

  setPage(page: Page): void;
  toast(message: string, kind?: ToastKind, action?: Toast["action"]): void;
  dismissToast(id: number): void;
  copy(text: string, label: string): Promise<void>;
  open(location: Location): Promise<void>;
  openUrl(url: string): Promise<void>;
  setReduceMotion(value: boolean): void;

  run<T>(command: string, args?: Record<string, unknown>): Promise<T | undefined>;
  mutate(command: string, args?: Record<string, unknown>, success?: string): Promise<boolean>;

  loadVersions(force?: boolean): void;
  scanJava(): void;
  scanJavaDirectory(): Promise<void>;
  downloadJava(major: number): Promise<void>;

  primaryAction(id: string): void;
  /** Asks the game to save and quit; `force` ends the process at once. */
  stopInstance(id: string, force: boolean): void;
  /** Starts a download, or queues it behind the running one; true only when it started now. */
  installInstance(id: string, launch?: boolean): Promise<boolean>;
  /** Takes a waiting world off the download queue. */
  unqueueInstall(id: string): void;
  cancelInstall(): void;
  /** With `world`, opens that singleplayer world directly (1.20+); with
      `server`, joins that multiplayer server. */
  launchInstance(id: string, world?: string, server?: string, offline?: boolean): Promise<void>;
  /** The one thing to do about a failed launch's error code, if there is one
      beyond reading the log: sign in again, or play offline. */
  launchFix(id: string, code: string | undefined): Toast["action"] | null;
  setActiveInstance(id: string): void;
  /** Stars a world and puts it in a group (null for none). */
  organize(id: string, favorite: boolean, group: string | null): Promise<boolean>;

  showCrashDialog(instanceId: string, code: number | null, sinceMs: number): Promise<void>;
  installJavaForInstance(id: string, major: number): Promise<boolean>;
  setInstanceMemory(id: string, memoryMb: number): Promise<boolean>;

  startLogin(mode: "browser" | "device"): Promise<void>;
  cancelLogin(): void;
  openAuthUrl(): void;

  /** A settings section to scroll to when the settings page opens. */
  settingsSection: string | null;
  openSettings(section?: string): void;
  openModal(modal: Modal): void;
  closeModal(): void;
  setFormGuard(guard: FormGuard | null): void;
  setPageGuard(guard: FormGuard | null): void;
  /** Answers the unsaved-edits question raised by `setPage`. */
  leavePage(save: boolean): Promise<void>;
  setManageTab(tab: ManageTab): void;
  /** Answers the unsaved-edits question: save first, or throw the edits away. */
  leave(save: boolean): Promise<void>;
  /** Quits again once the edits it asked about are saved or discarded. */
  resumeQuit(): void;
  cancelScheduledQuit(): void;
  /** Buttons pass their click event; only a string is taken as a version. */
  openCreate(version?: unknown): void;
  openManage(id: string, tab?: ManageTab, query?: string, kind?: ContentKind): void;
  openRecycle(): Promise<void>;

  openLogs(id: string): Promise<void>;
  reloadLogs(): Promise<void>;
  closeLogs(): void;
  toggleLogs(): void;

  /** `quiet` skips the "working" toast for operations that finish at once. */
  fileOperation(id: string | null, operation: FileOp, quiet?: boolean): Promise<void>;
  reloadLocalMods(): Promise<void>;
  reloadPacks(): Promise<void>;
  inspectMods(): Promise<void>;
  updateMods(id: string, versions: string[]): Promise<void>;
  refreshLoaderVersions(loader: string, minecraft: string | null): void;
  /** Without `kind`, searches the same kind as the current results. */
  /** `refine` changes the filters or the order; otherwise they are kept. */
  searchMods(id: string, query: string, offset: number, kind?: ContentKind, refine?: { filters?: FilterChoice; sort?: SearchSort; source?: SearchSource }): void;
  /** Switches what the download page shows, without searching. */
  setBrowseKind(kind: ContentKind): void;
  loadModVersions(id: string, project: string): void;
  /** Installs a version of whatever kind is being browsed. */
  /** `save` picks the save a datapack goes into. */
  installMod(id: string, version: string, save?: string): Promise<void>;
  /** OptiFine into the world's mods (with OptiFabric on Fabric and Quilt). */
  installOptifine(id: string): Promise<void>;
  chooseFiles(purpose: FilePurpose, id?: string): Promise<string[]>;
  /** A place to write a new file; null when cancelled. */
  chooseSavePath(title: string, defaultName: string, extension: string): Promise<string | null>;
  relocate(): Promise<void>;
}

const emptySettings: Settings = {
  schemaVersion: 3,
  clientId: "",
  downloadSource: "bmclapi",
  maxConcurrentDownloads: 32,
  memoryMb: 4096,
  memoryAuto: true,
  hideOnLaunch: true,
  closeToTray: true,
  autoChinese: true,
  javaPath: null,
  theme: "system",
  activeAccountId: null,
  activeInstanceId: null,
  launch: { jvmArgs: "", gameArgs: "", windowWidth: null, windowHeight: null, env: "", wrapper: "", preLaunch: "", postExit: "" },
  curseforgeKey: "",
  checkUpdates: true,
  language: "system",
  proxyMode: "system",
  proxyUrl: "",
  autoBackup: true,
  autoBackupKeep: 5,
  dedicatedGpu: true,
};

let toastSeq = 0;
let keySeq = 0;
const newKey = () => `k${++keySeq}-${Date.now()}`;

export const useStore = create<State & Actions>()((set, get) => ({
  ready: false,
  page: "home",
  settings: emptySettings,
  accounts: [],
  instances: [],
  appInfo: { version: "", dataDir: "", platform: "", arch: "" },
  systemMemory: null,

  versions: [],
  latest: null,
  versionsLoaded: false,
  versionsLoading: false,
  versionsFromCache: false,

  javaRuntimes: [],
  javaScanning: false,

  install: null,
  launchAfterInstall: null,
  launchAfterLogin: null,
  installQueue: [],
  running: null,
  runningIds: [],
  launchPending: null,
  stopping: [],
  launchSince: {},
  launchToken: 0,
  installOutcome: null,
  lastExits: {},
  lastError: null,

  logs: [],
  logsInstance: null,
  logsOpen: false,

  auth: null,
  authBusy: false,
  loginPending: null,
  showAzureHelp: false,
  loginConfigured: true,

  modal: null,
  formGuard: null,
  leaving: null,
  pageGuard: null,
  leavingPage: null,
  quitScheduled: false,
  toasts: [],
  fileBusy: false,
  filesChanged: 0,
  lastOperationOk: true,
  update: null,
  operationTask: null,

  loader: { key: "", versions: [], loading: false, error: null },
  modSearch: emptySearch(),
  modVersions: { key: "", project: null, versions: [], loading: false },
  modQuery: "",
  localMods: [],
  modInsights: noInsights,
  localPacks: null,
  settingsSection: null,
  recycled: [],

  reduceMotion: readReduceMotion(),

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
      instances: snapshot.instances,
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

  handleEvent(event) {
    const s = get();
    switch (event.type) {
      case "quitRequested":
        // Unsaved edits are asked about first, where they are; quitting
        // goes on from `resumeQuit` once they are saved or discarded.
        if (s.formGuard) set({ leaving: "quit" });
        else if (s.pageGuard) set({ leavingPage: "quit", modal: null });
        else set({ modal: { kind: "quit" } });
        break;
      case "quitScheduled":
        set({ quitScheduled: event.data.scheduled });
        break;
      case "launchRequest":
        s.launchFromShortcut(event.data.instanceId);
        break;
      case "operationComplete": {
        operationEnds++;
        set({ operationTask: null, fileBusy: false, filesChanged: s.filesChanged + 1, lastOperationOk: "Ok" in event.data });
        void s.refresh().then(() => {
          const { modal } = get();
          if (modal?.kind === "manage") {
            if (!get().instances.some((i) => i.id === modal.id)) set({ modal: null });
            else {
              void get().reloadLocalMods();
              void get().reloadPacks();
              // Files changed, so names and updates may have too.
              if (get().modInsights.checked) void get().inspectMods();
            }
          }
          if (modal?.kind === "recycle") void get().openRecycle();
        });
        if ("Ok" in event.data) s.toast(event.data.Ok, "success");
        else s.toast(event.data.Err.message, "error");
        break;
      }
      case "versions": {
        if ("Ok" in event.data) {
          const list = event.data.Ok;
          set({ versionsLoading: false, versions: list.versions, latest: list.latest, versionsLoaded: true, versionsFromCache: list.fromCache });
        } else {
          set({ versionsLoading: false });
          s.toast(event.data.Err.message, "error");
        }
        break;
      }
      case "javaRuntimes":
        set({ javaRuntimes: event.data, javaScanning: false });
        break;
      case "loaderVersions": {
        if (event.data.key !== s.loader.key) break;
        const r = event.data.result;
        set({ loader: { ...s.loader, loading: false, versions: "Ok" in r ? r.Ok : [], error: "Ok" in r ? null : r.Err.message } });
        break;
      }
      case "modSearch": {
        if (event.data.key !== s.modSearch.key) break;
        const r = event.data.result;
        set({
          modSearch: "Ok" in r
            ? { ...s.modSearch, loading: false, hits: r.Ok.hits, total: r.Ok.total_hits, error: null }
            : { ...s.modSearch, loading: false, hits: [], error: r.Err.message },
        });
        break;
      }
      case "modVersions": {
        if (event.data.key !== s.modVersions.key) break;
        const r = event.data.result;
        set({ modVersions: { ...s.modVersions, loading: false, versions: "Ok" in r ? r.Ok : [] } });
        if ("Err" in r) set({ modSearch: { ...get().modSearch, error: r.Err.message } });
        break;
      }
      case "storageChanged": {
        operationEnds++;
        set({ fileBusy: false });
        if ("Ok" in event.data) {
          void s.refresh();
          set({ javaRuntimes: [] });
          s.scanJava();
          s.toast(t("已切换存储目录。原目录保留为备份。", "Storage folder switched. The old folder is kept as a backup."), "success");
        } else s.toast(event.data.Err.message, "error");
        break;
      }
      case "installProgress": {
        const install = s.install;
        const p = event.data;
        if (!install || install.taskId !== p.taskId) break;
        const now = performance.now();
        let { speed, sample } = install.stage === p.stage ? install : { speed: null, sample: null };
        if (!sample) sample = { at: now, bytes: p.bytes };
        else if (now - sample.at >= 900) {
          const current = Math.max(0, p.bytes - sample.bytes) / ((now - sample.at) / 1000);
          speed = speed == null ? current : speed * 0.6 + current * 0.4;
          sample = { at: now, bytes: p.bytes };
        }
        set({ install: { ...install, stage: p.stage, done: p.done, total: p.total, bytes: p.bytes, totalBytes: p.totalBytes, speed, sample } });
        break;
      }
      case "installComplete": {
        if (s.install?.taskId !== event.data.taskId) {
          if (!s.install) earlyInstallEnds.set(event.data.taskId, event);
          break;
        }
        const launchNext = s.launchAfterInstall === event.data.instanceId;
        set({
          install: null,
          lastError: s.lastError?.instanceId === event.data.instanceId ? null : s.lastError,
          launchAfterInstall: null,
          installOutcome: { instanceId: event.data.instanceId, ok: true },
        });
        void s.refresh();
        if (launchNext) void s.launchInstance(event.data.instanceId);
        else s.toast(t(`${event.data.versionId} 已准备就绪`, `${event.data.versionId} is ready`), "success");
        startQueuedInstall();
        break;
      }
      case "installError": {
        if (s.install?.taskId !== event.data.taskId) {
          if (!s.install) earlyInstallEnds.set(event.data.taskId, event);
          break;
        }
        set({
          install: null,
          launchAfterInstall: null,
          installOutcome: { instanceId: event.data.instanceId, ok: false },
        });
        if (event.data.error.code === "cancelled") s.toast(t("下载已取消", "Download cancelled"));
        else {
          set({ lastError: { instanceId: event.data.instanceId, message: event.data.error.message, code: event.data.error.code, detail: event.data.error.detail, what: "install" } });
          s.toast(event.data.error.message, "error");
        }
        // One world failing doesn't hold up the others in line.
        startQueuedInstall();
        break;
      }
      case "launchStarted": {
        const id = event.data.instanceId;
        set({
          launchPending: s.launchPending === id ? null : s.launchPending,
          running: id,
          runningIds: [...s.runningIds.filter((other) => other !== id), id],
          stopping: s.stopping.filter((other) => other !== id),
          launchSince: { ...s.launchSince, [id]: s.launchSince[id] ?? Date.now() },
        });
        void s.refresh();
        break;
      }
      case "launchLogs": {
        if (s.logsInstance !== event.data.instanceId) break;
        let logs = s.logs.concat(event.data.lines.map((l) => (l.stream === "stderr" ? `[stderr] ${l.line}` : l.line)));
        if (logs.length > MAX_LOG_LINES) logs = logs.slice(logs.length - MAX_LOG_LINES + 1000);
        set({ logs });
        break;
      }
      case "launchExit": {
        const id = event.data.instanceId;
        if (!s.runningIds.includes(id)) break;
        const stopped = s.stopping.includes(id);
        const since = s.launchSince[id] ?? 0;
        const runningIds = s.runningIds.filter((other) => other !== id);
        const { [id]: _ended, ...launchSince } = s.launchSince;
        set({
          running: runningIds[runningIds.length - 1] ?? null,
          runningIds,
          stopping: s.stopping.filter((other) => other !== id),
          launchSince,
          lastExits: { ...s.lastExits, [id]: stopped || event.data.success },
        });
        void s.refresh();
        if (stopped) {
          s.toast(t("游戏已停止", "Game stopped"));
          break;
        }
        if (event.data.success) {
          s.toast(t("游戏已退出", "Game closed"));
          break;
        }
        const message = event.data.code != null ? t(`游戏异常退出（代码 ${event.data.code}）`, `The game quit unexpectedly (code ${event.data.code})`) : t("游戏异常退出", "The game quit unexpectedly");
        set({ lastError: { instanceId: id, message, what: "crash" } });
        void s.showCrashDialog(event.data.instanceId, event.data.code, since);
        break;
      }
      case "launchError": {
        const id = event.data.instanceId;
        if (s.launchPending === id) set({ launchPending: null });
        const { [id]: _failed, ...launchSince } = s.launchSince;
        const { message, code, detail } = event.data.error;
        set({
          lastError: { instanceId: id, message, code, detail, what: "launch" },
          stopping: s.stopping.filter((other) => other !== id),
          launchSince,
        });
        s.toast(message, "error", s.launchFix(id, code) ?? { label: t("查看日志", "Show log"), run: () => void get().openLogs(id) });
        break;
      }
      case "accountsChanged":
        set({ accounts: event.data });
        break;
      case "loginStarted": {
        // A login cancelled while it was starting must not come back.
        if (!s.authBusy && s.loginPending !== event.data.loginId) break;
        set({ authBusy: false, loginPending: event.data.loginId, auth: event.data });
        const url = event.data.authorizeUrl ?? event.data.verificationUri;
        if (url) void s.openUrl(url);
        break;
      }
      case "loginComplete":
        if (s.loginPending === event.data.loginId || s.auth?.loginId === event.data.loginId) {
          endedLogins.add(event.data.loginId);
          set({ auth: null, loginPending: null, authBusy: false, launchAfterLogin: null });
          void s.refresh();
          s.toast(t(`登录成功：${event.data.account.username}`, `Signed in as ${event.data.account.username}`), "success");
          // Signed in again because a game couldn't start: start it now.
          const relaunch = s.launchAfterLogin;
          if (relaunch && s.instances.some((i) => i.id === relaunch) && !s.runningIds.includes(relaunch)) void s.launchInstance(relaunch);
        } else if (s.authBusy && !s.loginPending) {
          earlyLoginEnds.set(event.data.loginId, event);
        } else {
          // Finished as it was being cancelled: the account was stored anyway.
          void s.refresh();
        }
        break;
      case "loginError":
        if (s.loginPending === event.data.loginId || s.auth?.loginId === event.data.loginId) {
          endedLogins.add(event.data.loginId);
          set({ auth: null, loginPending: null, authBusy: false, launchAfterLogin: null });
          if (event.data.error.code !== "cancelled") s.toast(event.data.error.message, "error");
        } else if (s.authBusy && !s.loginPending) {
          earlyLoginEnds.set(event.data.loginId, event);
        }
        break;
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

  loadVersions(force = false) {
    if (get().versionsLoading) return;
    set({ versionsLoading: true });
    void call("load_versions", { force }).catch((e) => {
      set({ versionsLoading: false });
      get().toast(toError(e).message, "error");
    });
  },

  scanJava() {
    if (get().javaScanning) return;
    set({ javaScanning: true });
    void call("load_java_runtimes").catch(() => set({ javaScanning: false }));
  },

  async scanJavaDirectory() {
    const [dir] = await get().chooseFiles("java-root");
    if (!dir) return;
    set({ javaScanning: true });
    await get().run("scan_java_directory", { path: dir });
  },

  async downloadJava(major) {
    const state = await beginOperation(async () => {
      const task = await get().run<string>("download_java", { major });
      return task ? { task } : null;
    });
    if (state === "running") get().toast(t(`正在检测或下载 Java ${major}…`, `Finding or downloading Java ${major}…`));
  },

  primaryAction(id) {
    const s = get();
    const instance = s.instances.find((i) => i.id === id);
    if (!instance) return;
    // Stopping a game is never one click: unsaved progress is at stake.
    if (s.runningIds.includes(id)) return set({ modal: { kind: "stopGame", id } });
    // Several worlds may run at once; one launch is prepared at a time.
    if (s.install?.instanceId === id || s.launchPending) return;
    // Pressed again while waiting, it leaves the queue, the way a running game stops.
    if (s.installQueue.some((queued) => queued.id === id)) {
      s.unqueueInstall(id);
      s.toast(t("已移出下载队列", "Left the download queue"));
      return;
    }
    const bound = instance.accountId && s.accounts.some((a) => a.id === instance.accountId);
    if (!bound && !s.accounts.some((a) => a.id === s.settings.activeAccountId)) {
      s.toast(t("请先添加一个账号", "Add an account first"));
      s.setPage("accounts");
      return;
    }
    if (instance.installed) void s.launchInstance(id);
    else void s.installInstance(id, true);
  },

  stopInstance(id, force) {
    const s = get();
    if (!s.runningIds.includes(id)) return;
    // Remember this before the exit event arrives; otherwise a stop would
    // be mistaken for a crash. Clear it again if a gentle stop fails, so a
    // later real crash is still reported.
    if (!s.stopping.includes(id)) set({ stopping: [...s.stopping, id] });
    void call("stop_instance", { id, force }).then(
      () => {
        if (!force && get().stopping.includes(id)) get().toast(t("正在保存并关闭游戏…", "Saving and closing the game…"));
      },
      (error) => {
        if (!force) set({ stopping: get().stopping.filter((other) => other !== id) });
        get().toast(toError(error).message, "error");
      },
    );
  },

  async installInstance(id, launch = false) {
    const { install, installQueue } = get();
    if (install || installStarting || installQueue.length > 0) {
      if (install?.instanceId === id || installQueue.some((queued) => queued.id === id)) {
        get().toast(t("这个世界已经在下载队列里", "This world is already in the download queue"));
        return false;
      }
      const ahead = installQueue.length + (install ? 1 : 0);
      set({ installQueue: [...installQueue, { id, launch }] });
      get().toast(t(`已加入下载队列，前面还有 ${ahead} 个`, `Queued behind ${ahead} other download${ahead > 1 ? "s" : ""}`));
      return false;
    }
    return beginInstall(id, launch);
  },

  unqueueInstall(id) {
    set({ installQueue: get().installQueue.filter((queued) => queued.id !== id) });
  },

  cancelInstall() {
    const install = get().install;
    if (!install) return;
    set({ launchAfterInstall: null });
    void call("cancel_task", { taskId: install.taskId });
    get().toast(t("正在取消…", "Cancelling…"));
  },

  async launchInstance(id, world, server, offline = false) {
    // Set pending state before the command so a fast launchStarted or
    // launchError event cannot be overwritten by the code after the await.
    const token = get().launchToken + 1;
    const lastError = get().lastError;
    set({
      stopping: get().stopping.filter((other) => other !== id),
      launchSince: { ...get().launchSince, [id]: Date.now() },
      launchToken: token,
      lastError: lastError?.instanceId === id ? null : lastError,
      launchPending: id,
      logs: [],
      logsInstance: id,
    });
    try {
      await call("launch_instance", { id, world: world ?? null, server: server ?? null, offline });
    } catch (error) {
      // Only clear state that belongs to this attempt. If the event already
      // moved the run on, it has handled (and reported) the failure itself.
      if (get().launchToken !== token || get().launchPending !== id) return;
      const { [id]: _failed, ...launchSince } = get().launchSince;
      set({ launchPending: null, launchSince, logs: [], logsInstance: null });
      get().toast(toError(error).message, "error");
    }
  },

  launchFix(id, code) {
    if (code === "auth_offline")
      return {
        label: t("离线进入单人", "Play offline"),
        run: () => {
          get().toast(t("以离线身份开始：单人游戏和存档照常，联机服务器进不去", "Playing offline: singleplayer and saves work, online servers don't"));
          void get().launchInstance(id, undefined, undefined, true);
        },
      };
    if (code !== "auth_expired") return null;
    return {
      label: t("重新登录", "Sign in again"),
      run: () => {
        // The account the world plays as, looked up when pressed.
        const store = get();
        const instance = store.instances.find((i) => i.id === id);
        const bound = instance?.accountId && store.accounts.some((a) => a.id === instance.accountId) ? instance.accountId : store.settings.activeAccountId;
        const account = store.accounts.find((a) => a.id === bound);
        // A Microsoft account signs in right here and the game starts after;
        // others sign in on the Accounts page, with their server's password.
        if (account?.provider !== "microsoft" || !store.loginConfigured) return store.setPage("accounts");
        set({ launchAfterLogin: id });
        void store.startLogin("browser");
      },
    };
  },

  async showCrashDialog(instanceId, code, sinceMs) {
    const token = get().launchToken;
    let analysis: CrashAnalysis = { diagnoses: [], excerpt: "" };
    try {
      analysis = await call<CrashAnalysis>("analyze_crash", { instanceId, sinceMs });
    } catch (error) {
      get().toast(toError(error).message, "error");
    }
    // Do not cover a retry or a different run that started while the
    // analysis was in flight. Every launch attempt bumps launchToken.
    if (get().launchToken !== token) return;
    if (get().runningIds.includes(instanceId) || get().launchPending === instanceId) return;
    set({ modal: { kind: "crash", instanceId, code, analysis } });
  },

  async installJavaForInstance(id, major) {
    const java = await get().run<JavaInstallation>("install_java_for_instance", { id, major });
    if (!java) return false;
    await get().refresh();
    get().toast(t(`已准备 Java ${java.major}，该世界下次启动会使用它`, `Java ${java.major} is ready; this world uses it from the next launch`), "success");
    return true;
  },

  async setInstanceMemory(id, memoryMb) {
    const instance = get().instances.find((item) => item.id === id);
    if (!instance) return false;
    return get().mutate(
      "configure_instance",
      {
        id,
        name: instance.name,
        memoryMb,
        javaPath: instance.javaPath,
        loader: instance.loader,
        loaderVersion: instance.loaderVersion,
      },
      t(`世界内存已调整为 ${memoryMb} MB`, `World memory set to ${memoryMb} MB`),
    );
  },

  setActiveInstance(id) {
    void get().mutate("set_active_instance", { id });
  },

  organize(id, favorite, group) {
    return get().mutate("organize_instance", { id, favorite, group });
  },

  async startLogin(mode) {
    const s = get();
    if (s.auth || s.loginPending || s.authBusy) return;
    set({ authBusy: true });
    const attempt = ++loginAttempt;
    earlyLoginEnds.clear();
    endedLogins.clear();
    try {
      const loginId = await call<string>("start_login", { mode });
      if (attempt !== loginAttempt) {
        // Cancelled while it was starting: stop it now that it has an id.
        void call("cancel_login", { loginId });
        return;
      }
      const early = earlyLoginEnds.get(loginId);
      earlyLoginEnds.clear();
      if (endedLogins.has(loginId)) return;
      set({ loginPending: loginId });
      if (early) get().handleEvent(early);
    } catch (error) {
      if (attempt !== loginAttempt) return;
      set({ authBusy: false });
      const e = toError(error);
      s.toast(e.message, "error");
      if (e.code === "client_id" || !s.settings.clientId) set({ page: "settings", showAzureHelp: true });
    }
  },

  cancelLogin() {
    loginAttempt++;
    const s = get();
    const loginId = s.loginPending ?? s.auth?.loginId;
    if (loginId) void call("cancel_login", { loginId });
    set({ auth: null, loginPending: null, authBusy: false, launchAfterLogin: null });
  },

  openAuthUrl() {
    const auth = get().auth;
    const url = auth?.authorizeUrl ?? auth?.verificationUri;
    if (url) void get().openUrl(url);
    else get().toast(t("当前没有可打开的登录链接", "There's no sign-in link to open"));
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
    const instance = get().instances.find((i) => i.id === id);
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

  async fileOperation(id, operation, quiet) {
    if (get().fileBusy) return;
    const state = await beginOperation(async () => {
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
    const state = await beginOperation(async () => {
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
    void call("search_mods", { id, key, query, offset, kind: nextKind, categories, sort: next.sort, source }).catch((e) =>
      set({ modSearch: { ...get().modSearch, loading: false, error: toError(e).message } }),
    );
  },

  setBrowseKind(kind) {
    set({ modSearch: emptySearch(kind, get().modSearch.source), modVersions: { key: "", project: null, versions: [], loading: false } });
  },

  loadModVersions(id, project) {
    const key = newKey();
    set({ modVersions: { key, project, versions: [], loading: true } });
    void call("load_mod_versions", { id, key, project, kind: get().modSearch.kind }).catch((e) => {
      set({ modVersions: { key, project, versions: [], loading: false } });
      get().toast(toError(e).message, "error");
    });
  },

  async installOptifine(id) {
    const state = await beginOperation(async () => {
      const task = await get().run<string>("install_optifine", { id });
      return task ? { task } : null;
    });
    if (state === "running") get().toast(t("正在下载 OptiFine…", "Downloading OptiFine…"));
  },

  async installMod(id, version, save) {
    const kind = get().modSearch.kind;
    const state = await beginOperation(async () => {
      const task = kind === "mod" ? await get().run<string>("install_mod", { id, version }) : await get().run<string>("install_pack", { id, version, kind, save: save ?? null });
      return task ? { task } : null;
    });
    if (state === "running") get().toast(kind === "mod" ? t("正在解析依赖、下载并校验文件…", "Resolving dependencies, downloading and verifying…") : t("正在下载并校验文件…", "Downloading and verifying…"));
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
      saveFolder: { title: t("选择存档文件夹（里面有 level.dat）", "Choose a save folder (one with level.dat)"), directory: true },
      gameDir: { title: t("选择游戏目录（.minecraft、版本文件夹或其他启动器的实例）", "Choose a game folder (.minecraft, a version folder, or another launcher's instance)"), directory: true },
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
    const state = await beginOperation(async () => {
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

// ----- derived helpers ------------------------------------------------------

type Store = State & Actions;

/** Subscribe to a few fields; re-renders only when one of them changes. */
export function usePick<T extends object>(selector: (s: Store) => T): T {
  return useStore(useShallow(selector));
}

/** True while a download is being handed to the backend, before `install`
    holds it; a second request in that moment waits in the queue. */
let installStarting = false;

async function beginInstall(id: string, launch: boolean): Promise<boolean> {
  const store = useStore.getState();
  installStarting = true;
  try {
    const taskId = await call<string>("install_instance", { id });
    installStarting = false;
    const lastError = useStore.getState().lastError;
    useStore.setState({
      lastError: lastError?.instanceId === id ? null : lastError,
      installOutcome: null,
      launchAfterInstall: launch ? id : null,
      install: { taskId, instanceId: id, stage: "metadata", done: 0, total: 0, bytes: 0, totalBytes: 0, speed: null, sample: null },
    });
    const early = earlyInstallEnds.get(taskId);
    earlyInstallEnds.clear();
    if (early) useStore.getState().handleEvent(early);
    return true;
  } catch (error) {
    installStarting = false;
    store.toast(toError(error).message, "error");
    // A world that can't start downloading doesn't hold up the rest.
    startQueuedInstall();
    return false;
  }
}

/** Starts the next world waiting for a download; worlds removed meanwhile are skipped. */
function startQueuedInstall() {
  const s = useStore.getState();
  if (s.install || installStarting) return;
  const [next, ...rest] = s.installQueue;
  if (!next) return;
  useStore.setState({ installQueue: rest });
  if (!s.instances.some((instance) => instance.id === next.id)) return startQueuedInstall();
  void beginInstall(next.id, next.launch);
}

export function instanceState(instance: Instance, s: Pick<State, "runningIds" | "launchPending" | "install" | "installQueue">): InstanceState {
  if (s.runningIds.includes(instance.id)) return "running";
  if (s.launchPending === instance.id) return "starting";
  if (s.install?.instanceId === instance.id) return "installing";
  if (s.installQueue.some((queued) => queued.id === instance.id)) return "queued";
  return instance.installed ? "ready" : "notInstalled";
}

export function useActiveInstance(): Instance | null {
  return useStore((s) => s.instances.find((i) => i.id === s.settings.activeInstanceId) ?? null);
}

/** Automatic memory for one instance, counting its mods the way a launch
    does; falls back to the machine-wide value until the answer arrives. */
export function useAutoMemory(instance: Instance | null): number | null {
  const fallback = useStore((s) => s.systemMemory?.recommendedMb ?? null);
  // A change in the open instance's mod list can move the recommendation.
  const mods = useStore((s) => s.localMods.length);
  const [mb, setMb] = useState<{ id: string; value: number } | null>(null);
  const id = instance?.id ?? null;
  useEffect(() => {
    if (!id) return;
    let live = true;
    call<SystemMemory>("system_memory", { id })
      .then((memory) => live && setMb({ id, value: memory.recommendedMb }))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [id, mods]);
  return mb && mb.id === id ? mb.value : fallback;
}

export function useActiveAccount(): Account | null {
  return useStore((s) => s.accounts.find((a) => a.id === s.settings.activeAccountId) ?? null);
}

export function overlayOpen(s: Pick<State, "modal" | "logsOpen" | "auth" | "authBusy">): boolean {
  return s.modal !== null || s.logsOpen || s.auth !== null || s.authBusy;
}
