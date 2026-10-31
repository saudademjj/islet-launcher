// Shapes of the data the Rust side serializes (serde camelCase unless noted).

export type Theme = "system" | "dark" | "light";
export type DownloadSource = "official" | "bmclapi";

export interface Settings {
  schemaVersion: number;
  clientId: string;
  downloadSource: DownloadSource;
  maxConcurrentDownloads: number;
  memoryMb: number;
  memoryAuto: boolean;
  hideOnLaunch: boolean;
  closeToTray?: boolean;
  /** Set the game language to Simplified Chinese before a first launch. */
  autoChinese: boolean;
  javaPath: string | null;
  theme: Theme;
  activeAccountId: string | null;
  activeInstanceId: string | null;
  /** Launch options for every world. */
  launch: LaunchConfig;
  /** CurseForge API key; empty leaves CurseForge off. */
  curseforgeKey: string;
  checkUpdates: boolean;
  systemNotifications?: boolean;
  discordApplicationId?: string;
  /** "system", "zh" or "en". */
  language: "system" | "zh" | "en";
  proxyMode: "system" | "none" | "custom";
  proxyUrl: string;
  /** Back up each save the game changed, after it exits. */
  autoBackup: boolean;
  /** Automatic backups kept per save. */
  autoBackupKeep: number;
  /** Run the game on a laptop's discrete GPU (Windows and Linux). */
  dedicatedGpu: boolean;
}

export type AccountProvider = "microsoft" | "offline" | "yggdrasil";

export interface Account {
  id: string;
  provider: AccountProvider;
  username: string;
  uuid: string;
  xuid: string | null;
  ownership: boolean | null;
  skinUrl: string | null;
  /** The skin has slim (3-pixel) arms. */
  skinSlim?: boolean;
  createdAt: number;
  /** Third-party accounts: the server's API root and display name. */
  server?: string | null;
  serverName?: string | null;
}

/** A newer launcher release. */
export interface Release {
  version: string;
  notes: string;
  url: string;
  /** This computer's package, when its SHA-256 is published. */
  package: { url: string; sha256: string } | null;
}

/** A cape a Microsoft account owns. */
export interface Cape {
  id: string;
  alias: string;
  url: string;
  state: string;
}

export interface Instance {
  id: string;
  name: string;
  versionId: string;
  versionType: string;
  createdAt: number;
  lastLaunchedAt: number | null;
  installed: boolean;
  loader: string;
  loaderVersion: string | null;
  memoryMb: number | null;
  /** null inherits the global setting, "auto" follows the game version. */
  javaPath: string | null;
  /** Seconds spent in game across all launches. */
  playSeconds: number;
  /** This world's own launch options; empty fields use the global ones. */
  launch: LaunchConfig;
  /** Plays as this account instead of the one in use. */
  accountId: string | null;
  /** Starred on the worlds page. */
  favorite: boolean;
  /** The group it sits in on the worlds page. */
  group: string | null;
}

/** Options for starting the game, kept globally and per world. */
export interface LaunchConfig {
  jvmArgs: string;
  gameArgs: string;
  windowWidth: number | null;
  windowHeight: number | null;
  /** KEY=VALUE per line. */
  env: string;
  wrapper: string;
  preLaunch: string;
  /** Run after the game exits; its exit code is in MC_EXIT_CODE. */
  postExit: string;
}

export interface WorldList {
  worlds: WorldInfo[];
  /** The installed version can open a world directly (Minecraft 1.20+). */
  quickPlay: boolean;
}

export interface WorldInfo {
  folder: string;
  /** The name shown in the game (level.dat's LevelName). */
  name: string | null;
  bytes: number;
  modifiedMs: number | null;
  /** The world's icon.png as a data URL, when small enough. */
  icon: string | null;
}

/** One entry of the game's multiplayer list (servers.dat). */
export interface ServerEntry {
  /** Position in servers.dat; edits name it together with the address. */
  index: number;
  name: string;
  address: string;
  /** The icon the game cached from the server, as a data URL. */
  icon: string | null;
}

export type ServerEdit =
  | { kind: "add"; name: string; address: string }
  | { kind: "update"; index: number; expected: string; name: string; address: string }
  | { kind: "remove"; index: number; expected: string }
  | { kind: "raise"; index: number; expected: string };

