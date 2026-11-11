// Demo backend for reviewing the UI in a plain browser (`npm run dev`).
// It keeps state in memory and plays back plausible events; nothing here
// ships in the Tauri window.

import type { Backend } from "./backend";
import type {
  Account,
  GameOptions,
  Instance,
  JavaInstallation,
  LauncherEvent,
  LocalMod,
  PackFile,
  Screenshot,
  ServerEdit,
  ServerEntry,
  ServerStatus,
  Settings,
  Snapshot,
  VersionSummary,
} from "./types";

const now = Date.now();
const HOUR = 3_600_000;

const settings: Settings = {
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
  activeAccountId: "offline:steve",
  activeInstanceId: "i-survival",
  launch: { jvmArgs: "", gameArgs: "", windowWidth: null, windowHeight: null, env: "", wrapper: "", preLaunch: "", postExit: "" },
  curseforgeKey: "",
  checkUpdates: true,
  // The demo keeps the language across reloads, as the real settings file does.
  language: ((): Settings["language"] => {
    const saved = typeof localStorage === "undefined" ? null : localStorage.getItem("mc-launcher.language");
    return saved === "zh" || saved === "en" ? saved : "system";
  })(),
  proxyMode: "system",
  proxyUrl: "",
  autoBackup: true,
  autoBackupKeep: 5,
  dedicatedGpu: true,
};

let accounts: Account[] = [
  account("offline:steve", "offline", "Steve", "8667ba71-b85a-4004-af54-457a9734eed7"),
  account("ms:alex", "microsoft", "Alex_Builds", "ec561538-f3fd-461d-aff5-086b22154bce"),
];

const projectDefaults = { motd: "", maxPlayers: 20, difficulty: 2, gamemode: 0, pvp: true, whitelist: false, onlineMode: true, port: 25565, memoryMb: 0, seed: "" };

let instances: Instance[] = [
  instance("i-survival", "生存 1.21", "1.21.4", "fabric", true, now - 2 * HOUR),
  { ...instance("i-skyblock", "空岛整合包", "1.20.1", "fabric", false, null), group: "整合包" },
  instance("i-vanilla", "原版 26.3", "26.3", "vanilla", true, now - 50 * HOUR),
  { ...instance("i-create", "机械动力", "1.20.1", "forge", true, now - 9 * 24 * HOUR), favorite: true, group: "整合包" },
  { ...instance("s-paper", "朋友们的生存服", "1.21.4", "paper", false, null), server: { ...projectDefaults, motd: "欢迎来玩" } },
  { ...instance("s-fabric", "模组服", "1.21.1", "fabric", false, null), loaderVersion: "0.16.10", server: { ...projectDefaults } },
];

const plugins: LocalMod[] = [
  { name: "EssentialsX-2.21.0.jar", enabled: true, bytes: 1_402_118 },
  { name: "LuckPerms-Bukkit-5.4.152.jar", enabled: true, bytes: 2_004_331 },
  { name: "worldedit-bukkit-7.3.10.jar", enabled: true, bytes: 3_120_500 },
  { name: "CoreProtect-23.1.jar", enabled: false, bytes: 880_210 },
];

const recycled: Instance[] = [instance("i-old", "旧存档测试", "1.19.2", "vanilla", true, now - 40 * 24 * HOUR)];

/** A tiny 4×4 pixel icon as a data URL, standing in for Modrinth's icons. */
function demoIcon(a: string, b: string): string {
  const cells = "1101011100110110".split("").map((c, i) => `<rect x="${i % 4}" y="${Math.floor(i / 4)}" width="1" height="1" fill="${c === "1" ? a : b}"/>`).join("");
  return `data:image/svg+xml;utf8,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 4 4" shape-rendering="crispEdges">${cells}</svg>`)}`;
}

const DEMO_CATALOG: Record<"mod" | "resourcepack" | "shader", { slug: string; title: string; description: string; downloads: number; icon?: [string, string]; categories?: string[] }[]> = {
  mod: [
    { slug: "sodium", title: "Sodium", description: "现代化的渲染引擎，大幅提升帧率并修复光照问题。", downloads: 98_000_000, icon: ["#7fb238", "#3f7d31"] },
    { slug: "lithium", title: "Lithium", description: "无需修改玩法即可优化游戏逻辑、物理和区块加载。", downloads: 54_000_000, icon: ["#e3c47f", "#8a6a3a"] },
    { slug: "iris", title: "Iris Shaders", description: "兼容 OptiFine 光影包，与 Sodium 一起使用。", downloads: 61_000_000, icon: ["#9b6bd6", "#3a2a5a"] },
    { slug: "modmenu", title: "Mod Menu", description: "在游戏内查看已安装的模组并修改配置。", downloads: 72_000_000 },
    { slug: "jei", title: "Just Enough Items", description: "物品与合成配方查询。", downloads: 310_000_000, icon: ["#c0473a", "#f1e7d6"] },
    { slug: "xaeros-minimap", title: "Xaero's Minimap", description: "保持原版风格的小地图。", downloads: 88_000_000 },
    { slug: "appleskin", title: "AppleSkin", description: "显示食物饱和度和饥饿值的改进。", downloads: 45_000_000, icon: ["#c0473a", "#7fb238"] },
    { slug: "create", title: "Create", description: "机械动力：用旋转动力和机械组件建造自动化装置。", downloads: 120_000_000, icon: ["#c8703f", "#5b3a2f"] },
    { slug: "fabric-api", title: "Fabric API", description: "大多数 Fabric 模组需要的基础前置库。", downloads: 260_000_000, icon: ["#d9c69c", "#5d5242"] },
  ],
  resourcepack: [
    { slug: "fresh-animations", title: "Fresh Animations", description: "让生物动起来：更生动的走路、呼吸和表情动画。", downloads: 19_000_000, icon: ["#86b84c", "#2e5a3e"] },
    { slug: "faithful-32x", title: "Faithful 32x", description: "原版风格的 32x 高清材质。", downloads: 8_400_000, icon: ["#8f6440", "#c9a462"] },
    { slug: "stay-true", title: "Stay True", description: "细节更丰富、保持原版味道的方块材质。", downloads: 3_100_000 },
    { slug: "xalis-enchanted-books", title: "Enchanted Books", description: "按附魔给附魔书换上不同的颜色。", downloads: 2_600_000, icon: ["#9b6bd6", "#eba4c3"] },
  ],
  shader: [
    { slug: "complementary-reimagined", title: "Complementary Reimagined", description: "兼顾画质与性能的光影，保留原版的感觉。", downloads: 31_000_000, icon: ["#f2cf4a", "#1e4027"], categories: ["iris", "optifine"] },
    { slug: "bsl-shaders", title: "BSL Shaders", description: "明亮通透的经典光影，水面与天空很出色。", downloads: 22_000_000, icon: ["#6fb6e8", "#1b2f44"], categories: ["iris", "optifine"] },
    { slug: "makeup-ultra-fast", title: "MakeUp - Ultra Fast", description: "低配电脑也能流畅运行的轻量光影。", downloads: 9_300_000, categories: ["iris", "optifine"] },
  ],
};

/** Modrinth's tags for the demo projects, so the download filters narrow something. */
const DEMO_TAGS: Record<string, string[]> = {
  sodium: ["optimization"],
  lithium: ["optimization"],
  iris: ["optimization", "decoration"],
  modmenu: ["utility", "library"],
  jei: ["utility", "management"],
  "xaeros-minimap": ["utility", "adventure", "transportation"],
  appleskin: ["food", "utility"],
  create: ["technology", "decoration", "transportation"],
  "fabric-api": ["library"],
  "fresh-animations": ["entities", "vanilla-like", "16x"],
  "faithful-32x": ["vanilla-like", "blocks", "items", "32x"],
  "stay-true": ["vanilla-like", "blocks", "16x"],
  "xalis-enchanted-books": ["items", "tweaks", "16x"],
  "complementary-reimagined": ["iris", "optifine", "semi-realistic", "vanilla-like", "medium", "high"],
  "bsl-shaders": ["iris", "optifine", "semi-realistic", "high", "screenshot"],
  "makeup-ultra-fast": ["iris", "optifine", "vanilla-like", "potato", "low"],
};

/** A tiny pixel picture standing in for a pack's pack.png. */
const packPng = (a: string, b: string) =>
  `data:image/svg+xml,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8" shape-rendering="crispEdges"><rect width="8" height="8" fill="${a}"/><rect x="1" y="1" width="3" height="3" fill="${b}"/><rect x="4" y="4" width="3" height="3" fill="${b}"/><rect x="5" y="1" width="2" height="2" fill="#ffffff" opacity="0.5"/></svg>`)}`;

