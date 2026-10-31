import { motion } from "motion/react";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";

import { call } from "../api/backend";
import type { Instance, LocalMod, PackLists, Screenshot, ServerEntry, Theme, VersionSummary, WorldList } from "../api/types";
import { Icon, type IconName } from "../components/Icon";
import { ModIcon } from "../components/ModIcon";
import { useModIcons, usePackInsights } from "../lib/contentIcons";
import { Landscape, PixelIcon, PX } from "../components/Pixel";
import { SceneCover } from "../components/SceneCover";
import { Motd, Signal } from "../components/Server";
import { Identicon } from "../components/ui";
import { biomeStyle, useBiome } from "../lib/biome";
import { formatBytes, formatPlayTime, formatRelative, loaderLabel, updateName } from "../lib/format";
import { findModName, slugOfFile } from "../lib/modNames";
import { useP2p } from "../lib/p2p";
import { MOD_KEY } from "../lib/platform";
import { usePrefs } from "../lib/prefs";
import { shotTime, shotUrl, useShots } from "../lib/screenshots";
import { aliasesOf, matchItem, queryWords } from "../lib/search";
import { usePing, useServers } from "../lib/servers";
import { dayLabel } from "./Screenshots";
import { instanceState, usePick, useStore } from "../store";
import { isEnglish, t } from "../lib/i18n";

// ---------------------------------------------------------------------------
// One search for everything: worlds, saves, servers, mods and packs, screen-
// shots, single settings and actions. Content shows its own picture; commands
// show a line glyph. ↵ runs a row's main action, ⌘↵ its second one; Tab
// narrows the search to one kind, as do the prefixes @ # / and >.
// ---------------------------------------------------------------------------

type Kind = "world" | "save" | "server" | "mod" | "pack" | "shot" | "setting" | "action" | "page" | "version" | "online";

interface Action {
  label: string;
  run(): void;
}

interface Item {
  id: string;
  kind: Kind;
  label: string;
  sub?: string;
  /** Extra words that find this item (English names, pinyin-free aliases). */
  keywords?: string;
  icon: ReactNode;
  primary: Action;
  secondary?: Action;
  /** A value shown at the row's end, such as a setting's current state. */
  value?: string;
  shortcut?: string;
  /** The one dominant row: start the current world. */
  hero?: boolean;
  preview?: () => ReactNode;
}

const GROUP: Record<Kind, string> = {
  world: t("世界", "Worlds"),
  save: t("存档", "Saves"),
  server: t("服务器", "Servers"),
  mod: t("模组与资源", "Mods & packs"),
  pack: t("模组与资源", "Mods & packs"),
  shot: t("截图", "Screenshots"),
  setting: t("设置", "Settings"),
  action: t("操作", "Actions"),
  page: t("前往", "Go to"),
  version: t("新建世界", "New world"),
  online: t("在线搜索", "Online search"),
};

/** Ties between equally good matches go to the kinds people open most. */
const KIND_WEIGHT: Record<Kind, number> = { world: 8, action: 6, save: 6, server: 5, setting: 5, page: 4, mod: 3, pack: 2, shot: 1, version: 7, online: -50 };

interface Scope {
  id: string;
  label: string;
  prefix?: string;
  kinds: Kind[];
}

const SCOPES: Scope[] = [
  { id: "all", label: t("全部", "All"), kinds: [] },
  { id: "world", label: t("世界", "Worlds"), kinds: ["world", "version"] },
  { id: "save", label: t("存档", "Saves"), kinds: ["save"] },
  { id: "server", label: t("服务器", "Servers"), prefix: "@", kinds: ["server"] },
  { id: "mod", label: t("模组与资源", "Mods & packs"), prefix: "#", kinds: ["mod", "pack", "online"] },
  { id: "shot", label: t("截图", "Screenshots"), kinds: ["shot"] },
  { id: "setting", label: t("设置", "Settings"), prefix: "/", kinds: ["setting"] },
  { id: "action", label: t("操作", "Actions"), prefix: ">", kinds: ["action", "page"] },
];

const glyph = (name: IconName) => <Icon name={name} size={15} />;

// ----- recents ---------------------------------------------------------------

const RECENT_KEY = "mc-launcher.palette-recent";

function readRecent(): string[] {
  try {
    const list = JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]");
    return Array.isArray(list) ? list.filter((x) => typeof x === "string") : [];
  } catch {
    return [];
  }
}

function remember(id: string) {
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify([id, ...readRecent().filter((x) => x !== id)].slice(0, 8)));
  } catch {
    // Without storage the palette simply has no recents.
  }
}

// ----- data ------------------------------------------------------------------

/** Saves of every installed world, read once when the palette opens. */
function useAllSaves(instances: Instance[]): Record<string, WorldList> {
  const [saves, setSaves] = useState<Record<string, WorldList>>({});
  const ids = instances
    .filter((i) => i.installed)
    .slice(0, 12)
    .map((i) => i.id)
    .join(",");
  useEffect(() => {
    let live = true;
    for (const id of ids.split(",").filter(Boolean)) {
      call<WorldList>("list_worlds", { id })
        .then((list) => live && setSaves((all) => ({ ...all, [id]: list })))
        .catch(() => {});
    }
    return () => {
      live = false;
    };
  }, [ids]);
  return saves;
}

/** Mods and packs of the current world. */
function useContent(active: Instance | null): { mods: LocalMod[]; packs: PackLists | null } {
  const [mods, setMods] = useState<LocalMod[]>([]);
  const [packs, setPacks] = useState<PackLists | null>(null);
  const id = active?.installed ? active.id : null;
  const modded = active?.loader !== "vanilla";
  useEffect(() => {
    if (!id) return;
    let live = true;
    if (modded) call<LocalMod[]>("list_mods", { id }).then((list) => live && setMods(list)).catch(() => {});
    call<PackLists>("list_packs", { id }).then((list) => live && setPacks(list)).catch(() => {});
    return () => {
      live = false;
    };
  }, [id, modded]);
  return { mods, packs };
}

// ----- items -----------------------------------------------------------------

