import { useEffect, useState } from "react";
import { AnimatePresence, motion } from "motion/react";

import { backend, type WindowAction } from "../api/backend";
import { Icon } from "../components/Icon";
import { BrandMark, PixelIcon, PX, Terrain, type PixelIconName } from "../components/Pixel";
import { PlayerFace } from "../components/PlayerFace";
import { Button, Identicon, Kbd, ProgressBar } from "../components/ui";
import { formatBytes, formatSpeed, loaderLabel, progressRatio, stageLabel } from "../lib/format";
import { biomeStyle, useBiome } from "../lib/biome";
import { glide } from "../lib/motion";
import { LEVELS, roomLive, useP2p } from "../lib/p2p";
import { MOD_KEY } from "../lib/platform";
import { usePrefs } from "../lib/prefs";
import { instanceState, useActiveAccount, useActiveInstance, useAutoMemory, usePick, useStore, type Page } from "../store";
import { t } from "../lib/i18n";

const NAV: { page: Page; label: string; icon: PixelIconName; key: string }[] = [
  { page: "home", label: t("开始", "Home"), icon: "play", key: "1" },
  { page: "instances", label: t("世界", "Worlds"), icon: "block", key: "2" },
  { page: "multiplayer", label: t("联机", "Multiplayer"), icon: "signal", key: "3" },
  { page: "accounts", label: t("账号", "Accounts"), icon: "player", key: "4" },
  { page: "settings", label: t("设置", "Settings"), icon: "gear", key: "5" },
];

export function Sidebar() {
  const page = useStore((s) => s.page);
  const setPage = useStore((s) => s.setPage);
  const account = useActiveAccount();
  const playing = useStore((s) => s.runningIds.length > 0);
  const room = useRoomLevel();
  // What is going on behind each page: a game running, a room open.
  const badge = (target: Page) =>
    target === "home" && playing ? (
      <span className="nav-dot level-good" title={t("游戏运行中", "Game running")} />
    ) : target === "multiplayer" && room ? (
      <span className={`nav-dot level-${room}${LEVELS[room].pulse ? " pulse" : ""}`} title={LEVELS[room].label} />
    ) : null;

  return (
    <aside className="sidebar">
      <div className="sidebar-drag" data-tauri-drag-region />
      <div className="brand" data-tauri-drag-region>
        <BrandMark size={32} />
        {/* The version lives in the status bar, with the technical details. */}
        <span className="brand-name" data-tauri-drag-region>
          {t("草岛", "Islet")}
        </span>
      </div>
      <button type="button" className="sidebar-search" title={`${t("搜索与命令", "Search")} ${MOD_KEY}K`} onClick={() => useStore.getState().openModal({ kind: "palette" })}>
        <Icon name="search" size={14} />
        <span>{t("搜索与命令", "Search")}</span>
        <Kbd>{MOD_KEY}K</Kbd>
      </button>
      <nav className="nav">
        {NAV.map((item) => {
          const active = item.page === page;
          return (
            <button key={item.page} type="button" className="nav-item" title={`${item.label} ${MOD_KEY}${item.key}`} aria-current={active ? "page" : undefined} onClick={() => setPage(item.page)}>
              {active && (
                <motion.span layoutId="nav-highlight" className="nav-highlight" transition={glide}>
                  <span className="nav-bar" />
                </motion.span>
              )}
              <PixelIcon name={item.icon} size={20} />
              <span className="nav-label">{item.label}</span>
              {badge(item.page)}
              <Kbd>
                {MOD_KEY}
                {item.key}
              </Kbd>
            </button>
          );
        })}
      </nav>

      <div className="sidebar-foot">
        <AnimatePresence initial={false}>{page !== "home" && <CurrentWorld key="now" />}</AnimatePresence>
        <button type="button" className="sidebar-account" title={account ? account.username : t("添加账号", "Add an account")} onClick={() => setPage("accounts")}>
        {account ? (
          <PlayerFace account={account} size={32} />
        ) : (
          <span className="identicon hero" style={{ width: 34, height: 34, borderRadius: "var(--radius-lg)" }}>
            <Icon name="plus" size={15} />
          </span>
        )}
        <div className="col sidebar-account-text" style={{ flex: 1, gap: 1 }}>
          <span className="sidebar-account-name truncate">{account ? account.username : t("还没有账号", "No account yet")}</span>
          <span className="sidebar-account-kind">{account ? (account.provider === "microsoft" ? t("微软账号", "Microsoft account") : t("离线账号", "Offline account")) : t("点击添加账号", "Click to add one")}</span>
        </div>
        <Icon name="chevronRight" size={14} />
      </button>
      </div>
    </aside>
  );
}

