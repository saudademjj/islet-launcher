import type {
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
  Release,
  RemoteProgress,
  ServerModCheck,
  Settings,
  Snapshot,
  SystemMemory,
  VersionSummary,
} from "../api/types";
import type { FilterChoice, SearchSort } from "../lib/contentFilters";

export type Page = "home" | "instances" | "multiplayer" | "server" | "accounts" | "settings";

export type ManageTab = "general" | "options" | "mods" | "packs" | "browse" | "saves" | "servers" | "shots" | "tools";

/** A tab's way to save its edits; resolves false when saving failed. */
/** `discard` puts the saved values back; a page that stays on screen after
    its edits were thrown away (quitting was cancelled) needs it. */
export type FormGuard = { save: () => Promise<boolean>; discard?: () => void };

export type SearchSource = "modrinth" | "curseforge";

export type ModSearch = {
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

export const emptySearch = (kind: ContentKind = "mod", source: SearchSource = "modrinth"): ModSearch => ({
  key: "",
  kind,
  hits: [],
  total: 0,
  offset: 0,
  loading: false,
  error: null,
  filters: {},
  sort: "relevance",
  source,
});

export type ModInsights = { id: string | null; byFile: Record<string, ModInsight>; loading: boolean; error: string | null; checked: boolean };
export const noInsights: ModInsights = { id: null, byFile: {}, loading: false, error: null, checked: false };

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
  | { kind: "exportServer"; id: string }
  /** A world's server on a Linux host: set up, upload, keep running. */
  | { kind: "remote"; id: string }
  /** The server about to be joined runs mods this world lacks. */
  | { kind: "serverMods"; id: string; address: string; check: ServerModCheck; offline: boolean }
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
  | "serverDir"
  | "sshKey"
  | "skin";

export type InstanceState = "running" | "starting" | "installing" | "queued" | "ready" | "notInstalled";

export const MAX_LOG_LINES = 5000;
export const MOTION_KEY = "mc-launcher.reduce-motion";

export function readReduceMotion(): boolean {
  try {
    return localStorage.getItem(MOTION_KEY) === "1";
  } catch {
    return false;
  }
}

export interface State {
  ready: boolean;
  page: Page;
  settings: Settings;
  accounts: Account[];
  /** Worlds to play; server projects are kept apart in `serverProjects`. */
  instances: Instance[];
  serverProjects: Instance[];
  /** The project open on the server page. */
  serverProject: string | null;
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
  /** A server upload under way: its stage and bytes or seconds. */
  remoteProgress: RemoteProgress | null;
  /** Join this server once the missing mods are in. */
  joinAfterFix: { id: string; address: string; offline: boolean } | null;

  loader: { key: string; versions: LoaderVersion[]; loading: boolean; error: string | null };
  modSearch: ModSearch;
  modVersions: { key: string; project: string | null; versions: ModVersion[]; loading: boolean };
  /** Initial query used when the crash dialog opens the download page. */
  modQuery: string;
  localMods: LocalMod[];
  /** Names, icons and updates for local mods, from their hashes on Modrinth. */
  modInsights: ModInsights;
  localPacks: PackLists | null;
  /** A settings section to scroll to when the settings page opens. */
  settingsSection: string | null;
  recycled: Instance[];

  reduceMotion: boolean;
}

export interface Actions {
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
  launchInstance(id: string, world?: string, server?: string, offline?: boolean, skipModCheck?: boolean): Promise<void>;
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

export type Store = State & Actions;
export type SliceSet = (partial: Partial<Store> | ((state: Store) => Partial<Store>)) => void;
export type SliceGet = () => Store;

export const emptySettings: Settings = {
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

export const initialState: State = {
  ready: false,
  page: "home",
  settings: emptySettings,
  accounts: [],
  instances: [],
  serverProjects: [],
  serverProject: null,
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
  remoteProgress: null,
  joinAfterFix: null,

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
};

/** Non-reactive coordination across slices and backend event handlers. */
export const storeRuntime = {
  // install_instance starts the task before it returns its id, so a task that
  // finishes at once can report before the UI knows the id.
  earlyInstallEnds: new Map<string, LauncherEvent>(),
  // Background operations end with one event that names no task, and it can
  // arrive before the command that started them returns.
  operationEnds: 0,
  // A login can end before start_login returns its id; ended ids are remembered
  // so a late return does not mark them pending again.
  earlyLoginEnds: new Map<string, LauncherEvent>(),
  endedLogins: new Set<string>(),
  loginAttempt: 0,
  // True while a download is being handed to the backend, before `install` holds it.
  installStarting: false,
};

export function findInstance(state: { instances: Instance[]; serverProjects: Instance[] }, id: string): Instance | null {
  return state.instances.find((i) => i.id === id) ?? state.serverProjects.find((i) => i.id === id) ?? null;
}

/** Starts a background operation with the files marked busy from the start,
    so its end event clears them even when it comes before `start` returns.
    "running" when it is still going, "done" when it already ended, "failed" when it never started. */
export async function beginOperation(
  set: SliceSet,
  start: () => Promise<{ task: string | null } | null>,
): Promise<"running" | "done" | "failed"> {
  const ends = storeRuntime.operationEnds;
  set({ fileBusy: true });
  const started = await start();
  if (storeRuntime.operationEnds !== ends) return started ? "done" : "failed";
  if (!started) {
    set({ fileBusy: false });
    return "failed";
  }
  if (started.task) set({ operationTask: started.task });
  return "running";
}