/** A run of message-of-the-day text in one style; colors are #rrggbb. */
export interface MotdSpan {
  text: string;
  color: string | null;
  bold: boolean;
  italic: boolean;
  underlined: boolean;
  strikethrough: boolean;
  obfuscated: boolean;
}

export interface ServerStatus {
  latencyMs: number;
  version: string;
  protocol: number;
  online: number;
  max: number;
  /** Some of the players online, when the server shares them. */
  players: string[];
  motd: MotdSpan[];
  favicon: string | null;
}

export interface Screenshot {
  name: string;
  bytes: number;
  modifiedMs: number;
  /** Demo backend only: an inline picture instead of the mcshot scheme. */
  demoUrl?: string;
}

/** The game's own settings (options.txt); null leaves the game default. */
export interface GameOptions {
  exists?: boolean;
  lang: string | null;
  renderDistance: number | null;
  maxFps: number | null;
  guiScale: number | null;
  fov: number | null;
  fullscreen: boolean | null;
  vsync: boolean | null;
  volume: number | null;
}

export interface SystemMemory {
  totalMb: number;
  recommendedMb: number;
}

export interface AppInfo {
  version: string;
  dataDir: string;
  platform: string;
  arch: string;
}

export interface Snapshot {
  settings: Settings;
  accounts: Account[];
  instances: Instance[];
  appInfo: AppInfo;
}

export interface VersionSummary {
  id: string;
  versionType: string;
  url: string;
  time: string;
  releaseTime: string;
  sha1: string;
  complianceLevel: number | null;
}

export interface VersionList {
  latest: { release: string; snapshot: string };
  versions: VersionSummary[];
  fromCache: boolean;
}

export interface JavaInstallation {
  path: string;
  version: string;
  major: number;
  arch: string;
  vendor: string;
}

export interface LoaderVersion {
  version: string;
  stable: boolean;
}

/** What a Modrinth project installs as: a mod, a resource pack or a shader pack. */
/** `world` is a map: a zipped save, only on CurseForge. */
export type ContentKind = "mod" | "resourcepack" | "shader" | "datapack" | "world";

// Modrinth types keep the API's snake_case field names.
export interface ProjectDetails {
  body: string;
  format: "markdown" | "html";
  gallery: { url: string; title: string | null }[];
  /** [kind, url]: page, source, wiki, issues or discord. */
  links: [string, string][];
  license: string | null;
  updated: string | null;
}

export interface ModProject {
  project_id: string;
  /** Modrinth search hits include the slug; older backends may omit it. */
  slug?: string;
  title: string;
  description: string;
  downloads: number;
  icon_url?: string | null;
  /** Loaders and tags, e.g. iris / optifine on a shader pack. */
  categories?: string[];
}

export interface SearchResults {
  hits: ModProject[];
  total_hits: number;
}

/** The modpack a world came from. `source` is set when it was installed
 *  from Modrinth or CurseForge, so it can be updated online. */
export interface PackInfo {
  name: string;
  version: string;
  files: number;
  source: { project: string; version: string } | null;
}

export interface ModVersion {
  id: string;
  project_id: string;
  name: string;
  version_number: string;
  game_versions: string[];
  loaders: string[];
  files: { filename: string; primary: boolean; size: number }[];
  dependencies: { project_id: string | null; dependency_type: string }[];
  /** "release", "beta" or "alpha". */
  version_type?: string;
  date_published?: string;
}

export interface LocalMod {
  name: string;
  enabled: boolean;
  bytes: number;
}

/** What a mod's jar says about itself. */
export interface ModMeta {
  id: string | null;
  name: string | null;
  description: string | null;
  version: string | null;
  /** Required mod ids; the game, Java and the loader left out. */
  depends: string[];
  /** Other ids it answers to, bundled jars included. */
  provides: string[];
}

/** What Modrinth knows about one local mod file, matched by its hash. */
export interface ModInsight {
  file: string;
  projectId: string;
  slug: string | null;
  title: string;
  iconUrl: string | null;
  versionId: string;
  versionNumber: string;
  update: { versionId: string; versionNumber: string } | null;
}

/** A resource pack or shader pack: a .zip, or an unpacked folder. */
/** What Modrinth knows about a local zipped resource pack or shader pack. */
export interface PackInsight {
  kind: Exclude<ContentKind, "mod">;
  file: string;
  projectId: string;
  title: string;
  iconUrl: string | null;
}

