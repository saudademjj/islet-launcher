import { t } from "./i18n";

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value >= 100 ? 0 : 1)} ${units[unit]}`;
}

export function formatSpeed(bytesPerSecond: number): string {
  return `${formatBytes(bytesPerSecond)}/s`;
}

/** 950 / 1.2K / 34K / 10.5M — compact counts for download numbers. */
export function formatCount(value: number): string {
  if (value < 1000) return String(value);
  if (value < 1_000_000) return `${(value / 1000).toFixed(value < 10_000 ? 1 : 0)}K`;
  return `${(value / 1_000_000).toFixed(value < 10_000_000 ? 1 : 0)}M`;
}

export function formatDate(millis: number | null): string {
  if (!millis) return "—";
  const d = new Date(millis);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

export function formatRelative(millis: number | null): string {
  if (!millis || millis <= 0) return t("尚未游玩", "Never played");
  const seconds = Math.max(0, (Date.now() - millis) / 1000);
  if (seconds < 60) return t("刚刚", "just now");
  if (seconds < 3600) return t(`${Math.floor(seconds / 60)} 分钟前`, `${Math.floor(seconds / 60)} min ago`);
  if (seconds < 86_400) return t(`${Math.floor(seconds / 3600)} 小时前`, `${Math.floor(seconds / 3600)} h ago`);
  if (seconds < 2_592_000) return t(`${Math.floor(seconds / 86_400)} 天前`, `${Math.floor(seconds / 86_400)} d ago`);
  return formatDate(millis);
}

/** Total play time: minutes under an hour, then hours with one decimal. */
export function formatPlayTime(seconds: number): string {
  if (!seconds || seconds < 60) return t("不到 1 分钟", "Under a minute");
  if (seconds < 3600) return t(`${Math.floor(seconds / 60)} 分钟`, `${Math.floor(seconds / 60)} min`);
  const hours = seconds / 3600;
  return t(`${hours < 10 ? hours.toFixed(1).replace(/\.0$/, "") : Math.round(hours)} 小时`, `${hours < 10 ? hours.toFixed(1).replace(/\.0$/, "") : Math.round(hours)} h`);
}

export function formatReleaseDate(value: string): string {
  return value.slice(0, 10);
}

export function loaderLabel(loader: string): string {
  switch (loader) {
    case "legacyfabric":
      return "Legacy Fabric";
    case "liteloader":
      return "LiteLoader";
    case "cleanroom":
      return "Cleanroom";
    case "fabric":
      return "Fabric";
    case "quilt":
      return "Quilt";
    case "forge":
      return "Forge";
    case "neoforge":
      return "NeoForge";
    default:
      return t("原版", "Vanilla");
  }
}

export function versionTypeLabel(kind: string): string {
  switch (kind) {
    case "release":
      return t("正式版", "Release");
    case "snapshot":
      return t("快照", "Snapshot");
    case "old_beta":
      return t("旧测试版", "Old beta");
    case "old_alpha":
      return t("旧 Alpha", "Old alpha");
    default:
      return kind;
  }
}

export function stageLabel(stage: string): string {
  switch (stage) {
    case "metadata":
      return t("获取版本信息", "Getting version info");
    case "download":
      return t("下载游戏文件", "Downloading game files");
    case "java":
      return t("准备 Java", "Preparing Java");
    case "finalize":
      return t("完成安装", "Finishing");
    default:
      return t("处理中", "Working");
  }
}

/** Byte progress when sizes are known, else file count; null while unknown. */
export function progressRatio(done: number, total: number, bytes: number, totalBytes: number): number | null {
  if (totalBytes > 0) return Math.min(1, bytes / totalBytes);
  if (total > 0) return Math.min(1, done / total);
  return null;
}

export function validGuid(value: string): boolean {
  return /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(value);
}

/** Java builds report arch as aarch64/arm64 or x86_64/amd64; compare families. */
export function sameArch(a: string, b: string): boolean {
  const family = (arch: string) => {
    const v = arch.toLowerCase();
    if (v === "aarch64" || v === "arm64") return "arm64";
    if (v === "x86_64" || v === "amd64" || v === "x64") return "x64";
    return v;
  };
  return family(a) === family(b);
}

export function greeting(date = new Date()): string {
  const hour = date.getHours();
  if (hour < 5) return t("夜深了", "Up late");
  if (hour < 11) return t("早上好", "Good morning");
  if (hour < 13) return t("中午好", "Good afternoon");
  if (hour < 18) return t("下午好", "Good afternoon");
  return t("晚上好", "Good evening");
}

/** 10月27日 · 星期一 */
export function formatToday(date = new Date()): string {
  return t(`${date.getMonth() + 1}月${date.getDate()}日 · 星期${"日一二三四五六"[date.getDay()]}`, `${["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"][date.getDay()]}, ${["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"][date.getMonth()]} ${date.getDate()}`);
}

export type SkyPhase = "dawn" | "day" | "dusk" | "night";

/** The sky by the local time: dawn 5–7, day 7–17, dusk 17–19, night after. */
export function skyPhase(date = new Date()): SkyPhase {
  const hour = date.getHours();
  if (hour >= 5 && hour < 7) return "dawn";
  if (hour >= 7 && hour < 17) return "day";
  if (hour >= 17 && hour < 19) return "dusk";
  return "night";
}

/** Night from 19:00 to 5:00; the hero shows the moon and stars. */
export function isNight(date = new Date()): boolean {
  return skyPhase(date) === "night";
}

// Players remember updates by name, not number. Official Chinese names of the
// major updates; a point release inherits its update's name. Only names that
// are certain are listed; anything newer simply shows none.
const UPDATE_NAMES: [RegExp, string][] = [
  [/^1\.21\.4$/, t("苍园觉醒", "The Garden Awakens")],
  [/^1\.21(\.1)?$/, t("棘巧试炼", "Tricky Trials")],
  [/^1\.20(\.\d+)?$/, t("足迹与故事", "Trails & Tales")],
  [/^1\.19(\.\d+)?$/, t("荒野更新", "The Wild Update")],
  [/^1\.18(\.\d+)?$/, t("洞穴与山崖：第二部分", "Caves & Cliffs: Part II")],
  [/^1\.17(\.\d+)?$/, t("洞穴与山崖：第一部分", "Caves & Cliffs: Part I")],
  [/^1\.16(\.\d+)?$/, t("下界更新", "Nether Update")],
  [/^1\.15(\.\d+)?$/, t("嗡嗡蜂群", "Buzzy Bees")],
  [/^1\.14(\.\d+)?$/, t("村庄与掠夺", "Village & Pillage")],
  [/^1\.13(\.\d+)?$/, t("海洋更新", "Update Aquatic")],
  [/^1\.12(\.\d+)?$/, t("多彩世界", "World of Color")],
  [/^1\.11(\.\d+)?$/, t("探险更新", "Exploration Update")],
  [/^1\.10(\.\d+)?$/, t("冰霜更新", "Frostburn Update")],
  [/^1\.9(\.\d+)?$/, t("战斗更新", "Combat Update")],
  [/^1\.8(\.\d+)?$/, t("缤纷更新", "Bountiful Update")],
  [/^1\.7(\.\d+)?$/, t("改变世界的更新", "The Update that Changed the World")],
];

/** "足迹与故事" for 1.20.1; null when the version has no known update name. */
export function updateName(versionId: string): string | null {
  return UPDATE_NAMES.find(([pattern]) => pattern.test(versionId))?.[1] ?? null;
}