const packs: { resourcepacks: PackFile[]; shaderpacks: PackFile[] } = {
  resourcepacks: [
    { name: "Faithful 32x - 1.21.zip", bytes: 11_800_000, folder: false, icon: packPng("#4c8a3e", "#a3d35d"), active: 1 },
    { name: "FreshAnimations_v1.9.2.zip", bytes: 3_420_000, folder: false, icon: packPng("#b5653e", "#f2cf4a"), active: 2 },
    { name: "LowFire.zip", bytes: 24_000, folder: false, icon: null, active: null },
  ],
  // Shader packs carry no pack.png: one is known to Modrinth, one is not.
  shaderpacks: [
    { name: "ComplementaryReimagined_r5.4.zip", bytes: 1_040_000, folder: false, icon: null, active: 1 },
    { name: "my-own-shader.zip", bytes: 210_000, folder: false, icon: null, active: null },
  ],
};

/** Projects the demo has "updated", so their update badges go away. */
const updatedProjects = new Set<string>();

let mods: LocalMod[] = [
  { name: "sodium-fabric-0.6.5+mc1.21.4.jar", enabled: true, bytes: 1_204_331 },
  { name: "lithium-fabric-0.14.3+mc1.21.4.jar", enabled: true, bytes: 702_118 },
  { name: "iris-fabric-1.8.1+mc1.21.4.jar", enabled: false, bytes: 2_830_004 },
  { name: "modmenu-13.0.0.jar", enabled: true, bytes: 512_300 },
  { name: "fabric-api-0.110.5+1.21.4.jar", enabled: true, bytes: 2_310_000 },
  { name: "custom-tweaks-1.0.jar", enabled: true, bytes: 48_200 },
];

const java: JavaInstallation[] = [
  { path: "/Library/Java/JavaVirtualMachines/temurin-21.jdk/Contents/Home/bin/java", version: "21.0.5", major: 21, arch: "aarch64", vendor: "Eclipse Adoptium" },
  { path: "/Library/Java/JavaVirtualMachines/zulu-17.jdk/Contents/Home/bin/java", version: "17.0.13", major: 17, arch: "aarch64", vendor: "Azul Systems" },
  { path: "/usr/local/opt/openjdk@8/bin/java", version: "1.8.0_432", major: 8, arch: "x86_64", vendor: "Homebrew" },
];

function account(id: string, provider: Account["provider"], username: string, uuid: string): Account {
  return { id, provider, username, uuid, xuid: null, ownership: provider === "microsoft" ? true : null, skinUrl: null, skinSlim: false, createdAt: now - 30 * 24 * HOUR };
}

function instance(id: string, name: string, versionId: string, loader: string, installed: boolean, last: number | null): Instance {
  return { id, name, versionId, versionType: "release", createdAt: now - 60 * 24 * HOUR, lastLaunchedAt: last, installed, loader, loaderVersion: null, memoryMb: null, javaPath: null, playSeconds: last ? Math.round((now - last) / 40_000) : 0, launch: { ...settings.launch }, accountId: null, favorite: false, group: null, server: null };
}


function versions(): VersionSummary[] {
  const ids: [string, string][] = [];
  for (const v of ["26.3", "26.2", "26.1"]) ids.push([v, "release"]);
  for (const w of ["26w18a", "26w17b", "26w16a"]) ids.push([w, "snapshot"]);
  for (let m = 10; m >= 0; m--) ids.push([m ? `1.21.${m}` : "1.21", "release"]);
  for (let m = 6; m >= 0; m--) ids.push([m ? `1.20.${m}` : "1.20", "release"]);
  for (let m = 4; m >= 0; m--) ids.push([m ? `1.19.${m}` : "1.19", "release"]);
  for (const v of ["1.18.2", "1.18.1", "1.18", "1.17.1", "1.16.5", "1.12.2", "1.8.9", "1.7.10"]) ids.push([v, "release"]);
  return ids.map(([id, versionType], i) => {
    const date = new Date(Date.UTC(2026, 8, 18) - i * 21 * 24 * HOUR).toISOString();
    return { id, versionType, url: "", time: date, releaseTime: date, sha1: "", complianceLevel: 1 };
  });
}

function snapshot(): Snapshot {
  return {
    settings: { ...settings },
    accounts: [...accounts],
    instances: [...instances],
    // ?platform=windows previews the title bar the Windows build draws.
    appInfo: { version: "0.4.0", dataDir: "~/Library/Application Support/com.saudade.mclauncher", platform: new URLSearchParams(location.search).get("platform") ?? "macos", arch: "aarch64" },
  };
}

// ----- multiplayer, screenshots and game options ------------------------------

let servers: Omit<ServerEntry, "index">[] = [
  { name: "方块小镇", address: "play.blocktown.cn", icon: demoIcon("#7fb238", "#2a5034") },
  { name: "朋友的生存服", address: "192.168.1.20:25565", icon: null },
  { name: "Hypixel", address: "mc.hypixel.net", icon: demoIcon("#f2cf4a", "#c0473a") },
  { name: "旧的测试服", address: "old.example.net", icon: null },
];

const span = (text: string, color: string | null = null, bold = false) => ({ text, color, bold, italic: false, underlined: false, strikethrough: false, obfuscated: false });

const serverStatus: Record<string, ServerStatus | null> = {
  "play.blocktown.cn": {
    latencyMs: 38,
    version: "Paper 1.21.4",
    protocol: 769,
    online: 27,
    max: 120,
    players: ["Steve", "Alex_Builds", "小麦", "RedstoneKid", "Notch_fan"],
    motd: [span("方块小镇 ", "#55ff55", true), span("· 纯净生存 · 每周活动", "#ffffff"), span("\n万圣节南瓜大赛进行中！", "#ffaa00")],
    favicon: null,
  },
  "192.168.1.20:25565": {
    latencyMs: 4,
    version: "1.21.4",
    protocol: 769,
    online: 1,
    max: 8,
    players: ["小麦"],
    motd: [span("A Minecraft Server", null)],
    favicon: null,
  },
  "mc.hypixel.net": {
    latencyMs: 286,
    version: "Requires MC 1.8 / 1.21",
    protocol: 47,
    online: 12985,
    max: 200000,
    players: [],
    motd: [span("Hypixel Network ", "#55ff55"), span("[1.8/26.3]", "#ff5555"), span("\nSB GHOST VISITORS | HALLOWEEN EVENT", "#ffaa00", true)],
    favicon: null,
  },
  "old.example.net": null,
};

const listServers = (): ServerEntry[] => servers.map((server, index) => ({ ...server, index }));