export interface PackFile {
  name: string;
  bytes: number;
  folder: boolean;
  /** The pack's own pack.png as a data URL. */
  icon: string | null;
  /** Position in the game's enabled list, 1 on top (it wins); null when off. */
  active: number | null;
}

export interface PackLists {
  resourcepacks: PackFile[];
  shaderpacks: PackFile[];
}

export type CrashFix =
  | { kind: "useJava"; major: number }
  | { kind: "raiseMemory"; mb: number }
  | { kind: "installMod"; query: string }
  | { kind: "openMods" }
  | { kind: "repair" }
  | { kind: "none" };

export interface Diagnosis {
  kind: string;
  title: string;
  detail: string;
  fix: CrashFix | null;
}

export interface CrashAnalysis {
  diagnoses: Diagnosis[];
  excerpt: string;
}

export interface ErrorPayload {
  code: string;
  message: string;
  detail: string | null;
}

export type Res<T> = { Ok: T } | { Err: ErrorPayload };

export interface InstallProgress {
  taskId: string;
  instanceId: string;
  stage: string;
  done: number;
  total: number;
  bytes: number;
  totalBytes: number;
  message: string;
}

export interface LoginStart {
  loginId: string;
  mode: string;
  authorizeUrl: string | null;
  userCode: string | null;
  verificationUri: string | null;
  expiresIn: number | null;
}

export type LauncherEvent =
  | { type: "operationComplete"; data: Res<string> }
  | { type: "launchRequest"; data: { instanceId: string } }
  | { type: "quitRequested" }
  | { type: "quitScheduled"; data: { scheduled: boolean } }
  | { type: "versions"; data: Res<VersionList> }
  | { type: "javaRuntimes"; data: JavaInstallation[] }
  | { type: "loaderVersions"; data: { key: string; result: Res<LoaderVersion[]> } }
  | { type: "modSearch"; data: { key: string; result: Res<SearchResults> } }
  | { type: "modVersions"; data: { key: string; result: Res<ModVersion[]> } }
  | { type: "storageChanged"; data: Res<string> }
  | { type: "installProgress"; data: InstallProgress }
  | { type: "installComplete"; data: { taskId: string; instanceId: string; versionId: string } }
  | { type: "installError"; data: { taskId: string; instanceId: string; error: ErrorPayload } }
  | { type: "launchStarted"; data: { instanceId: string; pid: number } }
  | { type: "launchLogs"; data: { instanceId: string; lines: { line: string; stream: string }[] } }
  | { type: "launchExit"; data: { instanceId: string; code: number | null; success: boolean } }
  | { type: "launchError"; data: { instanceId: string; error: ErrorPayload } }
  | { type: "accountsChanged"; data: Account[] }
  | { type: "loginStarted"; data: LoginStart }
  | { type: "loginComplete"; data: { loginId: string; account: Account } }
  | { type: "loginError"; data: { loginId: string; error: ErrorPayload } };

export type FileOp =
  | { kind: "duplicate" }
  | { kind: "backup" }
  | { kind: "restoreBackup"; path: string }
  | { kind: "restoreDeleted" }
  | { kind: "addMods"; paths: string[] }
  | { kind: "toggleMod"; name: string }
  | { kind: "setMods"; names: string[]; enabled: boolean }
  | { kind: "recycleMods"; names: string[] }
  | { kind: "importPack"; path: string }
  | { kind: "installModpack"; version: string }
  | { kind: "recycle"; item: ItemKind; name: string; save?: string }
  | { kind: "addPacks"; item: "resourcePack" | "shader"; paths: string[] }
  | { kind: "addDatapacks"; save: string; paths: string[] }
  | { kind: "renameSave"; folder: string; name: string }
  | { kind: "backupSave"; folder: string }
  | { kind: "restoreSave"; folder: string; backup: string }
  | { kind: "importSave"; path: string }
  | { kind: "exportSave"; folder: string; destination: string }
  | { kind: "purgeRecycle"; only?: string }
  | { kind: "clean"; target: Cleanup }
  | { kind: "importGame"; source: string }
  | { kind: "updatePack"; path: string }
  | { kind: "updateModpackOnline"; version: string }
  | { kind: "exportPack"; destination: string; options: ExportOptions }
  | { kind: "exportServer"; destination: string; options: { acceptEula: boolean; memoryMb: number } }
  | { kind: "changeVersion"; versionId: string; versionType: string }
  | { kind: "undoModUpdate" };