/** The open room's health, or null when there is none. */
function useRoomLevel() {
  return useP2p((s) => (roomLive(s) ? (s.status?.active ? (s.status.health?.level ?? "starting") : "starting") : null));
}

/** The room in the status bar: its health and how many are in it. */
function RoomSegment() {
  const level = useRoomLevel();
  const people = useP2p((s) => (s.status?.active ? (s.status.health?.players.length ?? 0) + 1 : 0));
  const role = useP2p((s) => s.status?.role);
  const setPage = useStore((s) => s.setPage);
  if (!level) return null;
  return (
    <button type="button" className="status-seg" title={t("打开联机页", "Open Multiplayer")} onClick={() => setPage("multiplayer")}>
      <span className={`room-dot level-${level}${LEVELS[level].pulse ? " pulse" : ""}`} />
      <span>{role === "guest" ? t("已加入房间", "In a room") : t("房间", "Room")}</span>
      <span className="dim">{LEVELS[level].label}</span>
      {people > 0 && <span className="mono dim">{t(`${people} 人`, `${people} in`)}</span>}
    </button>
  );
}

/** The current world on its strip of ground, startable from any page but
    home, where the stage already carries the start button. */
function CurrentWorld() {
  const s = usePick((s) => ({ install: s.install, installQueue: s.installQueue, running: s.running,
    runningIds: s.runningIds, launchPending: s.launchPending, setPage: s.setPage, primaryAction: s.primaryAction, stopping: s.stopping }));
  const instance = useActiveInstance();
  const biome = useBiome(instance?.id ?? "");
  if (!instance) return null;
  const state = instanceState(instance, s);
  const install = s.install?.instanceId === instance.id ? s.install : null;
  const ratio = install ? progressRatio(install.done, install.total, install.bytes, install.totalBytes) : null;
  const stopping = s.stopping.includes(instance.id);
  const meta =
    state === "running"
      ? stopping ? t("正在关闭…", "Closing…") : t("运行中", "Running")
      : state === "starting"
        ? t("正在启动…", "Starting…")
        : state === "installing"
          ? t(`正在安装${ratio == null ? "…" : ` ${Math.round(ratio * 100)}%`}`, `Installing${ratio == null ? "…" : ` ${Math.round(ratio * 100)}%`}`)
          : `${instance.versionId} · ${loaderLabel(instance.loader)}`;
  const action =
    state === "running"
      ? { icon: "stop" as const, label: stopping ? t("强制结束", "Force quit") : t("停止游戏", "Stop game") }
      : state === "ready"
        ? { icon: "play" as const, label: t("开始游戏", "Play") }
        : state === "notInstalled"
          ? { icon: "download" as const, label: t("安装并开始", "Install and play") }
          : state === "queued"
            ? { icon: "close" as const, label: t("取消排队", "Leave queue") }
            : null;
  return (
    <motion.div
      className="now"
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0, transition: { duration: 0.22, ease: [0.2, 0.8, 0.2, 1] } }}
      exit={{ opacity: 0, y: 8, transition: { duration: 0.14 } }}
    >
      <div className="now-land" style={biomeStyle(biome)}>
        <Terrain seed={instance.id} unit={PX} cols={20} minHeight={1} maxHeight={2} sky={0} trees={false} strata={biome.strata} />
      </div>
      <button type="button" className="now-main" title={instance.name} onClick={() => s.setPage("home")}>
        <Identicon seed={instance.id} size={32} hero />
        <span className="col" style={{ flex: 1, minWidth: 0, gap: 1 }}>
          <span className="now-label">{t("当前世界", "Current world")}</span>
          <span className="now-name truncate">{instance.name}</span>
          <span className={`now-meta truncate${state === "running" || state === "installing" ? " live" : ""}`}>{meta}</span>
        </span>
      </button>
      {action ? (
        <button type="button" className={`now-go${state === "running" ? " stop" : ""}`} aria-label={action.label} title={action.label} onClick={() => s.primaryAction(instance.id)}>
          <Icon name={action.icon} size={15} />
        </button>
      ) : (
        <span className="now-go busy">
          <span className="spinner" />
        </span>
      )}
      {install && (
        <div className="now-progress">
          <ProgressBar ratio={ratio} height={3} />
        </div>
      )}
    </motion.div>
  );
}