function useItems(query: string): { items: Item[]; quick: Item[] } {
  const s = usePick((s) => ({
    instances: s.instances,
    settings: s.settings,
    running: s.running,
    runningIds: s.runningIds,
    launchPending: s.launchPending,
    install: s.install,
    installQueue: s.installQueue,
    reduceMotion: s.reduceMotion,
    versions: s.versions,
    systemMemory: s.systemMemory,
  }));
  const devInfo = usePrefs((p) => p.devInfo);
  const active = s.instances.find((i) => i.id === s.settings.activeInstanceId) ?? null;
  const saves = useAllSaves(s.instances);
  const serverLists = useServers((st) => st.lists);
  const shots = useShots((st) => (active ? st.lists[active.id] : undefined));
  const content = useContent(active);
  const modIcons = useModIcons(active?.id ?? "", content.mods);
  const packInsights = usePackInsights(active?.id ?? "", content.packs);

  // Load what is missing: every installed world's servers, the current world's screenshots.
  useEffect(() => {
    const servers = useServers.getState();
    for (const instance of s.instances) if (instance.installed && servers.lists[instance.id] === undefined) void servers.load(instance.id);
    if (active && useShots.getState().lists[active.id] === undefined) void useShots.getState().load(active.id);
    // Only on open: the lists refresh themselves after each session.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const items = useMemo(() => {
    const store = useStore.getState();
    const list: Item[] = [];
    const settings = s.settings;
    const update = (patch: Partial<typeof settings>, message: string) =>
      void store.mutate("update_settings", { settings: { ...useStore.getState().settings, ...patch } }, message);

    // Worlds: ↵ starts (or installs, or stops) it, ⌘↵ makes it current.
    for (const instance of s.instances) {
      const state = instanceState(instance, s);
      const isActive = instance.id === active?.id;
      const start =
        state === "running" ? t("停止游戏", "Stop game") : state === "queued" ? t("取消排队", "Leave queue") : state === "notInstalled" ? t("安装并开始", "Install and play") : state === "starting" || state === "installing" ? t("查看进度", "Show progress") : t("开始游戏", "Play");
      const item: Item = {
        id: `world:${instance.id}`,
        kind: "world",
        label: instance.name,
        sub: [instance.versionId, loaderLabel(instance.loader), isActive ? t("当前世界", "Current world") : instance.lastLaunchedAt ? t(`${formatRelative(instance.lastLaunchedAt)}玩过`, `Played ${formatRelative(instance.lastLaunchedAt)}`) : t("尚未游玩", "Never played")].join(" · "),
        keywords: `${instance.versionId} ${updateName(instance.versionId) ?? ""} ${instance.loader} world instance 世界 实例 ${aliasesOf(instance.name)}`,
        icon: <Identicon seed={instance.id} size={24} />,
        hero: isActive,
        primary: {
          label: start,
          run: () => {
            store.setActiveInstance(instance.id);
            store.setPage("home");
            if (state !== "starting" && state !== "installing") store.primaryAction(instance.id);
          },
        },
        secondary: isActive
          ? { label: t("管理", "Manage"), run: () => store.openManage(instance.id) }
          : {
              label: t("设为当前世界", "Make current"),
              run: () => {
                store.setActiveInstance(instance.id);
                store.setPage("home");
              },
            },
        preview: () => <WorldPreview instance={instance} saves={saves[instance.id]} />,
      };
      list.push(item);
    }

    // Saves: ↵ enters the save directly when the version allows it.
    for (const instance of s.instances) {
      const ws = saves[instance.id];
      if (!ws) continue;
      for (const world of ws.worlds) {
        list.push({
          id: `save:${instance.id}:${world.folder}`,
          kind: "save",
          label: world.folder,
          sub: t(`存档 · ${instance.name}${world.modifiedMs ? ` · ${formatRelative(world.modifiedMs)}` : ""}`, `Save · ${instance.name}${world.modifiedMs ? ` · ${formatRelative(world.modifiedMs)}` : ""}`),
          keywords: `save world 存档 ${instance.name}`,
          icon: world.icon ? <img className="palette-img pixelated" src={world.icon} alt="" /> : <PixelIcon name="map" size={20} />,
          primary: ws.quickPlay
            ? { label: t("进入存档", "Open save"), run: () => (store.setActiveInstance(instance.id), void store.launchInstance(instance.id, world.folder)) }
            : { label: t("查看存档", "Show saves"), run: () => store.openManage(instance.id, "data") },
          secondary: { label: t("在文件夹中显示", "Show in folder"), run: () => void store.open({ kind: "world", id: instance.id, folder: world.folder }) },
          preview: () => (
            <div className="pv">
              <div className="pv-figure">{world.icon ? <img className="pv-save pixelated" src={world.icon} alt="" /> : <PixelIcon name="map" size={40} />}</div>
              <div className="pv-title">{world.folder}</div>
              <dl className="pv-facts">
                <dt>{t("所在世界", "World")}</dt>
                <dd>{instance.name}</dd>
                <dt>{t("大小", "Size")}</dt>
                <dd>{formatBytes(world.bytes)}</dd>
                <dt>{t("上次游玩", "Last played")}</dt>
                <dd>{world.modifiedMs ? formatRelative(world.modifiedMs) : t("未知", "Unknown")}</dd>
              </dl>
              <p className="pv-note">{ws.quickPlay ? t("回车直接进入这个存档，跳过标题画面。", "Press Enter to go straight into this save, skipping the title screen.") : t("这个版本不支持直接进入存档（需要 1.20 以上），回车打开存档列表。", "This version can't open saves directly (needs 1.20+); Enter opens the save list.")}</p>
            </div>
          ),
        });
      }
    }

    // Servers of every installed world, one row per address: joining goes
    // through the current world when it lists the server, else the first.
    const seen = new Map<string, Instance[]>();
    const ordered = active ? [active, ...s.instances.filter((i) => i.id !== active.id)] : s.instances;
    for (const instance of ordered) {
      if (!instance.installed) continue;
      for (const server of serverLists[instance.id] ?? []) {
        const key = server.address.toLowerCase();
        const owners = seen.get(key);
        if (owners) {
          owners.push(instance);
          continue;
        }
        const worldsWith = [instance];
        seen.set(key, worldsWith);
        list.push({
          id: `server:${instance.id}:${server.index}`,
          kind: "server",
          label: server.name || server.address,
          get sub() {
            return t(`${server.address} · ${worldsWith.length > 1 ? `${instance.name} 等 ${worldsWith.length} 个世界` : instance.name}`, `${server.address} · ${worldsWith.length > 1 ? `${instance.name} and ${worldsWith.length - 1} more` : instance.name}`);
          },
          keywords: `server multiplayer join 服务器 多人 联机 加入 ${instance.name}`,
          icon: server.icon ? <img className="palette-img pixelated" src={server.icon} alt="" /> : <Identicon seed={server.address} size={24} />,
          primary: { label: t("加入", "Join"), run: () => (store.setActiveInstance(instance.id), void store.launchInstance(instance.id, undefined, server.address)) },
          secondary: { label: t("管理服务器", "Manage servers"), run: () => store.openManage(instance.id, "servers") },
          preview: () => <ServerPreview server={server} instance={instance} />,
        });
      }
    }

    // The current world's mods and packs.
    if (active) {
      for (const mod of content.mods) {
        const slug = slugOfFile(mod.name);
        const known = findModName(slug);
        const label = known ? (isEnglish ? known.en : `${known.en} · ${known.zh}`) : slug;
        list.push({
          id: `mod:${mod.name}`,
          kind: "mod",
          label,
          sub: t(`${mod.enabled ? "模组" : "已停用"} · ${mod.name}`, `${mod.enabled ? "Mod" : "Disabled"} · ${mod.name}`),
          keywords: `mod 模组 ${slug} ${known?.aliases?.join(" ") ?? ""}`,
          icon: <ModIcon src={modIcons[mod.name]} seed={slug} size={24} glyph="mod" />,
          value: mod.enabled ? undefined : t("停用", "Turn off"),
          primary: { label: t("打开模组页", "Open Mods tab"), run: () => store.openManage(active.id, "mods") },
          secondary: { label: mod.enabled ? t("停用", "Turn off") : t("启用", "Enable"), run: () => void store.fileOperation(active.id, { kind: "toggleMod", name: mod.name }) },
          preview: () => (
            <div className="pv">
              <div className="pv-figure">
                <ModIcon src={modIcons[mod.name]} seed={slug} size={56} glyph="mod" />
              </div>
              <div className="pv-title">{known ? (isEnglish ? known.en : known.zh) : slug}</div>
              {known && <div className="pv-sub">{known.en}</div>}
              <dl className="pv-facts">
                <dt>{t("文件", "File")}</dt>
                <dd className="mono">{mod.name}</dd>
                <dt>{t("大小", "Size")}</dt>
                <dd>{formatBytes(mod.bytes)}</dd>
                <dt>{t("状态", "Status")}</dt>
                <dd>{mod.enabled ? t("启用", "Enable") : t("已停用", "Disabled")}</dd>
              </dl>
            </div>
          ),
        });
      }
      const packRows: [PackLists["resourcepacks"], "resourcepack" | "shader"][] = [
        [content.packs?.resourcepacks ?? [], "resourcepack"],
        [content.packs?.shaderpacks ?? [], "shader"],
      ];
      for (const [packs, kind] of packRows) {
        for (const pack of packs) {
          const name = pack.name.replace(/\.zip$/i, "");
          list.push({
            id: `pack:${kind}:${pack.name}`,
            kind: "pack",
            label: name,
            sub: t(`${kind === "shader" ? "光影" : "资源包"} · ${active.name}`, `${kind === "shader" ? "Shader" : "Resource pack"} · ${active.name}`),
            keywords: kind === "shader" ? t("shader 光影 iris", "shader 光影 iris") : t("resource pack 资源包 材质", "resource pack 资源包 材质"),
            icon: <ModIcon src={pack.icon ?? packInsights[`${kind}:${pack.name}`]?.iconUrl} seed={pack.name} size={24} glyph={kind === "shader" ? "sun" : "pack"} />,
            primary: { label: t("打开资源包与光影", "Open Packs & shaders"), run: () => store.openManage(active.id, "packs") },
            secondary: { label: t("打开文件夹", "Open folder"), run: () => void store.open({ kind: "contentDir", id: active.id, content: kind }) },
          });
        }
      }
      for (const shot of shots ?? []) {
        const when = shotTime(shot);
        list.push({
          id: `shot:${shot.name}`,
          kind: "shot",
          label: t(`${dayLabel(when)}的截图`, `Screenshot from ${dayLabel(when)}`),
          sub: shot.name,
          keywords: `screenshot 截图 ${shot.name}`,
          icon: <img className="palette-img" src={shotUrl(active.id, shot)} alt="" loading="lazy" />,
          primary: { label: t("查看", "View"), run: () => store.openModal({ kind: "shot", id: active.id, name: shot.name }) },
          secondary: { label: t("用系统应用打开", "Open with system app"), run: () => void store.open({ kind: "screenshot", id: active.id, name: shot.name }) },
          preview: () => <ShotPreview id={active.id} shot={shot} />,
        });
      }
    }

    // Settings, one row each, showing the current value.
    const toggle = (id: string, label: string, sub: string, on: boolean, flip: () => void, keywords: string): Item => ({
      id: `setting:${id}`,
      kind: "setting",
      label,
      sub,
      keywords: `setting 设置 开关 ${keywords}`,
      icon: glyph("settings"),
      value: on ? t("开", "On") : t("关", "Off"),
      primary: { label: on ? t("关闭", "Close") : t("打开", "Open"), run: flip },
      secondary: { label: t("在设置中查看", "Show in Settings"), run: () => store.openSettings(id === "motion" || id === "dev" ? "look" : "game") },
    });
    const memoryText = settings.memoryAuto
      ? t(`自动${s.systemMemory ? ` · 约 ${Math.round(s.systemMemory.recommendedMb / 1024)} GB` : ""}`, `Auto${s.systemMemory ? ` · about ${Math.round(s.systemMemory.recommendedMb / 1024)} GB` : ""}`)
      : `${settings.memoryMb >= 1024 && settings.memoryMb % 1024 === 0 ? `${settings.memoryMb / 1024} GB` : `${settings.memoryMb} MB`}`;
    list.push({
      id: "setting:memory",
      kind: "setting",
      label: t("默认游戏内存", "Default game memory"),
      sub: t("输入「内存 6」可以直接改成 6 GB", "Type \"memory 6\" to set it to 6 GB"),
      keywords: "memory ram 内存 运行内存 neicun",
      icon: glyph("cpu"),
      value: memoryText,
      primary: { label: t("在设置中修改", "Change in Settings"), run: () => store.openSettings("game") },
    });
    list.push({
      id: "setting:concurrency",
      kind: "setting",
      label: t("下载并发数", "Parallel downloads"),
      sub: t("输入「并发 64」可以直接修改", "Type \"threads 64\" to change it"),
      keywords: "download concurrency threads 并发 下载 线程",
      icon: glyph("download"),
      value: String(settings.maxConcurrentDownloads),
      primary: { label: t("在设置中修改", "Change in Settings"), run: () => store.openSettings("download") },
    });
    list.push({
      id: "setting:source",
      kind: "setting",
      label: t("下载源", "Download source"),
      sub: settings.downloadSource === "bmclapi" ? t("回车切换到官方源", "Enter switches to the official source") : t("回车切换到 BMCLAPI 镜像", "Enter switches to the BMCLAPI mirror"),
      keywords: "download source mirror bmclapi 镜像 下载源 官方",
      icon: glyph("globe"),
      value: settings.downloadSource === "bmclapi" ? "BMCLAPI" : t("官方", "Official"),
      primary: {
        label: settings.downloadSource === "bmclapi" ? t("切换到官方源", "Switch to official") : t("切换到 BMCLAPI", "Switch to BMCLAPI"),
        run: () => update({ downloadSource: settings.downloadSource === "bmclapi" ? "official" : "bmclapi" }, t("下载源已切换", "Download source switched")),
      },
      secondary: { label: t("在设置中查看", "Show in Settings"), run: () => store.openSettings("download") },
    });
    list.push({
      id: "setting:java",
      kind: "setting",
      label: "Java",
      sub: t("默认按游戏版本自动匹配", "Matched to the game version by default"),
      keywords: "java jdk jre 运行时",
      icon: glyph("cpu"),
      value: settings.javaPath ? t("指定", "Custom") : t("自动", "Auto"),
      primary: { label: t("在设置中修改", "Change in Settings"), run: () => store.openSettings("game") },
    });
    list.push(toggle("hide", t("游戏启动后隐藏启动器", "Hide the launcher while playing"), t("游戏退出后自动回来", "It comes back when the game exits"), settings.hideOnLaunch, () => update({ hideOnLaunch: !settings.hideOnLaunch }, settings.hideOnLaunch ? t("启动后不再隐藏", "The launcher stays visible while playing") : t("启动后会隐藏启动器", "The launcher hides while playing")), t("hide 隐藏 最小化", "hide 隐藏 最小化")));
    list.push(toggle("chinese", t("新世界使用启动器的语言", "New worlds use the launcher's language"), t("第一次启动前把游戏语言设成和启动器一样", "Sets the game language to match the launcher before the first launch"), settings.autoChinese, () => update({ autoChinese: !settings.autoChinese }, settings.autoChinese ? t("新世界不再自动设置语言", "New worlds no longer get a language set") : t("新世界会使用启动器的语言", "New worlds will use the launcher's language")), t("language chinese 中文 语言", "language chinese 中文 语言")));
    list.push(toggle("motion", t("减少动效", "Reduce motion"), t("关闭过渡和滑动动画", "Turns off transitions and sliding animations"), s.reduceMotion, () => store.setReduceMotion(!s.reduceMotion), t("motion animation 动画 动效", "motion animation 动画 动效")));
    list.push(toggle("dev", t("状态栏显示技术信息", "Technical details in the status bar"), t("下载源、Java、内存和平台", "Download source, Java, memory and platform"), devInfo, () => usePrefs.getState().setDevInfo(!devInfo), t("status bar developer 状态栏 技术", "status bar developer 状态栏 技术")));
    const themes: [Theme, string][] = [
      ["system", t("跟随系统", "System")],
      ["light", t("浅色", "Light")],
      ["dark", t("深色", "Dark")],
    ];
    for (const [theme, label] of themes) {
      list.push({
        id: `setting:theme:${theme}`,
        kind: "setting",
        label: t(`主题：${label}`, `Theme: ${label}`),
        keywords: `theme dark light 主题 外观 ${theme}`,
        icon: glyph("palette"),
        value: settings.theme === theme ? t("当前", "Current") : undefined,
        primary: { label: t("切换", "Switch"), run: () => update({ theme }, t(`已切换到${label}主题`, `Switched to the ${label} theme`)) },
      });
    }
    list.push({ id: "setting:login", kind: "setting", label: t("微软登录 Client ID", "Microsoft sign-in Client ID"), sub: t("使用你自己的 Azure 应用接入正版登录", "Use your own Azure app for Microsoft sign-in"), keywords: "microsoft azure login client 微软 登录 正版", icon: glyph("shield"), primary: { label: t("在设置中修改", "Change in Settings"), run: () => store.openSettings("login") } });

    // Actions.
    const action = (id: string, label: string, icon: IconName, run: () => void, extra: Partial<Item> = {}): Item => ({ id: `action:${id}`, kind: "action", label, icon: glyph(icon), primary: { label: t("执行", "Run"), run }, ...extra });
    list.push(action("create", t("新建世界", "New world"), "plus", () => store.openCreate(), { shortcut: `${MOD_KEY}N`, keywords: "new create instance 新建 创建 实例" }));
    list.push(
      action("import", t("导入整合包或其他启动器的游戏", "Import a modpack or games from other launchers"), "archive", () => store.openModal({ kind: "import" }), {
        sub: t("整合包 · 官方启动器 · HMCL · PCL · Prism", "Modpacks · Official launcher · HMCL · PCL · Prism"),
        keywords: "import modpack mrpack curseforge 整合包 导入 迁移 hmcl pcl prism multimc 官方 .minecraft",
      }),
    );
    list.push(action("storage", t("清理存储空间", "Free up storage"), "disk", () => store.openSettings("storage"), { keywords: "storage disk clean cache 存储 空间 清理 缓存 磁盘" }));
    if (active) {
      const modded = active.loader !== "vanilla";
      list.push(action("manage", t("管理当前世界", "Manage current world"), "settings", () => store.openManage(active.id), { sub: active.name, keywords: "manage 管理 内存 java 加载器" }));
      if (modded) {
        list.push(action("mods", t("检查模组更新", "Check mod updates"), "refresh", () => store.openManage(active.id, "mods"), { sub: active.name, keywords: "mods update 模组 更新" }));
        list.push(action("get-mods", t("下载模组", "Download mods"), "download", () => store.openManage(active.id, "browse", "", "mod"), { sub: "Modrinth", keywords: "download mods modrinth 模组 下载 安装" }));
      }
      list.push(action("get-packs", t("下载资源包", "Download resource packs"), "layers", () => store.openManage(active.id, "browse", "", "resourcepack"), { sub: "Modrinth", keywords: "resource pack texture 资源包 材质 下载" }));
      list.push(action("get-shaders", t("下载光影", "Download shaders"), "sparkle", () => store.openManage(active.id, "browse", "", "shader"), { sub: "Modrinth", keywords: "shader iris optifine 光影 下载" }));
      list.push(action("saves", t("存档与备份", "Saves & backups"), "box", () => store.openManage(active.id, "data"), { sub: active.name, keywords: "worlds saves backup 存档 备份 恢复" }));
      list.push(action("servers", t("服务器列表", "Server list"), "globe", () => store.openManage(active.id, "servers"), { sub: active.name, keywords: "servers multiplayer 服务器 多人 联机 添加" }));
      list.push(action("shots", t("截图墙", "Screenshots"), "image", () => store.openManage(active.id, "shots"), { sub: active.name, keywords: "screenshots gallery 截图 相册 图片 f2" }));
      list.push(action("options", t("游戏选项", "Game options"), "edit", () => store.openManage(active.id, "options"), { sub: t("语言、渲染距离、视场角、帧率", "Language, render distance, FOV, frame rate"), keywords: "options language fov render distance fps 选项 语言 中文 视野 帧率 音量" }));
      list.push(action("logs", t("查看日志", "Show log"), "log", () => void store.openLogs(s.running ?? active.id), { sub: active.name, shortcut: `${MOD_KEY}L`, keywords: "logs console 日志 输出" }));
      list.push(action("game-dir", t("打开游戏目录", "Open game folder"), "folder", () => void store.open({ kind: "gameDir", id: active.id }), { sub: active.name, keywords: "folder directory 目录 文件夹" }));
    }
    list.push(action("recycle", t("回收站", "Recycle bin"), "trash", () => void store.openRecycle(), { keywords: "recycle trash restore 回收站 恢复" }));
    list.push(action("offline", t("添加离线账号", "Add offline account"), "user", () => store.openModal({ kind: "addOffline" }), { keywords: "account offline 账号 离线" }));
    // Rooms: open the multiplayer page on the card that does it.
    const room = useP2p.getState().status;
    if (room?.active && room.code) {
      const code = room.code;
      list.push(action("room-copy", t("复制房间码", "Copy room code"), "copy", () => void store.copy(code, t("房间码", "Room code")), { sub: code, keywords: "p2p room code copy 联机 房间码 复制 好友" }));
    } else if (!room?.active) {
      list.push(action("room-host", t("开房间", "Host a room"), "globe", () => useP2p.getState().focus("host"), { sub: t("把对局域网开放的世界分享给好友", "Share a world opened to LAN with friends"), keywords: "p2p host room lan 联机 开房间 房主 局域网 好友" }));
      list.push(action("room-join", t("加入房间", "Join a room"), "link", () => useP2p.getState().focus("join"), { sub: t("输入好友发来的房间码", "Enter the code a friend sent"), keywords: "p2p join room code 联机 加入房间 房间码 好友" }));
    }
    list.push(action("data-dir", t("打开数据目录", "Open data folder"), "folder", () => void store.open({ kind: "dataDir" }), { keywords: "data folder 数据 目录" }));

    const pages: [string, "home" | "instances" | "multiplayer" | "accounts" | "settings", IconName, string][] = [
      [t("开始", "Home"), "home", "home", "1"],
      [t("世界", "Worlds"), "instances", "layers", "2"],
      [t("联机", "Multiplayer"), "multiplayer", "globe", "3"],
      [t("账号", "Accounts"), "accounts", "user", "4"],
      [t("设置", "Settings"), "settings", "settings", "5"],
    ];
    for (const [label, page, icon, key] of pages) {
      list.push({ id: `page:${page}`, kind: "page", label: t(`前往「${label}」`, `Go to ${label}`), keywords: `go page ${page} 前往 页面`, icon: glyph(icon), shortcut: `${MOD_KEY}${key}`, primary: { label: t("前往", "Go to"), run: () => store.setPage(page) } });
    }
    return list;
  }, [s, devInfo, active, saves, serverLists, shots, content, modIcons, packInsights]);

  // Rows made from the query itself: a setting typed with its value, a
  // version to start a world from, and Modrinth when nothing local fits.
  const quick = useMemo(() => {
    const store = useStore.getState();
    const q = query.trim();
    const out: Item[] = [];
    if (!q) return out;
    const set = (patch: Partial<typeof s.settings>, message: string) =>
      void store.mutate("update_settings", { settings: { ...useStore.getState().settings, ...patch } }, message);

    const memory = q.match(/^(?:内存|默认内存|neicun|nc|memory|mem)\s*(\d+(?:\.\d+)?)\s*(g|gb|m|mb)?$/i);
    if (memory) {
      const n = Number(memory[1]);
      const unit = memory[2]?.toLowerCase();
      const mb = Math.round(unit?.startsWith("m") || (!unit && n > 64) ? n : n * 1024);
      const ok = Number.isFinite(mb) && mb >= 512 && mb <= 32768;
      out.push({
        id: "quick:memory",
        kind: "setting",
        label: ok ? t(`把默认内存设为 ${mb % 1024 === 0 ? `${mb / 1024} GB` : `${mb} MB`}`, `Set default memory to ${mb % 1024 === 0 ? `${mb / 1024} GB` : `${mb} MB`}`) : t("内存需要在 512 MB 到 32 GB 之间", "Memory must be between 512 MB and 32 GB"),
        sub: t("改为手动内存；世界单独设置的内存仍然优先", "Switches to manual memory; a world's own setting still wins"),
        icon: glyph("cpu"),
        primary: { label: ok ? t("应用", "Apply") : t("打开设置", "Open Settings"), run: () => (ok ? set({ memoryAuto: false, memoryMb: mb }, t(`默认内存已设为 ${mb} MB`, `Default memory set to ${mb} MB`)) : store.openSettings("game")) },
      });
    }
    const threads = q.match(/^(?:并发|bingfa|bf|concurrency|threads)\s*(\d+)$/i);
    if (threads) {
      const n = Number(threads[1]);
      const ok = n >= 1 && n <= 64;
      out.push({
        id: "quick:concurrency",
        kind: "setting",
        label: ok ? t(`把下载并发设为 ${n}`, `Set parallel downloads to ${n}`) : t("并发数需要在 1 到 64 之间", "Must be between 1 and 64"),
        icon: glyph("download"),
        primary: { label: ok ? t("应用", "Apply") : t("打开设置", "Open Settings"), run: () => (ok ? set({ maxConcurrentDownloads: n }, t(`下载并发已设为 ${n}`, `Parallel downloads set to ${n}`)) : store.openSettings("download")) },
      });
    }
    const theme = q.match(/^(?:主题|theme|zhuti)\s*(深色|浅色|跟随系统|dark|light|system)$/i);
    if (theme) {
      const value: Theme = /深色|dark/i.test(theme[1]) ? "dark" : /浅色|light/i.test(theme[1]) ? "light" : "system";
      out.push({ id: "quick:theme", kind: "setting", label: t(`切换到${value === "dark" ? "深色" : value === "light" ? "浅色" : "跟随系统"}主题`, `Switch to the ${value === "dark" ? "dark" : value === "light" ? "light" : "system"} theme`), icon: glyph("palette"), primary: { label: t("应用", "Apply"), run: () => set({ theme: value }, t("主题已切换", "Theme switched")) } });
    }

    if (/^\d+\.\d+/.test(q) || /^\d{2}w\d/.test(q)) {
      const hits = s.versions.filter((v: VersionSummary) => v.id.startsWith(q)).slice(0, 3);
      for (const v of hits) {
        const name = updateName(v.id);
        out.push({
          id: `version:${v.id}`,
          kind: "version",
          label: t(`用 ${v.id} 新建世界`, `New world on ${v.id}`),
          sub: [name, v.versionType === "release" ? t("正式版", "Release") : t("快照", "Snapshot")].filter(Boolean).join(" · "),
          icon: glyph("plus"),
          primary: { label: t("新建世界", "New world"), run: () => store.openCreate(v.id) },
        });
      }
    }

    if (active && !/^[@#/>]/.test(q)) {
      const kinds: ["mod" | "resourcepack" | "shader", string][] = [
        ...(active.loader !== "vanilla" ? ([["mod", t("模组", "Mods")]] as ["mod", string][]) : []),
        ["resourcepack", t("资源包", "Resource packs")],
        ["shader", t("光影", "Shaders")],
      ];
      for (const [kind, label] of kinds) {
        out.push({
          id: `online:${kind}`,
          kind: "online",
          label: t(`在 Modrinth 搜索${label}「${q}」`, `Search Modrinth ${label} for "${q}"`),
          sub: t(`为「${active.name}」下载`, `For "${active.name}"`),
          icon: glyph("search"),
          primary: { label: t("搜索", "Search"), run: () => store.openManage(active.id, "browse", q, kind) },
        });
      }
    }
    return out;
  }, [query, s, active]);

  return { items, quick };
}

// ----- ranking ---------------------------------------------------------------

interface Row {
  item: Item;
  marks: number[];
}

interface Section {
  title: string;
  rows: Row[];
  more: number;
}

const PER_GROUP = 6;

function rank(items: Item[], quick: Item[], query: string, scope: Scope): Section[] {
  const inScope = (item: Item) => scope.kinds.length === 0 || scope.kinds.includes(item.kind);
  const words = queryWords(query);
  const recent = readRecent();
  const byId = new Map(items.map((i) => [i.id, i]));

  if (words.length === 0) {
    if (scope.id !== "all") return group(items.filter(inScope).map((item) => ({ item, marks: [] })), Infinity);
    // Nothing typed: what you are most likely here for, then everything by kind.
    const hero = items.find((i) => i.hero);
    const first: Row[] = [];
    if (hero) first.push({ item: hero, marks: [] });
    const lastSave = items.find((i) => i.kind === "save" && hero && i.id.startsWith(`save:${hero.id.slice(6)}:`));
    if (lastSave) first.push({ item: lastSave, marks: [] });
    const recents = recent
      .map((id) => byId.get(id))
      .filter((i): i is Item => Boolean(i) && i !== hero && i !== lastSave)
      .slice(0, 5)
      .map((item) => ({ item, marks: [] }));
    const sections: Section[] = [{ title: t("继续", "Continue"), rows: first, more: 0 }];
    if (recents.length) sections.push({ title: t("最近使用", "Recent"), rows: recents, more: 0 });
    const shownIds = new Set([...first, ...recents].map((r) => r.item.id));
    const rest = items.filter((i) => !shownIds.has(i.id) && (i.kind === "world" || i.kind === "action" || i.kind === "page"));
    sections.push(...group(rest.map((item) => ({ item, marks: [] })), Infinity));
    return sections.filter((section) => section.rows.length > 0);
  }

  // "1.20" means a Minecraft version, not the end of an IP address.
  const versionLike = /^(1\.\d{1,2}(\.\d{1,2})?|2\d\.\d{1,2}(\.\d{1,2})?)$/.test(query.trim());
  const scored: (Row & { score: number })[] = [];
  for (const item of [...quick, ...items]) {
    if (!inScope(item)) continue;
    if (versionLike && item.kind === "server") continue;
    if (item.kind === "online" || item.kind === "version") {
      scored.push({ item, marks: [], score: item.kind === "online" ? -1 : 150 });
      continue;
    }
    if (item.id.startsWith("quick:")) {
      scored.push({ item, marks: [], score: 200 });
      continue;
    }
    const match = matchItem(item.label, [item.sub ?? "", item.keywords ?? ""], words);
    if (!match) continue;
    const boost = (recent.includes(item.id) ? 12 : 0) + KIND_WEIGHT[item.kind] + (item.hero ? 4 : 0);
    scored.push({ item, marks: match.marks, score: match.score + boost });
  }
  scored.sort((a, b) => b.score - a.score);
  // Modrinth rows only lead when nothing local matched.
  const local = scored.filter((r) => r.item.kind !== "online");
  const online = scored.filter((r) => r.item.kind === "online");
  return group([...local, ...online], scope.id === "all" ? PER_GROUP : Infinity, true);
}

/** Rows into sections by kind, keeping the order rows arrive in (so the
    best match's group comes first). */
function group(rows: Row[], limit: number, keepOrder = false): Section[] {
  const sections = new Map<string, Section>();
  const ordered = keepOrder ? rows : [...rows].sort((a, b) => KIND_WEIGHT[b.item.kind] - KIND_WEIGHT[a.item.kind]);
  for (const row of ordered) {
    const title = row.item.id.startsWith("quick:") ? t("直接修改", "Change directly") : GROUP[row.item.kind];
    let section = sections.get(title);
    if (!section) sections.set(title, (section = { title, rows: [], more: 0 }));
    if (section.rows.length < limit) section.rows.push(row);
    else section.more++;
  }
  return [...sections.values()];
}

// ----- the palette -------------------------------------------------------------

export function CommandPalette() {
  const close = useStore((s) => s.closeModal);
  const [raw, setRaw] = useState("");
  const [scopeId, setScopeId] = useState("all");
  const [active, setActive] = useState(0);
  const rows = useRef<(HTMLButtonElement | null)[]>([]);
  const input = useRef<HTMLInputElement>(null);

  // A leading @ # / > picks a scope and is not part of the search.
  const prefixScope = SCOPES.find((sc) => sc.prefix && raw.startsWith(sc.prefix));
  const scope = prefixScope ?? SCOPES.find((sc) => sc.id === scopeId) ?? SCOPES[0];
  const query = prefixScope ? raw.slice(prefixScope.prefix!.length) : raw;

  const { items, quick } = useItems(query);
  const sections = useMemo(() => rank(items, quick, query, scope), [items, quick, query, scope]);
  const flat = useMemo(() => sections.flatMap((section) => section.rows), [sections]);
  const current = flat[Math.min(active, flat.length - 1)] ?? null;

  useEffect(() => {
    setActive(0);
  }, [raw, scopeId]);
  // A version typed in the search needs the version list; fetch it once.
  useEffect(() => {
    const st = useStore.getState();
    if (/^\d+\.\d+|^\d{2}w\d/.test(query.trim()) && !st.versionsLoaded && !st.versionsLoading) st.loadVersions(false);
  }, [query]);
  // Newer engines return a promise from scrollIntoView; an effect must not.
  useEffect(() => {
    rows.current[active]?.scrollIntoView({ block: "nearest" });
  }, [active]);

  const run = (row: Row | null, second = false) => {
    if (!row) return;
    const action = second ? row.item.secondary : row.item.primary;
    if (!action) return;
    remember(row.item.id);
    close();
    action.run();
  };

  const cycleScope = (step: number) => {
    const at = SCOPES.findIndex((sc) => sc.id === scope.id);
    const next = SCOPES[(at + step + SCOPES.length) % SCOPES.length];
    setScopeId(next.id);
    if (prefixScope) setRaw(query);
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    // While an input method is composing (pinyin, kana), its keys are its own.
    if (e.nativeEvent.isComposing || e.keyCode === 229) return;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (flat.length === 0) return;
      const step = e.key === "ArrowDown" ? 1 : -1;
      setActive((i) => (i + step + flat.length) % flat.length);
    } else if (e.key === "Enter") {
      e.preventDefault();
      run(current, e.metaKey || e.ctrlKey);
    } else if (e.key === "Tab") {
      e.preventDefault();
      cycleScope(e.shiftKey ? -1 : 1);
    } else if (e.key === "Escape" && (raw || scope.id !== "all")) {
      // First Esc clears the search; the next one closes.
      e.preventDefault();
      e.stopPropagation();
      setRaw("");
      setScopeId("all");
    } else if (e.key === "Backspace" && !raw && scope.id !== "all") {
      setScopeId("all");
    }
  };

  let index = -1;
  return (
    <motion.div className="palette-root" initial="initial" animate="animate" exit="exit">
      <motion.div
        className="palette-scrim"
        onClick={close}
        variants={{ initial: { opacity: 0 }, animate: { opacity: 1, transition: { duration: 0.16 } }, exit: { opacity: 0, transition: { duration: 0.12 } } }}
      />
      <motion.div
        className="palette"
        role="dialog"
        aria-modal
        aria-label={t("搜索与命令", "Search & commands")}
        variants={{
          initial: { opacity: 0, y: -8, scale: 0.99 },
          animate: { opacity: 1, y: 0, scale: 1, transition: { duration: 0.2, ease: [0.2, 0.8, 0.2, 1] } },
          exit: { opacity: 0, y: -6, transition: { duration: 0.12, ease: [0.4, 0, 1, 1] } },
        }}
      >
        <div className="palette-input-row">
          <Icon name="search" size={17} />
          {scope.id !== "all" && <span className="palette-scope-pill">{scope.label}</span>}
          <input
            ref={input}
            className="palette-input"
            autoFocus
            placeholder={scope.id === "all" ? t("搜索世界、存档、服务器、模组、设置…（支持拼音）", "Search worlds, saves, servers, mods, settings…") : t(`在「${scope.label}」中搜索`, `Search in ${scope.label}`)}
            value={raw}
            onChange={(e) => setRaw(e.target.value)}
            onKeyDown={onKeyDown}
            role="combobox"
            aria-expanded
            aria-controls="palette-list"
            aria-activedescendant={current ? `palette-${current.item.id}` : undefined}
          />
          <span className="palette-key">esc</span>
        </div>
        <div className="palette-scopes" role="tablist" aria-label={t("搜索范围", "Search scope")}>
          {SCOPES.map((sc) => (
            <button
              key={sc.id}
              type="button"
              role="tab"
              aria-selected={sc.id === scope.id}
              className="palette-scope"
              tabIndex={-1}
              onClick={() => {
                setScopeId(sc.id);
                if (prefixScope) setRaw(query);
                input.current?.focus();
              }}
            >
              {sc.prefix && <span className="palette-scope-prefix">{sc.prefix}</span>}
              {sc.label}
            </button>
          ))}
        </div>
        <div className="palette-body">
          <div className="palette-list" id="palette-list" role="listbox">
            {flat.length === 0 ? (
              <div className="palette-empty">
                {t("没有找到「", "Nothing found for \"")}{query.trim()}{t("」", "\"")}
                <span>{t("试试拼音首字母，或按 Tab 换个范围", "Try other words, or press Tab to change the scope")}</span>
              </div>
            ) : (
              sections.map((section) => (
                <div key={section.title} role="group" aria-label={section.title}>
                  <div className="palette-group">
                    {section.title}
                    {section.more > 0 && <span className="palette-more">{t("还有 ", "")}{section.more}{t(" 项 · Tab 切换范围查看", " more · press Tab to change the scope")}</span>}
                  </div>
                  {section.rows.map((row) => {
                    index++;
                    const at = index;
                    const { item } = row;
                    return (
                      <button
                        key={item.id}
                        ref={(el) => {
                          rows.current[at] = el;
                        }}
                        id={`palette-${item.id}`}
                        type="button"
                        role="option"
                        className={`palette-item${item.hero && !query.trim() ? " hero" : ""}`}
                        aria-selected={at === active}
                        onMouseMove={() => at !== active && setActive(at)}
                        onClick={(e) => run(row, e.metaKey || e.ctrlKey)}
                      >
                        <span className="palette-icon">{item.icon}</span>
                        <span className="palette-text">
                          <span className="palette-label truncate">
                            <Highlight text={item.label} marks={row.marks} />
                          </span>
                          {item.sub && <span className="palette-hint truncate">{item.sub}</span>}
                        </span>
                        {item.value && <span className="palette-value">{item.value}</span>}
                        {item.shortcut && <span className="palette-key">{item.shortcut}</span>}
                        {at === active && <span className="palette-enter">{item.primary.label} ↵</span>}
                      </button>
                    );
                  })}
                </div>
              ))
            )}
          </div>
          <aside className="palette-preview" aria-live="polite">
            {current?.item.preview ? current.item.preview() : current ? <GenericPreview item={current.item} /> : null}
          </aside>
        </div>
        <div className="palette-foot">
          {current && (
            <span>
              <span className="palette-key">↵</span> {current.item.primary.label}
            </span>
          )}
          {current?.item.secondary && (
            <span>
              <span className="palette-key">{MOD_KEY}↵</span> {current.item.secondary.label}
            </span>
          )}
          <span className="spacer" />
          <span>
            <span className="palette-key">Tab</span>{t(" 范围", " scope")}
          </span>
          <span>
            <span className="palette-key">@ # / &gt;</span>{t(" 服务器 · 模组 · 设置 · 操作", " servers · mods · settings · actions")}
          </span>
        </div>
      </motion.div>
    </motion.div>
  );
}