/** The latest mod install or update that can still be undone. */
export interface LastModChange {
  createdMs: number;
  added: number;
  replaced: number;
}

export interface ExportOptions {
  name: string;
  version: string;
  summary: string;
  /** CurseForge .zip or Modrinth .mrpack. */
  format: "mrpack" | "curseforge";
  options: boolean;
  servers: boolean;
  saves: boolean;
}

/** A game found in another launcher's folder. */
export interface ImportCandidate {
  source: string;
  name: string;
  versionId: string;
  versionType: string;
  loader: string;
  loaderVersion: string | null;
  gameDir: string;
  kind: "official" | "isolated" | "prism";
  saves: number;
  mods: number;
}

/** Something inside a world that can be moved to the recycle bin. */
export type ItemKind = "mod" | "resourcePack" | "shader" | "save" | "datapack";

export type Cleanup = { kind: "thumbnails" } | { kind: "unusedVersions" } | { kind: "java"; path: string } | { kind: "oldLogs" };

export interface Datapack {
  name: string;
  bytes: number;
  folder: boolean;
  description: string | null;
}

/** A mod config file, relative to the game folder (`config/…`). */
export interface ConfigFile {
  path: string;
  bytes: number;
  modifiedMs: number;
}

export interface SaveBackup {
  path: string;
  bytes: number;
  createdMs: number;
  /** Made after a game session; only these rotate out. */
  auto: boolean;
}

export interface JavaUsage {
  path: string;
  major: number;
  bytes: number;
  unused: boolean;
}

export interface StorageUsage {
  root: string;
  worlds: number;
  libraries: number;
  assets: number;
  versions: number;
  unusedVersions: string[];
  unusedVersionsBytes: number;
  java: JavaUsage[];
  backups: number;
  recycle: number;
  thumbnails: number;
  logs: number;
}

export type Location =
  | { kind: "dataDir" }
  | { kind: "recycleDir" }
  | { kind: "gameDir"; id: string }
  | { kind: "logsDir"; id: string }
  | { kind: "backupsDir"; id: string }
  | { kind: "world"; id: string; folder: string }
  | { kind: "contentDir"; id: string; content: ContentKind }
  | { kind: "screenshotsDir"; id: string }
  | { kind: "screenshot"; id: string; name: string };

// ----- P2P rooms -------------------------------------------------------------

export type P2pLevel = "idle" | "good" | "fair" | "starting" | "poor" | "down";
export type P2pRole = "host" | "guest";

export interface P2pCheck {
  id: string;
  level: P2pLevel;
  label: string;
  detail: string;
}

export interface P2pPlayer {
  name: string;
  ip: string;
  /** The room's host, as a guest sees it. */
  host: boolean;
  direct: boolean;
  latencyMs: number | null;
  level: P2pLevel;
  detail: string;
}

export interface P2pReport {
  level: P2pLevel;
  summary: string;
  checks: P2pCheck[];
  players: P2pPlayer[];
  ip: string | null;
}

export interface P2pWorld {
  motd: string;
  online: number;
  max: number;
  version: string;
  latencyMs: number;
}

export interface P2pStatus {
  active: boolean;
  preparing: boolean;
  download: { done: number; total: number } | null;
  role: P2pRole | null;
  code: string | null;
  hostname: string;
  gamePort: number;
  /** Guest: where the game joins on this computer. */
  address: string | null;
  uptimeSeconds: number;
  health: P2pReport | null;
  world: P2pWorld | null;
  /** Everyone in the room, by the launcher they use. */
  members: P2pMember[];
  /** Guest: what holds the link to the host back, while something does. */
  notice: string | null;
}

export interface P2pMember {
  name: string;
  /** The launcher they play with, e.g. "HMCL 3.6". */
  vendor: string;
  host: boolean;
  /** This computer. */
  me: boolean;
}

export interface P2pDefaults {
  nodes: string[];
  downloadSize: number | null;
}

export interface LanGame {
  motd: string;
  port: number;
}