/** Windows has no overlay title bar, so the window drops its own and this
    strip of frame above the page takes its place: drag it to move the window,
    double-click it to maximize. It sits above dialogs, like a native title
    bar, so the window can still be moved or closed while one is open. */
export function WindowBar() {
  const [maximized, setMaximized] = useState(false);
  useEffect(() => {
    const off = backend().onMaximized(setMaximized);
    return () => void off.then((unlisten) => unlisten());
  }, []);
  const buttons: { action: WindowAction; label: string; icon: PixelIconName }[] = [
    { action: "minimize", label: t("最小化", "Minimize"), icon: "winMin" },
    maximized
      ? { action: "toggleMaximize", label: t("还原", "Restore"), icon: "winRestore" }
      : { action: "toggleMaximize", label: t("最大化", "Maximize"), icon: "winMax" },
    { action: "close", label: t("关闭", "Close"), icon: "winClose" },
  ];
  return (
    <div className="window-bar" data-tauri-drag-region>
      {buttons.map((button) => (
        <button
          key={button.action}
          type="button"
          className={`window-btn${button.action === "close" ? " close" : ""}`}
          title={button.label}
          aria-label={button.label}
          onClick={() => backend().windowAction(button.action)}
        >
          <PixelIcon name={button.icon} size={20} />
        </button>
      ))}
    </div>
  );
}

export function Toolbar({ title, meta, children }: { title: React.ReactNode; meta?: React.ReactNode; children?: React.ReactNode }) {
  return (
    <header className="toolbar" data-tauri-drag-region>
      <div className="toolbar-heading" data-tauri-drag-region>
        <span className="toolbar-title" data-tauri-drag-region>
          {title}
        </span>
        {meta && (
          <span className="toolbar-meta" data-tauri-drag-region>
            {meta}
          </span>
        )}
      </div>
      <div className="spacer" data-tauri-drag-region />
      <div className="row toolbar-actions">{children}</div>
    </header>
  );
}

/** Footer: the running task on the left. The environment (source, Java,
    memory, platform) is for people who want it: a tooltip by default, the
    full row when "technical details" is on in settings. */