/** A pixel landscape standing in for a real screenshot. */
function demoShot(seed: number, sky: string, ground: [string, string], hills: string, sun: string, night = false): string {
  let state = seed;
  const rand = () => ((state = (state * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff);
  const w = 64;
  const h = 36;
  const rects: string[] = [`<rect width="${w}" height="${h}" fill="${sky}"/>`];
  if (night) for (let i = 0; i < 26; i++) rects.push(`<rect x="${Math.floor(rand() * w)}" y="${Math.floor(rand() * 16)}" width="1" height="1" fill="#e8eedc"/>`);
  rects.push(`<rect x="${8 + Math.floor(rand() * 40)}" y="5" width="5" height="5" fill="${sun}"/>`);
  for (let i = 0; i < 3; i++) rects.push(`<rect x="${Math.floor(rand() * 56)}" y="${3 + Math.floor(rand() * 8)}" width="${6 + Math.floor(rand() * 6)}" height="2" fill="#ffffff" opacity="${night ? 0.12 : 0.55}"/>`);
  let far = 15 + Math.floor(rand() * 4);
  let near = 22 + Math.floor(rand() * 4);
  for (let x = 0; x < w; x++) {
    if (rand() < 0.35) far = Math.max(11, Math.min(20, far + (rand() < 0.5 ? -1 : 1)));
    if (rand() < 0.3) near = Math.max(19, Math.min(29, near + (rand() < 0.5 ? -1 : 1)));
    rects.push(`<rect x="${x}" y="${far}" width="1" height="${h - far}" fill="${hills}"/>`);
    rects.push(`<rect x="${x}" y="${near}" width="1" height="1" fill="${ground[0]}"/>`);
    rects.push(`<rect x="${x}" y="${near + 1}" width="1" height="${h - near - 1}" fill="${ground[1]}"/>`);
    if (rand() < 0.06 && x > 2 && x < w - 3) {
      rects.push(`<rect x="${x}" y="${near - 4}" width="1" height="4" fill="#6b4423"/>`, `<rect x="${x - 2}" y="${near - 7}" width="5" height="3" fill="#3f7d31"/>`);
    }
  }
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${w} ${h}" shape-rendering="crispEdges">${rects.join("")}</svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}

const DAY = 24 * HOUR;
let shots: Screenshot[] = [
  [1, "#7fb6e8", ["#7fb238", "#8b5a33"], "#5f8f6a", "#f2cf4a", false],
  [2, "#e59a6b", ["#86b84c", "#74482a"], "#8a5a4a", "#ffe08a", false],
  [3, "#16233c", ["#4f7a2a", "#4a3020"], "#22324a", "#e8eedc", true],
  [4, "#f0c8d8", ["#9ccb52", "#8b5a33"], "#c98aa8", "#fff3b0", false],
  [5, "#d8c08a", ["#e3c47f", "#b38b4a"], "#c49a5a", "#fff3b0", false],
  [6, "#bcd4e6", ["#f4f8fb", "#8b5a33"], "#8aa2b8", "#ffffff", false],
  [7, "#8fc4e8", ["#7fb238", "#8b5a33"], "#4f7f5c", "#f2cf4a", false],
].map(([seed, sky, ground, hills, sun, night], i) => {
  const at = now - (i * 1.7 + 0.3) * DAY;
  const d = new Date(at);
  const pad = (n: number) => String(n).padStart(2, "0");
  return {
    name: `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}_${pad(d.getHours())}.${pad(d.getMinutes())}.${pad(d.getSeconds())}.png`,
    bytes: 1_400_000 + ((seed as number) * 377_000) % 2_000_000,
    modifiedMs: at,
    demoUrl: demoShot(seed as number, sky as string, ground as [string, string], hills as string, sun as string, night as boolean),
  };
});

let gameOptions: GameOptions = { exists: true, lang: "zh_cn", renderDistance: 12, maxFps: 120, guiScale: 0, fov: 70, fullscreen: false, vsync: true, volume: 80 };

const listeners = new Set<(event: LauncherEvent) => void>();
const emit = (event: LauncherEvent) => listeners.forEach((l) => l(event));
const later = (ms: number, f: () => void) => window.setTimeout(f, ms);
const fail = (message: string, code = "demo") => Promise.reject({ code, message, detail: null });

// Worlds running now, each with its log timer.
const running = new Map<string, number>();
const stopTimers = new Map<string, number>();
let installing: { task: string; timer: number } | null = null;
// The latest mod update that "undo" would take back.
let lastModChange: { createdMs: number; added: number; replaced: number } | null = { createdMs: Date.now() - 42 * 60_000, added: 2, replaced: 2 };

// P2P rooms: the first room downloads the component, then health climbs
// from "connecting" as nodes and players arrive. A code starting (U/)ZZZZ
// plays a guest whose host never shows up; PPPP one on a weak relay.
const P2P_SIZE = 32_657_720;
// A cloud server the first world was deployed to.
let remoteTarget: Record<string, unknown> | null = null;
let remoteRunning = true;
let remoteBackups: [string, number][] = [
  ["backups/world-20261031-051700.tar.gz", 184_320_000],
  ["backups/world-20261030-051700.tar.gz", 183_910_000],
];
let p2pInstalled = false;
let p2pPrepStart: number | null = null;
let p2pRoom: { role: "host" | "guest"; code: string; started: number; port: number; local: number; name: string } | null = null;
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function p2pPrepare() {
  if (p2pInstalled) return;
  p2pPrepStart = Date.now();
  await sleep(3200);
  if (p2pPrepStart === null) return fail("已取消");
  p2pPrepStart = null;
  p2pInstalled = true;
}

function p2pCheck(id: string, level: string, label: string, detail: string) {
  return { id, level, label, detail };
}

function p2pStatus() {
  const download = p2pPrepStart === null ? null : { done: Math.min(P2P_SIZE, Math.round(((Date.now() - p2pPrepStart) / 3200) * P2P_SIZE)), total: P2P_SIZE };
  if (!p2pRoom) return { active: false, preparing: p2pPrepStart !== null, download, role: null, code: null, hostname: "", gamePort: 0, address: null, uptimeSeconds: 0, health: null, world: null, members: [], notice: null };
  const e = (Date.now() - p2pRoom.started) / 1000;
  const core = p2pCheck("core", "good", "联机组件", "EasyTier 2.6.4 运行中");
  const nodes = e < 4 ? p2pCheck("nodes", "starting", "共享节点", "正在连接共享节点…") : p2pCheck("nodes", "good", "共享节点", "已连接 3 个，最快 38 ms");
  const nat = e < 4 ? p2pCheck("nat", "starting", "NAT 类型", "正在检测…") : p2pCheck("nat", "good", "NAT 类型", "端口限制锥形 NAT，通常可以直连");
  const world = e < 4 ? null : { motd: "Steve - 生存之地", online: 1 + (e >= 9 ? 1 : 0) + (e >= 16 ? 1 : 0), max: 8, version: "1.20.1", latencyMs: p2pRoom.role === "host" ? 1 : 31 };
  const alex = { name: "Alex", ip: "10.126.126.2", host: false, direct: true, latencyMs: 24, level: "good", detail: "直连 · 24 ms · 丢包 0%" };
  const builder = { name: "Builder", ip: "10.126.126.3", host: false, direct: false, latencyMs: 132, level: "fair", detail: "中转 · 132 ms · 丢包 1%" };
  let checks;
  let players: (typeof alex)[] = [];
  let level: string;
  let summary: string;
  if (p2pRoom.role === "host") {
    players = e >= 16 ? [alex, builder] : e >= 9 ? [alex] : [];
    const game = e < 4 ? p2pCheck("game", "starting", "游戏世界", "正在检查游戏…") : p2pCheck("game", "good", "游戏世界", `已对局域网开放（端口 ${p2pRoom.port}）`);
    const who = players.length === 0 ? p2pCheck("players", "idle", "玩家", "还没有人加入，把房间码发给好友吧。") : p2pCheck("players", players.length > 1 ? "fair" : "good", "玩家", players.length > 1 ? "2 人已连接，其中 1 人走中转" : "1 人已连接");
    checks = [core, nodes, nat, game, who];
    level = e < 4 ? "starting" : players.length > 1 ? "fair" : "good";
    summary = e < 4 ? "正在连接共享节点…" : players.length > 1 ? "可以联机。玩家：2 人已连接，其中 1 人走中转" : players.length ? "1 位玩家已连接，一切正常" : "房间已就绪，等待玩家加入";
  } else {
    const lost = /^(U\/)?ZZZZ/.test(p2pRoom.code);
    const found = !lost && e >= 3;
    const ip = e < 3 ? p2pCheck("ip", "starting", "房间内地址", "正在分配地址…") : p2pCheck("ip", "good", "房间内地址", "10.126.126.2");
    const weak = /^(U\/)?PPPP/.test(p2pRoom.code);
    const host = found && weak
      ? p2pCheck("host", "poor", "房主连接", "中转 · 286 ms · 丢包 9%：试试「换节点」，或让双方都允许防火墙。")
      : found
      ? p2pCheck("host", "good", "房主连接", "直连 · 31 ms · 丢包 0%")
      : lost && e >= 8
        ? p2pCheck("host", "down", "房主连接", "找不到房主：确认房主还开着房间、房间码没错，并且双方至少有一个相同的共享节点。")
        : p2pCheck("host", "starting", "房主连接", "正在寻找房主…");
    const game = found ? p2pCheck("game", "good", "游戏世界", "房主的世界在线，可以加入。") : p2pCheck("game", "starting", "游戏世界", "连上房主后检查…");
    checks = [core, nodes, nat, ip, host, game];
    if (weak && found) checks[1] = p2pCheck("nodes", "fair", "共享节点", "只连上 1 个，延迟 240 ms");
    if (found) players = [{ name: "Steve", ip: "10.126.126.1", host: true, direct: !weak, latencyMs: weak ? 286 : 31, level: weak ? "poor" : "good", detail: host.detail.split("：")[0] }, alex];
    level = host.level;
    summary = found ? (weak ? "连上了房主，但延迟高、丢包多" : "已连上房主，可以进入游戏") : host.detail;
  }
  return {
    active: true,
    preparing: false,
    download: null,
    role: p2pRoom.role,
    code: p2pRoom.code,
    hostname: p2pRoom.name,
    gamePort: p2pRoom.port,
    address: p2pRoom.role === "guest" ? `127.0.0.1:${p2pRoom.local}` : null,
    uptimeSeconds: Math.floor(e),
    health: { level, summary, checks, players, ip: p2pRoom.role === "host" ? "10.126.126.1" : e >= 3 ? "10.126.126.2" : null },
    world: p2pRoom.role === "guest" && !(e >= 3 && !/^(U\/)?ZZZZ/.test(p2pRoom.code)) ? null : world,
    members: p2pRoom.code.startsWith("U/") && (p2pRoom.role === "host" || e >= 3)
      ? [
          { name: p2pRoom.role === "host" ? p2pRoom.name : "Steve", vendor: p2pRoom.role === "host" ? "Islet 0.4.0" : "HMCL 3.6.12", host: true, me: p2pRoom.role === "host" },
          ...players.filter((p) => !p.host).map((p, i) => ({ name: p.name, vendor: i ? "PCL CE 2.10" : "Islet 0.4.0", host: false, me: false })),
        ]
      : [],
    notice: p2pRoom.role === "guest" && p2pRoom.code.startsWith("U/ZZZZ") && e >= 8 ? "连不上房主的房间服务。" : null,
  };
}

// A Scaffolding code: 16 characters whose value (first one lowest) is a
// multiple of 7, as the backend makes them.
const randomCode = () => {
  const alphabet = "0123456789ABCDEFGHJKLMNPQRSTUVWXYZ";
  for (;;) {
    const values = Array.from({ length: 16 }, () => Math.floor(Math.random() * 34));
    if (values.reduceRight((rest, v) => (rest * 34 + v) % 7, 0) !== 0) continue;
    const chars = values.map((v) => alphabet[v]).join("");
    return `U/${chars.match(/.{4}/g)!.join("-")}`;
  }
};

const commands: Record<string, (args: any) => unknown> = {
  quit_when_idle: () => emit({ type: "quitScheduled", data: { scheduled: true } }),
  quit_launcher: () => undefined,
  quit_scheduled: () => false,
  cancel_scheduled_quit: () => emit({ type: "quitScheduled", data: { scheduled: false } }),
  set_unsaved_changes: () => undefined,
  // Nothing to quit in a browser; unsaved edits are still asked about.
  request_quit: ({ unsaved }) => (unsaved ? emit({ type: "quitRequested" }) : undefined),
  bootstrap: () => snapshot(),
  system_memory: ({ id } = {}) => ({ totalMb: 16384, recommendedMb: id && mods.length > 50 ? 5120 : 4096 }),
  frontend_log: ({ message }) => console.info(message),
  login_configured: () => settings.clientId !== "",
  update_settings: ({ settings: next }) => (Object.assign(settings, next), snapshot()),
  add_offline_account: ({ username }) => {
    if (!accounts.some((a) => a.provider === "microsoft" && a.ownership === true)) return fail("离线账号需要先登录一个拥有 Minecraft Java 版的微软账号。", "owner_required");
    if (!/^\S{1,16}$/.test(username)) return fail("用户名不能超过 16 个字符，也不能包含空格");
    const id = `offline:${username.toLowerCase()}`;
    accounts = [...accounts.filter((a) => a.id !== id), account(id, "offline", username, crypto.randomUUID())];
    settings.activeAccountId = id;
    return snapshot();
  },
  remove_account: ({ id }) => {
    accounts = accounts.filter((a) => a.id !== id);
    if (settings.activeAccountId === id) settings.activeAccountId = accounts[0]?.id ?? null;
    return snapshot();
  },
  set_active_account: ({ id }) => ((settings.activeAccountId = id), snapshot()),
  create_instance: ({ name, versionId, versionType, loader, loaderVersion }) => {
    const created = { ...instance(`i-${Date.now()}`, name || "新世界", versionId, loader, false, null), versionType, loaderVersion };
    instances = [...instances, created];
    settings.activeInstanceId = created.id;
    return snapshot();
  },
  delete_instance: ({ id }) => {
    const gone = instances.find((i) => i.id === id);
    if (gone) recycled.push(gone);
    instances = instances.filter((i) => i.id !== id);
    if (settings.activeInstanceId === id) settings.activeInstanceId = instances[0]?.id ?? null;
    return snapshot();
  },
  set_active_instance: ({ id }) => ((settings.activeInstanceId = id), snapshot()),
  configure_instance: ({ id, name, memoryMb, javaPath, loader, loaderVersion }) => {
    instances = instances.map((i) =>
      i.id === id ? { ...i, name, memoryMb, javaPath: javaPath || null, loader, loaderVersion, installed: i.installed && i.loader === loader } : i,
    );
    return snapshot();
  },
  instance_busy: ({ id }) => running.has(id) || installing !== null,
  read_log: () => demoLog(40),
  analyze_crash: () => ({
    diagnoses: [
      {
        kind: "javaVersion",
        title: "需要 Java 21",
        detail:
          "游戏或模组中有用 Java 21 编译的类（class file version 65），当前使用的 Java 版本太低。准备 Java 21 后，启动器会把它设为该世界使用的 Java。",
        fix: { kind: "useJava", major: 21 },
      },
      {
        kind: "missingMod",
        title: "缺少前置模组 fabric-api",
        detail:
          "世界缺少模组 fabric-api 或它的必需依赖，游戏在解析模组时停止加载。请在下载模组页搜索并安装 fabric-api，然后重新启动。",
        fix: { kind: "installMod", query: "fabric-api" },
      },
    ],
    excerpt: demoLog(18)
      .map((line) => line.replace("[Render thread/INFO]", "[main/ERROR]"))
      .join("\n"),
  }),
  install_java_for_instance: ({ id, major }) => {
    const path = `/Users/demo/Library/Application Support/com.saudade.mclauncher/cache/java/adoptium/${major}/aarch64/bin/java`;
    instances = instances.map((instance) => (instance.id === id ? { ...instance, javaPath: path } : instance));
    return { path, version: `${major}.0.2`, major, arch: "aarch64", vendor: "Eclipse Adoptium" };
  },
  load_versions: () => later(500, () => emit({ type: "versions", data: { Ok: { latest: { release: "26.3", snapshot: "26w18a" }, versions: versions(), fromCache: false } } })),
  load_java_runtimes: () => later(600, () => emit({ type: "javaRuntimes", data: java })),
  scan_java_directory: () => later(600, () => emit({ type: "javaRuntimes", data: java })),
  download_java: ({ major }) => {
    later(1500, () => emit({ type: "operationComplete", data: { Ok: `Java ${major} 已就绪` } }));
    return "task-java";
  },
  load_loader_versions: ({ key, loader }) =>
    later(450, () =>
      emit({
        type: "loaderVersions",
        data: {
          key,
          result: { Ok: (loader === "fabric" ? ["0.16.14", "0.16.13", "0.16.12", "0.16.10", "0.15.11"] : ["52.1.0", "52.0.45", "52.0.40", "51.0.33"]).map((version, i) => ({ version, stable: i !== 1 })) },
        },
      }),
    ),
  search_mods: ({ key, query, offset, kind = "mod", categories = [], sort }) =>
    later(500, () => {
      const needle = String(query ?? "").toLowerCase();
      const all = DEMO_CATALOG[kind as keyof typeof DEMO_CATALOG].filter(
        (item) => (!needle || item.slug.includes(needle) || item.title.toLowerCase().includes(needle)) && (categories as string[]).every((tag) => DEMO_TAGS[item.slug]?.includes(tag)),
      );
      if (sort === "downloads") all.sort((a, b) => b.downloads - a.downloads);
      emit({
        type: "modSearch",
        data: {
          key,
          result: {
            Ok: {
              hits: all.map((item) => ({ project_id: item.slug, slug: item.slug, title: item.title, description: item.description, downloads: item.downloads, icon_url: item.icon ? demoIcon(...item.icon) : null, categories: item.categories ?? [] })),
              total_hits: offset + all.length,
            },
          },
        },
      });
    }),
  load_mod_versions: ({ key, project, kind = "mod" }) =>
    later(400, () =>
      emit({
        type: "modVersions",
        data: { key, result: { Ok: ["0.6.5", "0.6.4", "0.6.2"].map((v, i) => ({ id: `${project}@${v}`, project_id: project, name: `${project} ${v}`, version_number: v, version_type: i === 1 ? "beta" : "release", game_versions: ["1.21.4"], loaders: kind === "mod" ? ["fabric"] : kind === "shader" ? ["iris", "optifine"] : ["minecraft"], files: [{ filename: `${project}-${v}.${kind === "mod" ? "jar" : "zip"}`, primary: true, size: 900_000 + i * 1000 }], dependencies: i || kind !== "mod" ? [] : [{ project_id: "fabric-api", dependency_type: "required" }] })) } },
      }),
    ),
  install_pack: ({ version, kind }) => {
    later(1400, () => {
      const list = kind === "shader" ? packs.shaderpacks : packs.resourcepacks;
      const name = `${version}.zip`;
      if (!list.some((p) => p.name === name)) list.push({ name, bytes: 2_000_000, folder: false, icon: null, active: null });
      emit({ type: "operationComplete", data: { Ok: `已安装${kind === "shader" ? "光影包" : "资源包"}：${name}` } });
    });
    return "task-pack";
  },
  list_packs: () => ({ resourcepacks: [...packs.resourcepacks], shaderpacks: [...packs.shaderpacks] }),
  inspect_mods: () =>
    new Promise((resolve) =>
      setTimeout(() => {
        const known: Record<string, [string, string, string, string | null, [string, string] | null]> = {
          sodium: ["sodium", "Sodium", "0.6.4", null, ["#7fb238", "#3f7d31"]],
          lithium: ["lithium", "Lithium", "mc1.21.4-0.14.3", "mc1.21.4-0.14.6", ["#e3c47f", "#8a6a3a"]],
          iris: ["iris", "Iris Shaders", "1.8.1+1.21.4", "1.8.4+1.21.4", ["#9b6bd6", "#3a2a5a"]],
          modmenu: ["modmenu", "Mod Menu", "13.0.0", null, null],
          "fabric-api": ["fabric-api", "Fabric API", "0.110.5+1.21.4", "0.119.2+1.21.4", ["#d9c69c", "#5d5242"]],
        };
        resolve(
          mods.flatMap((mod) => {
            const slug = Object.keys(known).find((k) => mod.name.startsWith(k));
            if (!slug) return [];
            const [id, title, version, update, icon] = known[slug];
            const fresh = !update || updatedProjects.has(id);
            return [{ file: mod.name, projectId: id, slug: id, title, iconUrl: icon ? demoIcon(...icon) : null, versionId: `${id}@${fresh ? update ?? version : version}`, versionNumber: fresh ? update ?? version : version, update: fresh ? null : { versionId: `${id}@${update}`, versionNumber: update } }];
          }),
        );
      }, 700),
    ),
  update_mods: ({ versions }) => {
    for (const version of versions as string[]) updatedProjects.add(version.split("@")[0]);
    later(1600, () => {
      lastModChange = { createdMs: Date.now(), added: versions.length, replaced: versions.length };
      emit({ type: "operationComplete", data: { Ok: `已安装或更新 ${versions.length} 个模组（包含必需依赖）；在「模组」页可以撤销这一次。` } });
    });
    return "task-update";
  },
  install_mod: () => (later(1800, () => emit({ type: "operationComplete", data: { Ok: "模组已安装，必需依赖已一并添加。" } })), "task-mod"),
  list_mods: ({ id }) => (id === "s-paper" ? [...plugins] : id === "s-fabric" ? mods.slice(1, 2) : [...mods]),
  create_server_project: ({ name, versionId, loader }) => {
    const created = { ...instance(`s-${Date.now()}`, name, versionId, loader, false, null), server: { ...projectDefaults } };
    instances = [...instances, created];
    return [snapshot(), created.id];
  },
  configure_server_project: ({ id, name, versionId, loader, project }) => {
    instances = instances.map((i) => (i.id === id ? { ...i, name, versionId, loader, server: project, loaderVersion: i.loader === loader && i.versionId === versionId ? i.loaderVersion : null } : i));
    return snapshot();
  },
  server_versions: async ({ loader }) => {
    await sleep(300);
    return loader === "paper" || loader === "purpur" ? ["1.21.4", "1.21.3", "1.21.1", "1.21", "1.20.6", "1.20.4", "1.20.1", "1.19.4", "1.18.2", "1.16.5", "1.12.2", "1.8.8"] : [];
  },
  last_mod_change: () => lastModChange,
  mod_meta: () => {
    const meta = (id: string, name: string, version: string, description: string, depends: string[] = [], provides: string[] = []) => ({ id, name, version, description, depends, provides });
    const known: Record<string, ReturnType<typeof meta>> = {
      sodium: meta("sodium", "Sodium", "0.6.5+mc1.21.4", "A modern rendering engine for Minecraft which greatly improves performance", ["fabric-rendering-v1", "fabric-resource-loader-v0"]),
      lithium: meta("lithium", "Lithium", "0.14.3+mc1.21.4", "No-compromises game logic optimization mod"),
      iris: meta("iris", "Iris", "1.8.1+mc1.21.4", "A modern shaders mod for Minecraft compatible with existing OptiFine shader packs", ["sodium"]),
      modmenu: meta("modmenu", "Mod Menu", "13.0.0", "Adds a mod menu to view the list of mods you have installed.", ["fabric-screen-api-v1"]),
      "fabric-api": meta("fabric-api", "Fabric API", "0.110.5+1.21.4", "Core API module providing key hooks and intercompatibility features.", [], ["fabric", "fabric-rendering-v1", "fabric-resource-loader-v0", "fabric-screen-api-v1"]),
      "custom-tweaks": meta("custom-tweaks", "Custom Tweaks", "1.0", "", ["cloth-config"]),
    };
    return Object.fromEntries(mods.flatMap((mod) => {
      const found = Object.entries(known).find(([slug]) => mod.name.startsWith(slug));
      return found ? [[mod.name, found[1]]] : [];
    }));
  },
  // Icons inside the demo jars; lithium and iris have none (Modrinth's shows), custom-tweaks none at all.
  mod_icons: () =>
    Object.fromEntries(
      mods.flatMap((mod) => {
        const colours: Record<string, [string, string]> = { sodium: ["#3f7d31", "#a3d35d"], modmenu: ["#5d5242", "#e3c47f"], "fabric-api": ["#d9c69c", "#5d5242"] };
        const found = Object.keys(colours).find((k) => mod.name.startsWith(k));
        return found ? [[mod.name, packPng(...colours[found])]] : [];
      }),
    ),
  inspect_packs: () =>
    new Promise((resolve) =>
      setTimeout(() => resolve([{ kind: "shader", file: "ComplementaryReimagined_r5.4.zip", projectId: "complementary-reimagined", title: "Complementary Shaders - Reimagined", iconUrl: demoIcon("#f2cf4a", "#1e4027") }]), 600),
    ),
  list_servers: () => listServers(),
  edit_server: ({ change }: { change: ServerEdit }) => {
    if (running.size > 0) return fail("游戏运行期间不能修改服务器列表");
    const at = "index" in change ? servers[change.index] : null;
    if ("expected" in change && at?.address !== change.expected) return fail("服务器列表已经变化，请刷新后再试。");
    if (change.kind === "add") servers = [...servers, { name: change.name.trim() || "Minecraft 服务器", address: change.address.trim(), icon: null }];
    if (change.kind === "update") servers = servers.map((server, i) => (i === change.index ? { name: change.name.trim() || "Minecraft 服务器", address: change.address.trim(), icon: server.address === change.address.trim() ? server.icon : null } : server));
    if (change.kind === "remove") servers = servers.filter((_, i) => i !== change.index);
    if (change.kind === "raise" && change.index > 0) {
      const next = [...servers];
      [next[change.index - 1], next[change.index]] = [next[change.index], next[change.index - 1]];
      servers = next;
    }
    return listServers();
  },
  ping_server: ({ address }) =>
    new Promise((resolve, reject) => {
      const status = serverStatus[address];
      setTimeout(
        () => (status ? resolve({ ...status, latencyMs: Math.max(1, status.latencyMs + Math.round((Math.random() - 0.5) * status.latencyMs * 0.3)) }) : reject({ code: "server_offline", message: status === null ? "无法连接到服务器。" : "服务器没有响应。", detail: null })),
        300 + Math.random() * 900,
      );
    }),
  check_server_mods: ({ address }) =>
    address === "play.example.com"
      ? {
          server: "周末生存服",
          missing: [
            { file: "create-1.21.1-6.0.4.jar", sha1: "a".repeat(40), id: "create", url: "https://cdn.modrinth.com/data/x/create.jar" },
            { file: "farmersdelight-1.21.1-1.2.7.jar", sha1: "b".repeat(40), id: "farmersdelight", url: "https://cdn.modrinth.com/data/y/fd.jar" },
          ],
          outdated: ["create-1.21.1-6.0.2.jar"],
          fixable: true,
        }
      : null,
  remote_info: () => ({ target: remoteTarget, hasSecret: !!remoteTarget, defaultDir: "islet/my-survival", pack: remoteTarget ? "/m/instances/i/server-pack/我的生存-给朋友.mrpack" : null }),
  remote_connect: ({ target }) =>
    new Promise((resolve) => {
      remoteTarget = { ...target, fingerprint: "SHA256:demo" };
      setTimeout(() => resolve({ os: "Linux", arch: "x86_64", root: true, tools: ["tar", "curl", "sha256sum", "setsid", "mkfifo", "crontab"], java: null, deployed: false, freeMb: 38_400 }), 700);
    }),
  remote_settings: ({ target }) => {
    remoteTarget = { ...remoteTarget, ...target, fingerprint: remoteTarget?.fingerprint ?? null, deployedAt: remoteTarget?.deployedAt ?? null };
  },
  remote_forget: () => {
    remoteTarget = null;
  },
  remote_status: () => ({
    state: { running: remoteRunning, ready: remoteRunning, backups: remoteBackups, scheduled: "cron" },
    address: String(remoteTarget?.address || remoteTarget?.host || ""),
    ping: remoteRunning ? { latencyMs: 38, version: "1.21.1", protocol: 767, online: 2, max: 20, players: ["Steve", "Alex"], motd: [], favicon: null } : null,
  }),
  remote_action: ({ action }) => {
    if (action === "stop") remoteRunning = false;
    if (action === "start" || action === "restart") remoteRunning = true;
    if (action === "backup") {
      remoteBackups = [["backups/world-20261101-120000.tar.gz", 184_500_000], ...remoteBackups];
      return "backups/world-20261101-120000.tar.gz";
    }
    return action === "stop" ? "stopped" : "started";
  },
  remote_command: () => undefined,
  remote_log: () =>
    [
      "[12:00:01] [Server thread/INFO]: Starting minecraft server version 1.21.1",
      "[12:00:02] [Server thread/INFO]: Loading properties",
      "[12:00:19] [Server thread/INFO]: Preparing level \"world\"",
      "[12:00:31] [Server thread/INFO]: Done (28.412s)! For help, type \"help\"",
      "[12:04:12] [Server thread/INFO]: Steve joined the game",
      "[12:06:40] [Server thread/INFO]: Alex joined the game",
    ].join("\n"),
  remote_download_backup: () => new Promise((resolve) => setTimeout(resolve, 600)),
  list_screenshots: () => [...shots],
  recycle_screenshot: ({ name }) => {
    shots = shots.filter((shot) => shot.name !== name);
  },
  read_game_options: () => ({ ...gameOptions }),
  write_game_options: ({ options }) => {
    if (running.size > 0) return fail("游戏运行期间不能修改游戏选项");
    const patch = Object.fromEntries(Object.entries(options as GameOptions).filter(([, value]) => value != null));
    gameOptions = { ...gameOptions, ...patch, exists: true };
    return { ...gameOptions };
  },
  list_worlds: () => ({ quickPlay: true, worlds: [
    { folder: "新的世界", name: "新的世界", bytes: 48_200_000, modifiedMs: now - 2 * HOUR, icon: null },
    { folder: "机械工厂", name: "机械工厂", bytes: 212_500_000, modifiedMs: now - 3 * 24 * HOUR, icon: null },
    { folder: "生存岛 (1)", name: "生存岛", bytes: 9_800_000, modifiedMs: now - 40 * 24 * HOUR, icon: null },
  ] }),
  list_datapacks: () => [
    { name: "VanillaTweaks.zip", bytes: 84_000, folder: false, description: "原版增强：更多合成与小功能" },
    { name: "coords-hud", bytes: 12_000, folder: true, description: null },
  ],
  list_configs: () => [
    { path: "config/sodium-options.json", bytes: 812, modifiedMs: now - 3 * HOUR },
    { path: "config/create-client.toml", bytes: 4_210, modifiedMs: now - 26 * HOUR },
    { path: "config/jei/jei-client.ini", bytes: 1_904, modifiedMs: now - 50 * HOUR },
  ],
  read_config: ({ path }) =>
    path.endsWith(".json") ? '{\n  "quality": {\n    "weather_quality": "FAST"\n  }\n}\n' : `# ${path}\n[client]\n\tenableOverlay = true\n`,
  write_config: () => null,
  save_backups: () => [
    { path: "/demo/backups/2.auto.zip", bytes: 47_000_000, createdMs: now - 2 * HOUR, auto: true },
    { path: "/demo/backups/1.zip", bytes: 46_000_000, createdMs: now - 26 * HOUR, auto: false },
  ],
  storage_usage: () => ({
    root: "/Users/demo/Library/Application Support/com.saudade.mclauncher",
    worlds: 3_400_000_000,
    libraries: 820_000_000,
    assets: 1_100_000_000,
    versions: 260_000_000,
    unusedVersions: ["1.19.2", "1.8.9"],
    unusedVersionsBytes: 48_000_000,
    java: [
      { path: "/demo/java/adoptium/21/aarch64", major: 21, bytes: 190_000_000, unused: false },
      { path: "/demo/java/adoptium/8/x64", major: 8, bytes: 110_000_000, unused: true },
    ],
    backups: 520_000_000,
    recycle: 230_000_000,
    thumbnails: 18_000_000,
    logs: 6_000_000,
  }),
  known_game_dirs: () => [["官方启动器", "/Users/demo/Library/Application Support/minecraft"]],
  scan_games: () =>
    new Promise((resolve) =>
      setTimeout(
        () =>
          resolve([
            { source: "/m/versions/1.20.1", name: "1.20.1", versionId: "1.20.1", versionType: "release", loader: "vanilla", loaderVersion: null, gameDir: "/m", kind: "official", saves: 4, mods: 0 },
            { source: "/m/versions/my", name: "我的生存", versionId: "1.20.1", versionType: "release", loader: "fabric", loaderVersion: "0.15.11", gameDir: "/m/versions/my", kind: "isolated", saves: 2, mods: 37 },
          ]),
        500,
      ),
    ),
  modpack_info: ({ id }) => (id === "i-create" ? { name: "机械动力：齿轮之城", version: "2.1.0", files: 142, source: { project: "demo-pack", version: "demo-pack-v2" } } : null),
  modpack_update: ({ id }) =>
    id === "i-create"
      ? { id: "demo-pack-v3", project_id: "demo-pack", name: "2.2.0", version_number: "2.2.0", game_versions: ["1.20.1"], loaders: ["forge"], files: [], dependencies: [], version_type: "release" }
      : null,
  account_capes: () => [
    { id: "c1", alias: "Migrator", url: "https://textures.minecraft.net/texture/2340c0e03dd24a11b15a8b33c2a7e9e32abb2051b2481d0ba7defd635ca7a933", state: "ACTIVE" },
  ],
  change_skin: () => snapshot(),
  set_cape: () => undefined,
  login_yggdrasil: ({ username }) => ({ profiles: username === "multi" ? [{ id: "a", name: "Alex_Skin" }, { id: "b", name: "Builder" }] : null, snapshot: snapshot() }),
  check_update: () => null,
  update_source: () => false,
  patch_notes: () => [
    { title: "Minecraft Java Edition 26.3", version: "26.3", kind: "release", date: "2026-09-18T09:00:00Z", summary: "新的生物群系和铜质工具，村民交易再平衡。" },
    { title: "Minecraft Snapshot 26w18a", version: "26w18a", kind: "snapshot", date: "2026-10-22T13:00:00Z", summary: "试验性特性：新的下界结构。" },
  ],
  take_startup_launch: () => null,
  take_open_files: () => [],
  mod_changelog: () => "### Changes\nImproved compatibility and performance.",
  search_modpacks: ({ query }) => ({ hits: [{ project_id: "demo-pack", title: "Demo Adventure Pack", description: "Preview modpack for testing the browser and version picker.", downloads: 123456, icon_url: null }].filter(p => !query || p.title.toLowerCase().includes(String(query).toLowerCase())), total_hits: 1 }),
  modpack_versions: () => [{ id: "demo-pack-v1", project_id: "demo-pack", name: "Demo 1.0", version_number: "1.0", game_versions: ["1.20.1"], loaders: ["fabric"], files: [], dependencies: [], version_type: "release" }],
  tunnel_status: () => ({ running: false, address: "", detail: "" }),
  p2p_defaults: () => ({
    nodes: ["tcp://225284.xyz:11010", "tcp://easytier.weiai.org.cn:11010", "tcp://183.230.36.171:11010", "udp://et.basd1.de:11010", "wss://et.chinokou.cn", "txt://net.qicwken.top"],
    downloadSize: p2pInstalled ? null : P2P_SIZE,
  }),
  // A world is open to LAN only while a game runs.
  p2p_detect_lan: async () => {
    await sleep(1400);
    return running.size > 0 ? [{ motd: "Steve - 生存之地", port: 51234 }] : [];
  },
  p2p_host: async ({ port, name }) => {
    if (!port) return fail("请填写游戏显示的局域网端口。");
    if (p2pRoom) return fail("已经在房间里了，请先退出当前房间。");
    if (port !== 51234 && port !== 25565) return fail(`本机端口 ${port} 上没有游戏：先在游戏里“对局域网开放”，再填它显示的端口。`);
    await p2pPrepare();
    p2pRoom = { role: "host", code: randomCode(), started: Date.now(), port, local: 0, name: name || "Steve" };
  },
  p2p_join: async ({ code, name }) => {
    const scaffolding = String(code).startsWith("U/");
    if (scaffolding ? String(code).slice(2).replace(/[^0-9A-Za-z]/g, "").length !== 16 : String(code).replace(/[^0-9A-Za-z]/g, "").length !== 21)
      return fail(scaffolding ? "房间码不对：应为 U/ 开头的 16 位字母数字，请向房主重新要一次。" : "房间码不对：应为 21 位字母数字（可带横线），请向房主重新要一次。");
    if (p2pRoom) return fail("已经在房间里了，请先退出当前房间。");
    await p2pPrepare();
    p2pRoom = { role: "guest", code: String(code).toUpperCase(), started: Date.now(), port: 51234, local: 50123, name: name || "Alex" };
  },
  p2p_status: () => p2pStatus(),
  p2p_stop: () => {
    p2pRoom = null;
    p2pPrepStart = null;
  },
  set_language: () => undefined,
  mod_details: () => ({
    body: "## Sodium\n\nA **rendering engine** replacement that greatly improves frame rates.\n\n- Faster chunk rendering\n- Lower memory use\n- Works with [Iris](https://modrinth.com/mod/iris) for shaders\n\n> Demo description from the preview backend.\n\n```\nsodium-fabric-0.6.0.jar\n```",
    format: "markdown",
    gallery: [],
    links: [["page", "https://modrinth.com/mod/sodium"], ["source", "https://github.com/CaffeineMC/sodium"], ["issues", "https://github.com/CaffeineMC/sodium/issues"]],
    license: "Polyform Shield",
    updated: new Date(Date.now() - 5 * 86400_000).toISOString(),
  }),
  set_resource_packs: ({ packs: order }) => {
    packs.resourcepacks = packs.resourcepacks.map((p) => ({ ...p, active: order.includes(p.name) ? order.indexOf(p.name) + 1 : null }));
  },
  configure_launch: ({ id, launch, accountId }) => {
    instances = instances.map((i) => (i.id === id ? { ...i, launch, accountId } : i));
    return snapshot();
  },
  organize_instance: ({ id, favorite, group }) => {
    instances = instances.map((i) => (i.id === id ? { ...i, favorite: !!favorite, group: String(group ?? "").trim() || null } : i));
    return snapshot();
  },
  change_version: ({ id, versionId }) => {
    instances = instances.map((i) => (i.id === id ? { ...i, versionId, installed: false } : i));
    return snapshot();
  },
  install_optifine: () => (later(1500, () => emit({ type: "operationComplete", data: { Ok: "已安装 OptiFine_1.20.1_HD_U_I6.jar 和 OptiFabric。" } })), "task-optifine"),
  upload_log: () => new Promise((resolve) => setTimeout(() => resolve("https://mclo.gs/demo123"), 600)),
  create_shortcut: () => "/Users/demo/Desktop/生存 1.21.app",
  recycled_instances: () => [...recycled],
  file_operation: ({ id, operation }) => {
    if (operation.kind === "toggleMod") mods = mods.map((m) => (m.name === operation.name ? { ...m, enabled: !m.enabled } : m));
    if (operation.kind === "setMods") mods = mods.map((m) => (operation.names.includes(m.name) ? { ...m, enabled: operation.enabled } : m));
    if (operation.kind === "recycleMods") mods = mods.filter((m) => !operation.names.includes(m.name));
    if (operation.kind === "changeVersion") instances = instances.map((i) => (i.id === id ? { ...i, versionId: operation.versionId, installed: false } : i));
    const undone = lastModChange;
    if (operation.kind === "undoModUpdate") {
      lastModChange = null;
      updatedProjects.clear();
    }
    const messages: Record<string, string> = {
      duplicate: "世界已复制，包含存档和模组。",
      backup: "备份已保存：backups/i-survival/2026-10-27.zip",
      restoreBackup: "已恢复。恢复前的数据已另存备份。",
      restoreDeleted: "世界已从回收站恢复。",
      addMods: "模组已添加。",
      toggleMod: "模组状态已更新。",
      setMods: operation.enabled ? `已启用 ${operation.names?.length} 个模组。` : `已禁用 ${operation.names?.length} 个模组。`,
      recycleMods: `${operation.names?.length} 个模组已移到回收站。`,
      importPack: "整合包已导入，首次开始游戏时安装。",
      recycle: `「${operation.name}」已移到回收站。`,
      addPacks: "已添加 1 个资源包。",
      addDatapacks: "已添加 1 个数据包，下次进入存档时生效。",
      renameSave: `存档已改名为「${operation.name}」。`,
      backupSave: "存档已备份。",
      restoreSave: "存档已恢复，恢复前的存档在回收站里。",
      importSave: "存档已导入。",
      exportSave: "存档已导出。",
      purgeRecycle: operation.only ? "已永久删除这个世界。" : "回收站已清空。",
      clean: "已释放 48 MB。",
      importGame: "已导入「我的生存」，开始游戏时会下载所需文件。",
      updatePack: "整合包已更新，开始游戏时会下载新文件。",
      updateModpackOnline: "整合包已更新到新版本，开始游戏时会下载新文件。",
      exportPack: "已导出：118 个文件从 Modrinth 下载，24 个直接打包。",
      deployServer: "服务器已经在 203.0.113.10 运行，带 112 个模组。把「我的生存-给朋友.mrpack」发给朋友，导入后就能进服。",
      fixServerMods: "已补齐 2 个模组，1 个旧版本移到了回收站。",
      exportServer: "服务端已生成，带 112 个模组：运行文件夹里的 start.bat（Windows）或 start.sh 启动。6 个只在客户端运行的模组没有放进去。",
      changeVersion: `已换成 ${operation.versionId}；换之前把 3 个存档各备份了一份，在存档的「备份」里可以恢复。`,
      undoModUpdate: `已撤销上次模组更新：移除 ${undone?.added ?? 0} 个文件，恢复 ${undone?.replaced ?? 0} 个旧文件。`,
    };
    if (operation.kind === "recycle" && operation.item === "mod") mods = mods.filter((m) => m.name !== operation.name);
    if (operation.kind === "deployServer") {
      const stages: [string, number, number][] = [["连接服务器", 0, 0], ["生成服务端", 0, 0], ["在服务器上安装 Java", 0, 0], ["上传", 40_000_000, 160_000_000], ["上传", 120_000_000, 160_000_000], ["等待服务器启动", 12, 0]];
      stages.forEach(([stage, done, total], i) => later(600 * i, () => emit({ type: "remoteProgress", data: { instanceId: id, stage, done, total } })));
      return later(600 * stages.length, () => {
        if (remoteTarget) remoteTarget = { ...remoteTarget, deployedAt: Date.now() };
        remoteRunning = true;
        emit({ type: "operationComplete", data: { Ok: messages.deployServer } });
      });
    }
    later(700, () => emit({ type: "operationComplete", data: { Ok: messages[operation.kind] } }));
  },
  relocate_storage: () => later(1500, () => emit({ type: "storageChanged", data: { Ok: "/Volumes/Games/MC" } })),
  install_instance: ({ id }) => {
    if (installing) return fail("该世界正在下载，请稍候。");
    const task = `task-${Date.now()}`;
    const totalBytes = 617_600_000;
    let tick = 0;
    const timer = window.setInterval(() => {
      tick += 1;
      const stage = tick < 4 ? "metadata" : tick < 40 ? "download" : tick < 46 ? "java" : "finalize";
      const ratio = Math.min(1, Math.max(0, (tick - 4) / 36));
      emit({ type: "installProgress", data: { taskId: task, instanceId: id, stage, done: Math.round(5229 * ratio), total: 5229, bytes: Math.round(totalBytes * ratio), totalBytes, message: "" } });
      if (tick >= 48) {
        window.clearInterval(timer);
        installing = null;
        instances = instances.map((i) => (i.id === id ? { ...i, installed: true } : i));
        emit({ type: "installComplete", data: { taskId: task, instanceId: id, versionId: instances.find((i) => i.id === id)?.versionId ?? "" } });
      }
    }, 200);
    installing = { task, timer };
    return task;
  },
  cancel_task: ({ taskId }) => {
    const current = installing;
    if (current && current.task === taskId) {
      window.clearInterval(current.timer);
      installing = null;
      emit({ type: "installError", data: { taskId, instanceId: "", error: { code: "cancelled", message: "已取消", detail: null } } });
    }
  },
  launch_instance: ({ id, offline }) => {
    later(900, () => {
      // `localStorage.mockSignIn = "auth_offline"` (or "auth_expired") in the
      // console tries the sign-in failures out.
      const player = accounts.find((a) => a.id === (instances.find((i) => i.id === id)?.accountId ?? settings.activeAccountId));
      if (player?.provider === "offline" && !accounts.some((a) => a.provider === "microsoft" && a.ownership === true)) {
        emit({ type: "launchError", data: { instanceId: id, error: { code: "owner_required", message: "离线账号需要先登录一个拥有 Minecraft Java 版的微软账号。", detail: null } } });
        return;
      }
      const failure = offline ? null : localStorage.getItem("mockSignIn");
      if (failure) {
        const message = failure === "auth_expired" ? "登录已过期，请重新登录。" : "连不上登录服务器。可以先以离线身份玩单人游戏，存档和进度都在；联机服务器要等网络恢复。";
        emit({ type: "launchError", data: { instanceId: id, error: { code: failure, message, detail: null } } });
        return;
      }
      emit({ type: "launchStarted", data: { instanceId: id, pid: 4242 } });
      instances = instances.map((i) => (i.id === id ? { ...i, lastLaunchedAt: Date.now() } : i));
      let n = 0;
      const timer = window.setInterval(() => {
        n += 1;
        emit({ type: "launchLogs", data: { instanceId: id, lines: demoLog(n < 10 ? 30 : 2).map((line) => ({ line, stream: "stdout" })) } });
      }, 300);
      running.set(id, timer);
    });
  },
  stop_instance: ({ id, force }) => {
    // A gentle stop takes a few seconds, the way a real game saves first.
    window.clearTimeout(stopTimers.get(id));
    stopTimers.set(
      id,
      window.setTimeout(() => {
        stopTimers.delete(id);
        window.clearInterval(running.get(id));
        running.delete(id);
        emit({ type: "launchExit", data: { instanceId: id, code: 0, success: true } });
      }, force ? 300 : 4000),
    );
  },
  start_login: ({ mode }) => {
    const loginId = `login-${Date.now()}`;
    later(700, () =>
      emit({
        type: "loginStarted",
        data: mode === "device"
          ? { loginId, mode, authorizeUrl: null, userCode: "MCLX-7Q4P", verificationUri: "https://www.microsoft.com/link", expiresIn: 900 }
          : { loginId, mode, authorizeUrl: "https://login.live.com/oauth20_authorize.srf?demo", userCode: null, verificationUri: null, expiresIn: null },
      }),
    );
    return loginId;
  },
  cancel_login: ({ loginId }) => emit({ type: "loginError", data: { loginId, error: { code: "cancelled", message: "已取消", detail: null } } }),
  open_location: () => undefined,
  open_url: ({ url }) => void window.open(url, "_blank"),
};

let logLine = 0;
function demoLog(count: number): string[] {
  const samples = [
    "[Render thread/INFO]: Setting user: Steve",
    "[Render thread/INFO]: Backend library: LWJGL version 3.3.3",
    "[Worker-Main-2/INFO]: Loaded 1290 recipes",
    "[Render thread/INFO]: Sodium renderer initialized",
    "[Server thread/INFO]: Preparing spawn area: 84%",
    "[Render thread/WARN]: Missing sound for event: minecraft:item.goat_horn.play",
    "[Server thread/INFO]: Steve joined the game",
  ];
  return Array.from({ length: count }, () => {
    logLine += 1;
    const t = new Date().toTimeString().slice(0, 8);
    return `[${t}] ${samples[logLine % samples.length]}`;
  });
}

export const mockBackend: Backend = {
  async call<T>(command: string, args: Record<string, unknown> = {}) {
    const handler = commands[command];
    if (!handler) return fail(`demo backend has no ${command}`) as Promise<T>;
    await new Promise((r) => setTimeout(r, 60));
    return (await handler(args)) as T;
  },
  async subscribe(handler) {
    listeners.add(handler);
    return () => listeners.delete(handler);
  },
  async pick(options) {
    if (options.directory) return ["/Users/demo/Games"];
    return options.multiple ? ["/Users/demo/Downloads/appleskin-3.0.6.jar"] : ["/Users/demo/Downloads/demo.mrpack"];
  },
  async pickSave(options) {
    return `/Users/demo/Downloads/${options.defaultPath}`;
  },
  reveal() {},
  setWindowTitle() {},
  setWindowTheme() {},
  windowAction() {},
  async onMaximized(handler) {
    handler(false);
    return () => {};
  },
};