function Highlight({ text, marks }: { text: string; marks: number[] }) {
  if (marks.length === 0) return <>{text}</>;
  const chars = [...text];
  const on = new Set(marks);
  const parts: ReactNode[] = [];
  let i = 0;
  while (i < chars.length) {
    const hit = on.has(i);
    let j = i;
    while (j < chars.length && on.has(j) === hit) j++;
    const piece = chars.slice(i, j).join("");
    parts.push(hit ? <mark key={i} className="palette-hit">{piece}</mark> : piece);
    i = j;
  }
  return <>{parts}</>;
}

// ----- previews -----------------------------------------------------------------

function WorldPreview({ instance, saves }: { instance: Instance; saves?: WorldList }) {
  const biome = useBiome(instance.id);
  const state = useStore((s) => instanceState(instance, s));
  const label: Record<typeof state, string> = { running: t("运行中", "Running"), starting: t("启动中", "Starting"), installing: t("安装中", "Installing"), queued: t("排队中", "Queued"), ready: t("就绪", "Ready"), notInstalled: t("未安装", "Not installed") };
  return (
    <div className="pv">
      <div className="pv-scene" style={biomeStyle(biome)}>
        <Landscape seed={instance.id} unit={PX} cols={24} ground={[1, 2]} sky={5} treesFrom={3} treeChance={0.5} far={[2, 4]} biome={biome} />
        <SceneCover id={instance.id} />
        <span className="pv-biome">{biome.name}</span>
      </div>
      <div className="pv-title">{instance.name}</div>
      <div className="pv-sub">
        {instance.versionId}
        {updateName(instance.versionId) ? ` · ${updateName(instance.versionId)}` : ""} · {loaderLabel(instance.loader)}
      </div>
      <dl className="pv-facts">
        <dt>{t("状态", "Status")}</dt>
        <dd>{label[state]}</dd>
        <dt>{t("上次游玩", "Last played")}</dt>
        <dd>{instance.lastLaunchedAt ? formatRelative(instance.lastLaunchedAt) : t("尚未游玩", "Never played")}</dd>
        <dt>{t("游玩时长", "Play time")}</dt>
        <dd>{instance.playSeconds >= 60 ? formatPlayTime(instance.playSeconds) : t("不到 1 分钟", "Under a minute")}</dd>
        <dt>{t("存档", "Saves")}</dt>
        <dd>{saves ? t(`${saves.worlds.length} 个`, `${saves.worlds.length}`) : instance.installed ? "…" : "—"}</dd>
      </dl>
    </div>
  );
}