/** Everything in progress, each with its own way to stop or look. */
function TaskPanel({ onClose }: { onClose: () => void }) {
  const s = usePick((s) => ({
    install: s.install,
    installQueue: s.installQueue,
    unqueueInstall: s.unqueueInstall,
    fileBusy: s.fileBusy,
    operationTask: s.operationTask,
    runningIds: s.runningIds,
    launchPending: s.launchPending,
    instances: s.instances,
    cancelInstall: s.cancelInstall,
    primaryAction: s.primaryAction,
    openLogs: s.openLogs,
    run: s.run,
    stopping: s.stopping,
  }));
  const nameOf = (id: string) => s.instances.find((i) => i.id === id)?.name ?? t("世界", "Worlds");
  const ratio = s.install ? progressRatio(s.install.done, s.install.total, s.install.bytes, s.install.totalBytes) : null;
  return (
    <div className="task-panel" role="dialog" aria-label={t("任务", "Tasks")} onKeyDown={(e) => e.key === "Escape" && onClose()}>
      {s.install && (
        <div className="task-row">
          <span className="status tone-accent pulse" />
          <div className="col" style={{ flex: 1, minWidth: 0, gap: 4 }}>
            <span className="truncate">
              {t("安装 ", "Install ")}{nameOf(s.install.instanceId)} · {stageLabel(s.install.stage)}
            </span>
            <ProgressBar ratio={ratio} height={4} />
          </div>
          <span className="mono">{ratio == null ? "…" : `${Math.round(ratio * 100)}%`}</span>
          <Button size="sm" variant="ghost" onClick={s.cancelInstall}>
            {t("取消", "Cancel")}
          </Button>
        </div>
      )}
      {s.installQueue.map((queued, index) => (
        <div key={queued.id} className="task-row">
          <span className="status tone-neutral" />
          <span className="truncate" style={{ flex: 1 }}>
            {t(`等待下载 ${nameOf(queued.id)}`, `Waiting to download ${nameOf(queued.id)}`)}
            {queued.launch && <span className="faint">{t(" · 下载完开始游戏", " · plays when ready")}</span>}
          </span>
          <span className="mono faint">#{index + 1}</span>
          <Button size="sm" variant="ghost" onClick={() => s.unqueueInstall(queued.id)}>
            {t("移出", "Remove")}
          </Button>
        </div>
      ))}
      {s.fileBusy && (
        <div className="task-row">
          <span className="spinner" />
          <span style={{ flex: 1 }}>{t("正在处理文件或下载内容…", "Working on files or downloading content…")}</span>
          {s.operationTask && (
            <Button size="sm" variant="ghost" onClick={() => void s.run("cancel_task", { taskId: s.operationTask })}>
              {t("取消", "Cancel")}
            </Button>
          )}
        </div>
      )}
      {s.launchPending && (
        <div className="task-row">
          <span className="status tone-warn pulse" />
          <span className="truncate" style={{ flex: 1 }}>
            {t("正在启动 ", "Starting ")}{nameOf(s.launchPending)}
          </span>
        </div>
      )}
      {s.runningIds.map((id) => (
        <div key={id} className="task-row">
          <span className="status tone-accent" />
          <span className="truncate" style={{ flex: 1 }}>
            {nameOf(id)}{s.stopping.includes(id) ? t(" 正在关闭…", " closing…") : t(" 运行中", " running")}
          </span>
          <Button size="sm" variant="ghost" icon="log" onClick={() => void s.openLogs(id)}>
            {t("日志", "Log")}
          </Button>
          <Button size="sm" variant="danger-ghost" icon="stop" onClick={() => s.primaryAction(id)}>
            {s.stopping.includes(id) ? t("强制结束", "Force quit") : t("停止", "Stop")}
          </Button>
        </div>
      ))}
    </div>
  );
}

export function StatusBar() {
  const install = useStore((s) => s.install);
  const running = useStore((s) => s.running);
  const runningCount = useStore((s) => s.runningIds.length);
  const launchPending = useStore((s) => s.launchPending);
  const queued = useStore((s) => s.installQueue.length);
  const fileBusy = useStore((s) => s.fileBusy);
  const instances = useStore((s) => s.instances);
  const settings = useStore((s) => s.settings);
  const appInfo = useStore((s) => s.appInfo);
  const setPage = useStore((s) => s.setPage);
  const openLogs = useStore((s) => s.openLogs);
  const update = useStore((s) => s.update);
  const quitScheduled = useStore((s) => s.quitScheduled);
  const cancelScheduledQuit = useStore((s) => s.cancelScheduledQuit);
  const [tasksOpen, setTasksOpen] = useState(false);
  const openUrl = useStore((s) => s.openUrl);
  const devInfo = usePrefs((s) => s.devInfo);

  const nameOf = (id: string | null) => instances.find((i) => i.id === id)?.name ?? "";
  let task: React.ReactNode;
  if (install) {
    const ratio = progressRatio(install.done, install.total, install.bytes, install.totalBytes);
    task = (
      <div className="status-seg">
        <span className="status tone-accent pulse">{stageLabel(install.stage)}</span>
        <div className="status-progress">
          <ProgressBar ratio={ratio} height={4} />
        </div>
        <span className="mono">{ratio == null ? "…" : `${Math.round(ratio * 100)}%`}</span>
        {install.totalBytes > 0 && (
          <span className="mono dim">
            {formatBytes(install.bytes)}/{formatBytes(install.totalBytes)}
          </span>
        )}
        {install.speed != null && install.speed > 1 && <span className="mono dim">{formatSpeed(install.speed)}</span>}
      </div>
    );
  } else if (running || launchPending) {
    const id = running ?? launchPending;
    task = (
      <button type="button" className="status-seg" onClick={() => id && void openLogs(id)}>
        <span className={`status ${running ? "tone-accent" : "tone-warn"} pulse`}>{running ? t("运行中", "Running") : t("启动中", "Starting")}</span>
        <span className="truncate">{nameOf(id)}</span>
        {runningCount > 1 && <span className="dim">{t("等 ", "(")}{runningCount}{t(" 个", " running)")}</span>}
        <span className="dim">{t("· 查看日志", "· show log")}</span>
      </button>
    );
  } else if (fileBusy) {
    task = (
      <div className="status-seg">
        <span className="spinner" />
        {t("正在处理文件…", "Working on files…")}
      </div>
    );
  } else {
    task = (
      <div className="status-seg">
        <span className="status tone-accent">{t("就绪", "Ready")}</span>
      </div>
    );
  }

  const java = settings.javaPath && settings.javaPath !== "auto" ? t("指定", "Custom") : t("自动", "Auto");
  // The active instance's own memory setting wins over the global one, as at launch.
  const active = instances.find((i) => i.id === settings.activeInstanceId) ?? null;
  const instanceMemory = active?.memoryMb ?? null;
  const autoMemory = useAutoMemory(active);
  const memory =
    instanceMemory != null
      ? t(`单独 ${instanceMemory}`, `World ${instanceMemory}`)
      : settings.memoryAuto
        ? t(`自动 ${autoMemory ?? settings.memoryMb}`, `Auto ${autoMemory ?? settings.memoryMb}`)
        : `${settings.memoryMb}`;
  const source = t(`${settings.downloadSource === "bmclapi" ? "BMCLAPI" : "官方"} · ${settings.maxConcurrentDownloads} 并发`, `${settings.downloadSource === "bmclapi" ? "BMCLAPI" : "Official"} · ${settings.maxConcurrentDownloads} parallel`);
  const platform = `${appInfo.platform}-${appInfo.arch}`;
  const activity = (install ? 1 : 0) + queued + (fileBusy ? 1 : 0) + runningCount + (launchPending ? 1 : 0);
  return (
    <footer className="statusbar">
      {task}
      {activity > 0 && (
        <button type="button" className="status-seg status-tasks" aria-expanded={tasksOpen} onClick={() => setTasksOpen(!tasksOpen)}>
          {activity}{t(" 个任务 ", activity === 1 ? " task " : " tasks ")}{tasksOpen ? "▾" : "▴"}
        </button>
      )}
      {tasksOpen && activity > 0 && <TaskPanel onClose={() => setTasksOpen(false)} />}
      {/* Showing the window to watch the work keeps the plan; only this
          (or the tray) cancels it. */}
      {quitScheduled && (
        <button
          type="button"
          className="status-seg"
          title={t("游戏和后台任务都结束后，启动器会自动退出", "The launcher quits by itself once games and background work finish")}
          onClick={cancelScheduledQuit}
        >
          <span className="status tone-warn">{t("任务完成后退出", "Quitting when done")}</span>
          <span className="dim">{t("· 取消", "· cancel")}</span>
        </button>
      )}
      <RoomSegment />
      <div className="spacer" />
      {update && (
        <button
          type="button"
          className="status-seg status-update"
          title={update.notes || (update.package ? t("在设置里下载并安装", "Download and install in Settings") : t("打开下载页", "Open download page"))}
          onClick={() => (update.package ? useStore.getState().openSettings("about") : void openUrl(update.url))}
        >
          <span className="status tone-accent">{t("新版本 ", "New version ")}{update.version}</span>
        </button>
      )}
      {devInfo ? (
        <>
          <button type="button" className="status-seg" onClick={() => setPage("settings")}>
            {t("源 ", "Source ")}{source}
          </button>
          <div className="status-seg">Java {java}</div>
          <div className="status-seg">{t("内存 ", "Memory ")}{memory}M</div>
          <div className="status-seg dim">{platform}</div>
        </>
      ) : (
        <button
          type="button"
          className="status-seg dim status-version"
          title={t(`下载源 ${source}\nJava ${java}\n内存 ${memory}M\n${platform}`, `Source ${source}\nJava ${java}\nMemory ${memory}M\n${platform}`)}
          onClick={() => setPage("settings")}
        >
          v{appInfo.version}
        </button>
      )}
    </footer>
  );
}