function ServerPreview({ server, instance }: { server: ServerEntry; instance: Instance }) {
  const ping = usePing(server.address);
  useEffect(() => {
    useServers.getState().ping([server.address]);
  }, [server.address]);
  const status = ping?.status;
  return (
    <div className="pv">
      <div className="pv-figure">{status?.favicon || server.icon ? <img className="pv-save pixelated" src={status?.favicon ?? server.icon!} alt="" /> : <Identicon seed={server.address} size={56} hero />}</div>
      <div className="pv-title">{server.name || server.address}</div>
      <div className="pv-sub mono">{server.address}</div>
      <div className="pv-motd">{status ? <Motd spans={status.motd} /> : ping?.loading ? t("正在查询…", "Checking…") : ping?.error ? t("无法连接到服务器", "Can't reach the server") : ""}</div>
      <dl className="pv-facts">
        <dt>{t("延迟", "Ping")}</dt>
        <dd className="pv-signal">
          <Signal ping={ping} />
          {status ? `${status.latencyMs} ms` : "—"}
        </dd>
        <dt>{t("在线", "Online")}</dt>
        <dd>{status ? `${status.online} / ${status.max}` : "—"}</dd>
        <dt>{t("所在世界", "World")}</dt>
        <dd>{instance.name}</dd>
      </dl>
    </div>
  );
}

function ShotPreview({ id, shot }: { id: string; shot: Screenshot }) {
  return (
    <div className="pv">
      <img className="pv-shot" src={shotUrl(id, shot)} alt="" />
      <div className="pv-title">{dayLabel(shotTime(shot))}</div>
      <div className="pv-sub mono">{shot.name}</div>
      <dl className="pv-facts">
        <dt>{t("大小", "Size")}</dt>
        <dd>{formatBytes(shot.bytes)}</dd>
      </dl>
    </div>
  );
}

function GenericPreview({ item }: { item: Item }) {
  return (
    <div className="pv">
      <div className="pv-figure pv-glyph">{item.icon}</div>
      <div className="pv-title">{item.label}</div>
      {item.sub && <div className="pv-sub">{item.sub}</div>}
      <dl className="pv-facts">
        <dt>{t("回车", "Enter")}</dt>
        <dd>{item.primary.label}</dd>
        {item.secondary && (
          <>
            <dt>{MOD_KEY}{t("+回车", "+Enter")}</dt>
            <dd>{item.secondary.label}</dd>
          </>
        )}
        {item.value && (
          <>
            <dt>{t("当前", "Current")}</dt>
            <dd>{item.value}</dd>
          </>
        )}
        {item.shortcut && (
          <>
            <dt>{t("快捷键", "Shortcut")}</dt>
            <dd>{item.shortcut}</dd>
          </>
        )}
      </dl>
    </div>
  );
}
