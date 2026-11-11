import { AnimatePresence, motion } from "motion/react";
import { useEffect, useRef, useState } from "react";

import type { Instance, Screenshot, ServerEntry, WorldList } from "../api/types";
import { Icon } from "../components/Icon";
import { BrandMark, Celestial, Clouds, Landscape, PX, Stars } from "../components/Pixel";
import { ServerCard } from "../components/Server";
import { ShotTile, dayLabel } from "../overlays/Screenshots";
import { shotTime, useScreenshots } from "../lib/screenshots";
import { useServerList } from "../lib/servers";
import { BlockProgress, Button, Identicon, Kbd, Notice, ProgressBar, SectionTitle } from "../components/ui";
import { BIOMES, biomeStyle, useBiome, useCover } from "../lib/biome";
import { SceneCover } from "../components/SceneCover";
import { updateName, formatBytes, formatPlayTime, formatRelative, formatSpeed, formatToday, greeting, isNight, loaderLabel, skyPhase, progressRatio, stageLabel } from "../lib/format";
import { glide, item, normal, stagger } from "../lib/motion";
import { MOD_KEY } from "../lib/platform";
import { useWorlds } from "../lib/worlds";
import { usePatchNotes, wikiUrl } from "../lib/news";
import { useWorldStats, type WorldStats } from "../lib/worldStats";
import { instanceState, overlayOpen, useActiveAccount, useActiveInstance, useAutoMemory, usePick, useStore, type InstanceState, type LastError, type ModInsights } from "../store";
import { t } from "../lib/i18n";
import { LEVELS, useP2p } from "../lib/p2p";

/** Home is a title screen: the current world fills the top of the sheet,
    cut open down to the stone, with the other worlds on a hotbar along the
    stone; its facts sit below the cut, then a short look at its servers
    and screenshots. */
export function HomePage() {
  const instances = useStore((s) => s.instances);
  const active = useActiveInstance();
  const worlds = useWorlds(active);
  const servers = useServerList(active);
  const shots = useScreenshots(active);
  // Servers and screenshots share one row when both exist, so the page ends
  // within a screen or so; either alone takes the full width.
  const pair = Boolean(servers?.length && shots?.length);
  return (
    <div className="content">
      {active ? <Stage instance={active} worlds={worlds} /> : <Welcome hasInstances={instances.length > 0} />}
      <motion.div className="page home-below" variants={stagger} initial="initial" animate="animate">
        {active && <Facts instance={active} worlds={worlds} />}
        {active && (
          <div className={pair ? "home-pair" : undefined}>
            {servers && servers.length > 0 && <Multiplayer instance={active} list={servers} half={pair} />}
            {shots && shots.length > 0 && <RecentShots instance={active} shots={shots} half={pair} />}
          </div>
        )}
        {instances.length === 1 && active && <NewWorlds />}
        {active && <News />}
      </motion.div>
    </div>
  );
}

/** Opens the import dialog: a modpack, or games from other launchers. */
export function ImportPackButton({ disabled, label = t("导入", "Import"), variant }: { disabled?: boolean; label?: string; variant?: "hero" }) {
  const openModal = useStore((s) => s.openModal);
  return (
    <Button variant={variant} icon="archive" disabled={disabled} onClick={() => openModal({ kind: "import" })}>
      {label}
    </Button>
  );
}

/** Install failures where the files couldn't be fetched: what arrived is
    kept, so retrying resumes; the download source or proxy is what to change. */
const UNREACHABLE = new Set(["network", "http", "download", "download_slow", "download_size", "java_download"]);
/** Install failures where a file arrived damaged: it is fetched again. */
const DAMAGED = new Set(["download_hash", "fabric_hash", "metadata_hash", "java_extract", "invalid_data", "version_json"]);

/** What went wrong with this world last, and what to do about it. */
function ErrorBanner({ error, instance }: { error: LastError; instance: Instance | null }) {
  const s = usePick((s) => ({ copy: s.copy, installInstance: s.installInstance, openLogs: s.openLogs, launchFix: s.launchFix, openSettings: s.openSettings }));
  const title = !instance
    ? t("上次操作没有完成", "The last action didn't finish")
    : error.what === "install"
      ? t(`${instance.name} 下载没有完成`, `${instance.name} didn't finish installing`)
      : error.what === "crash"
        ? t(`${instance.name} 异常退出了`, `${instance.name} quit unexpectedly`)
        : t(`${instance.name} 没能启动`, `${instance.name} didn't start`);
  // Sign-in trouble is fixed by signing in, not by repairing files.
  const fix = instance && error.what === "launch" ? s.launchFix(instance.id, error.code) : null;
  // A download that couldn't reach its source is not a damaged file; each
  // gets its own next step.
  const cause = instance && error.what === "install" && error.code ? (UNREACHABLE.has(error.code) ? "network" : DAMAGED.has(error.code) ? "damaged" : null) : null;
  // The readable message leads; when the cause already says it, the
  // message joins the technical detail behind "Details".
  const detail = (cause ? [error.message, error.detail] : [error.detail]).filter(Boolean).join("\n");
  return (
    <motion.div initial={{ opacity: 0, height: 0 }} animate={{ opacity: 1, height: "auto", transition: normal }} exit={{ opacity: 0, height: 0, transition: { duration: 0.15 } }} style={{ overflow: "hidden", paddingBottom: 3 }}>
      <Notice
        dark
        tone="danger"
        title={title}
        detail={detail}
        actions={
          <>
            {fix && (
              <Button variant="hero" size="sm" icon={error.code === "auth_offline" ? "play" : "user"} onClick={fix.run}>
                {fix.label}
              </Button>
            )}
            {instance && cause === "network" && (
              <Button variant="hero" size="sm" icon="refresh" onClick={() => void s.installInstance(instance.id)}>
                {t("重试下载", "Retry download")}
              </Button>
            )}
            {instance && cause === "damaged" && (
              <Button variant="hero" size="sm" icon="shield" onClick={() => void s.installInstance(instance.id)}>
                {t("重新获取", "Fetch again")}
              </Button>
            )}
            {instance && !fix && !cause && (
              <Button variant="hero" size="sm" icon="shield" onClick={() => void s.installInstance(instance.id)}>
                {t("校验并修复", "Verify and repair")}
              </Button>
            )}
            {cause && (
              <Button variant="hero" size="sm" icon="settings" onClick={() => s.openSettings("download")}>
                {t("下载源与代理", "Source and proxy")}
              </Button>
            )}
            {instance && (
              <Button variant="hero" size="sm" icon="log" onClick={() => void s.openLogs(instance.id)}>
                {t("日志", "Log")}
              </Button>
            )}
            <Button variant="hero" size="sm" icon="copy" title={t("复制错误信息", "Copy the error")} aria-label={t("复制错误信息", "Copy the error")} onClick={() => void s.copy([error.message, error.detail].filter(Boolean).join("\n"), t("错误信息", "Error message"))} />
            <Button variant="hero" size="sm" icon="close" title={t("关闭", "Close")} aria-label={t("关闭", "Close")} onClick={() => useStore.setState({ lastError: null })} />
          </>
        }
      >
        {cause === "network"
          ? t("连不上下载源，或者速度太慢。已下载的部分会保留，重试会接着下载。", "The download source couldn't be reached or was too slow. What arrived is kept, and a retry resumes.")
          : cause === "damaged"
            ? t("有文件没通过校验，多半是下载源上的副本坏了。重新获取会丢掉坏文件再下载。", "A file failed its check, most likely a damaged copy on the source. Fetching again drops it and downloads anew.")
            : <span className="selectable">{error.message}</span>}
      </Notice>
    </motion.div>
  );
}

const HERO_STATUS: Record<InstanceState, [string, string]> = {
  running: [t("运行中", "Running"), "accent"],
  starting: [t("启动中", "Starting"), "warn"],
  installing: [t("安装中", "Installing"), "accent"],
  queued: [t("排队中", "Queued"), "warn"],
  ready: [t("已就绪", "Ready"), "accent"],
  notInstalled: [t("未安装", "Not installed"), "warn"],
};

const reducedMotion = () =>
  document.documentElement.classList.contains("reduce-motion") || window.matchMedia("(prefers-reduced-motion: reduce)").matches;

/** Mouse parallax: writes --px (−0.5 … 0.5) on the stage and CSS moves each
    layer by a different amount. Nothing runs while the mouse is still. */
function useParallax() {
  const ref = useRef<HTMLElement>(null);
  const frame = useRef(0);
  const onMouseMove = (e: React.MouseEvent) => {
    const el = ref.current;
    if (!el || frame.current || reducedMotion()) return;
    const x = e.clientX;
    frame.current = requestAnimationFrame(() => {
      frame.current = 0;
      const box = el.getBoundingClientRect();
      el.style.setProperty("--px", ((x - box.left) / box.width - 0.5).toFixed(3));
    });
  };
  const onMouseLeave = () => ref.current?.style.setProperty("--px", "0");
  return { ref, onMouseMove, onMouseLeave };
}

/** Sun or moon by the local time (low at dawn and dusk), stars at night,
    clouds always. */
function HeroSky({ seed }: { seed: string }) {
  const night = isNight();
  return (
    <>
      {night && <Stars seed={seed} cols={150} rows={14} unit={3} className="hero-stars" />}
      <motion.div
        className="hero-celestial"
        initial={{ y: 14, opacity: 0 }}
        animate={{ y: 0, opacity: 1, transition: { delay: 0.12, duration: 0.6, ease: [0.2, 0.8, 0.2, 1] } }}
      >
        <Celestial night={night} />
      </motion.div>
      <Clouds seed={seed} cols={150} rows={8} unit={3} className="hero-sky" />
    </>
  );
}

/** Greeting on the left, creation on the right; also the window's drag area. */
function StageBar() {
  const s = usePick((s) => ({ fileBusy: s.fileBusy, openCreate: s.openCreate }));
  const account = useActiveAccount();
  return (
    <header className="stage-bar" data-tauri-drag-region>
      <div className="col" data-tauri-drag-region>
        <span className="stage-greeting" data-tauri-drag-region>
          {greeting()}{t("，", ", ")}{account?.username ?? t("玩家", "player")}
        </span>
        <span className="stage-date" data-tauri-drag-region>
          {formatToday()}
        </span>
      </div>
      <div className="spacer" data-tauri-drag-region />
      <Button variant="hero" icon="download" onClick={() => useStore.getState().openModal({ kind: "modpacks" })}>{t("浏览整合包", "Browse modpacks")}</Button>
      <ImportPackButton variant="hero" disabled={s.fileBusy} />
      <Button variant="hero" icon="plus" onClick={s.openCreate}>
        {t("新建世界 ", "New world ")}<Kbd>{MOD_KEY}N</Kbd>
      </Button>
    </header>
  );
}

/** The current world, full-bleed. One action dominates: start. */
function Stage({ instance, worlds }: { instance: Instance; worlds: WorldList | null }) {
  const s = usePick((s) => ({
    install: s.install,
    installQueue: s.installQueue,
    installOutcome: s.installOutcome,
    running: s.running,
    runningIds: s.runningIds,
    launchPending: s.launchPending,
    lastError: s.lastError,
    openManage: s.openManage,
    open: s.open,
    openLogs: s.openLogs,
    primaryAction: s.primaryAction,
    launchInstance: s.launchInstance,
    stopping: s.stopping.includes(instance.id),
  }));
  const parallax = useParallax();
  const biome = useBiome(instance.id);
  const cover = useCover(instance.id);
  const phase = skyPhase();
  const state = instanceState(instance, s);
  const install = s.install?.instanceId === instance.id ? s.install : null;
  const [statusLabel, statusTone] = HERO_STATUS[state];
  const room = useP2p((s) => !!s.status?.active);
  const busy = state === "running" || state === "starting" || state === "installing";

  // An install that just finished gets its moment: the blocks grow grass.
  // It follows the completion event itself; the world's "installed" flag
  // only arrives with the refresh after it. An outcome seen at mount is old.
  const seenOutcome = useRef(s.installOutcome);
  const [justInstalled, setJustInstalled] = useState(false);
  useEffect(() => {
    const outcome = s.installOutcome;
    if (outcome === seenOutcome.current) return;
    seenOutcome.current = outcome;
    if (!outcome?.ok || outcome.instanceId !== instance.id) return;
    setJustInstalled(true);
    const timer = setTimeout(() => setJustInstalled(false), 1600);
    return () => clearTimeout(timer);
  }, [s.installOutcome, instance.id]);
  useEffect(() => setJustInstalled(false), [instance.id]);

  const primary = (() => {
    switch (state) {
      case "running":
        return s.stopping
          ? { label: t("正在关闭… · 强制结束", "Closing… · Force quit"), icon: "stop" as const, variant: "default" as const }
          : { label: t("停止游戏", "Stop game"), icon: "stop" as const, variant: "danger" as const };
      case "starting":
        return { label: t("正在启动…", "Starting…"), busy: true, variant: "primary" as const };
      case "installing":
        return { label: t("正在安装…", "Installing…"), busy: true, variant: "primary" as const };
      case "ready":
        return { label: t("开始游戏", "Play"), icon: "play" as const, variant: "primary" as const };
      case "queued":
        return { label: t("排队下载中 · 取消", "Queued · cancel"), icon: "close" as const, variant: "default" as const };
      default:
        return { label: t("安装并开始", "Install and play"), icon: "download" as const, variant: "primary" as const };
    }
  })();

  return (
    <section ref={parallax.ref} onMouseMove={parallax.onMouseMove} onMouseLeave={parallax.onMouseLeave} className={`stage ${phase}`} style={biomeStyle(biome, phase)}>
      <HeroSky seed={instance.id} />
      {/* Switching worlds travels: the old land leaves to the left while the new one arrives from the right. */}
      <AnimatePresence initial={false}>
        <motion.div
          key={instance.id}
          className="stage-land"
          initial={{ x: 160, opacity: 0 }}
          animate={{ x: 0, opacity: 1, transition: { duration: 0.75, ease: [0.2, 0.8, 0.2, 1] } }}
          exit={{ x: -160, opacity: 0, transition: { duration: 0.45, ease: [0.4, 0, 1, 1] } }}
        >
          {/* Each land keeps its own biome, so the one leaving does not recolour. */}
          <div style={biomeStyle(biome, phase)}>
            <Landscape seed={instance.id} unit={PX} cols={150} ground={[6, 8]} sky={8} treesFrom={50} treeChance={0.4} far={[12, 16]} near={[9, 12]} biome={biome} soilDepth={2} />
          </div>
          <SceneCover id={instance.id} full className="stage-cover" />
        </motion.div>
      </AnimatePresence>

      <StageBar />
      <div className="stage-body">
        <AnimatePresence>{s.lastError?.instanceId === instance.id && <ErrorBanner key="error" error={s.lastError} instance={instance} />}</AnimatePresence>
        <motion.div
          key={instance.id}
          className="stage-intro"
          initial={{ opacity: 0, y: 12 }}
          animate={{ opacity: 1, y: 0, transition: { duration: 0.45, ease: [0.2, 0.8, 0.2, 1] } }}
        >
          <div className="stage-eyebrow">
            <Identicon seed={instance.id} size={26} hero />
            {t("当前世界", "Current world")}{cover ? "" : ` · ${biome.name}`}
            <span className={`hero-status tone-${statusTone}${busy ? " pulse" : ""}`}>{statusLabel}</span>
          </div>
          <h1 className="stage-title">{instance.name}</h1>
          <div className="stage-meta">
            <span className="mono hero-version">{instance.versionId}</span>
            <span className="dot" />
            {loaderLabel(instance.loader)}
            <span className="dot" />
            {instance.lastLaunchedAt ? t(`上次游玩 ${formatRelative(instance.lastLaunchedAt)}`, `Last played ${formatRelative(instance.lastLaunchedAt)}`) : t("尚未游玩", "Never played")}
            {instance.playSeconds >= 60 && (
              <>
                <span className="dot" />
                {t("已玩 ", "Played ")}{formatPlayTime(instance.playSeconds)}
              </>
            )}
          </div>
        </motion.div>

        <AnimatePresence initial={false}>
          {(install || justInstalled) && (
            <motion.div
              key="progress"
              className="hero-progress"
              initial={{ height: 0, opacity: 0 }}
              animate={{ height: "auto", opacity: 1, transition: normal }}
              exit={{ height: 0, opacity: 0, transition: { duration: 0.18 } }}
            >
              {install ? (
                <InstallProgressView />
              ) : (
                <div className="install">
                  <div className="row gap-12">
                    <span className="install-percent">100</span>
                    <span className="install-stage install-done">{t("安装完成，可以开始游戏了", "Installed. Ready to play")}</span>
                  </div>
                  <BlockProgress ratio={1} grow />
                </div>
              )}
            </motion.div>
          )}
        </AnimatePresence>

        <div className="stage-actions">
          <Button
            size="xl"
            variant={primary.variant}
            icon={"icon" in primary ? primary.icon : undefined}
            busy={"busy" in primary ? primary.busy : false}
            onClick={() => s.primaryAction(instance.id)}
            className="hero-primary"
          >
            {primary.label}
          </Button>
          <Button variant="hero" icon="settings" onClick={() => s.openManage(instance.id)}>
            {t("管理", "Manage")}
          </Button>
          <Button variant="hero" icon="folder" onClick={() => void s.open({ kind: "gameDir", id: instance.id })}>
            {t("目录", "Folder")}
          </Button>
          <Button variant="hero" icon="log" onClick={() => void s.openLogs(instance.id)}>
            {t("日志", "Log")}
          </Button>
        </div>
        {state === "ready" && worlds?.quickPlay && worlds.worlds[0] && (
          <motion.button
            type="button"
            className="btn btn-hero stage-continue"
            initial={{ opacity: 0, y: 6 }}
            animate={{ opacity: 1, y: 0, transition: { delay: 0.2, duration: 0.4, ease: [0.2, 0.8, 0.2, 1] } }}
            onClick={() => void s.launchInstance(instance.id, worlds.worlds[0].folder)}
          >
            <Icon name="play" />
            <span className="stage-continue-label">
              {t("继续存档", "Continue")}
              {worlds.worlds[0].icon ? <img className="stage-continue-icon" src={worlds.worlds[0].icon} alt="" /> : <Icon name="map" />}
              <span className="truncate">{worlds.worlds[0].folder}</span>
            </span>
            {worlds.worlds[0].modifiedMs && <span className="stage-continue-when">{formatRelative(worlds.worlds[0].modifiedMs)}</span>}
          </motion.button>
        )}
        <AnimatePresence>{room && <StageRoom key="room" />}</AnimatePresence>
      </div>
      <Hotbar activeId={instance.id} />
    </section>
  );
}

/** Up to nine worlds for the hotbar: starred ones first, each group in the
    worlds' own order so a slot keeps its number; the current world always
    has a place, taking the last one when it would fall off the end. */
function hotbarSlots(instances: Instance[], activeId: string) {
  const ordered = [...instances.filter((i) => i.favorite), ...instances.filter((i) => !i.favorite)];
  const slots = ordered.slice(0, 9);
  if (!slots.some((i) => i.id === activeId)) {
    const active = instances.find((i) => i.id === activeId);
    if (active) slots[slots.length - 1] = active;
  }
  return slots;
}

/** The worlds along the stone, as the game keeps its items: the current
    one framed, a click or its number key travels there. Running worlds
    glow in their corner; one installing shows a bar along its foot. */
function Hotbar({ activeId }: { activeId: string }) {
  const s = usePick((s) => ({
    instances: s.instances,
    runningIds: s.runningIds,
    install: s.install,
    setPage: s.setPage,
    setActiveInstance: s.setActiveInstance,
  }));
  const slots = hotbarSlots(s.instances, activeId);
  const rest = s.instances.length - slots.length;
  const travel = (id: string) => {
    if (id !== activeId) s.setActiveInstance(id);
    document.querySelector(".content")?.scrollTo({ top: 0, behavior: reducedMotion() ? "auto" : "smooth" });
  };
  const travelTo = useRef(travel);
  travelTo.current = travel;
  const ids = slots.map((i) => i.id).join(" ");
  // Number keys pick a slot, as in the game, unless something is typed into.
  useEffect(() => {
    const list = ids.split(" ");
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey || e.shiftKey || e.isComposing || e.repeat) return;
      const n = Number(e.key);
      if (!Number.isInteger(n) || n < 1 || n > list.length) return;
      if (e.target instanceof Element && e.target.closest("input, textarea, select, [contenteditable]")) return;
      if (overlayOpen(useStore.getState())) return;
      e.preventDefault();
      travelTo.current(list[n - 1]);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [ids]);
  if (s.instances.length < 2) return null;
  // Arrows move along the bar, like a toolbar.
  const onKeyDown = (e: React.KeyboardEvent<HTMLElement>) => {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    const buttons = [...e.currentTarget.querySelectorAll<HTMLButtonElement>("button")];
    const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
    buttons[(at + (e.key === "ArrowRight" ? 1 : buttons.length - 1)) % buttons.length]?.focus();
    e.preventDefault();
  };
  return (
    <nav className="hotbar" aria-label={t("世界", "Worlds")} onKeyDown={onKeyDown}>
      {slots.map((world, i) => {
        const on = world.id === activeId;
        const install = s.install?.instanceId === world.id ? s.install : null;
        const ratio = install ? progressRatio(install.done, install.total, install.bytes, install.totalBytes) : null;
        return (
          <button
            key={world.id}
            type="button"
            className="hotbar-slot"
            aria-current={on ? "true" : undefined}
            title={`${world.name} · ${i + 1}`}
            onClick={() => travel(world.id)}
          >
            {on && <motion.span layoutId="hotbar-frame" className="hotbar-frame" transition={glide} />}
            <Identicon seed={world.id} size={36} hero />
            <span className="hotbar-key">{i + 1}</span>
            {s.runningIds.includes(world.id) && <span className="hotbar-live" />}
            {install && (
              <span className="hotbar-bar">
                <span style={{ width: `${Math.round((ratio ?? 0) * 100)}%` }} />
              </span>
            )}
          </button>
        );
      })}
      {rest > 0 && (
        <button type="button" className="hotbar-slot hotbar-more" title={t("全部世界", "All worlds")} onClick={() => s.setPage("instances")}>
          +{rest}
        </button>
      )}
    </nav>
  );
}

/** An open room rides on the stage: its health and head count lead to the
    multiplayer page, and the code (or, for a guest, the address) copies. */
function StageRoom() {
  const status = useP2p((s) => s.status);
  const copy = useStore((s) => s.copy);
  const setPage = useStore((s) => s.setPage);
  if (!status?.active) return null;
  const level = status.health?.level ?? "starting";
  const people = (status.health?.players.length ?? 0) + 1;
  const host = status.role === "host";
  const share = host ? status.code : status.address;
  return (
    <motion.div
      className="stage-room"
      initial={{ opacity: 0, y: 6 }}
      animate={{ opacity: 1, y: 0, transition: normal }}
      exit={{ opacity: 0, transition: { duration: 0.12 } }}
    >
      <button type="button" className="stage-room-open" onClick={() => setPage("multiplayer")} title={t("打开联机页", "Open multiplayer")}>
        <span className={`room-dot level-${level}${LEVELS[level].pulse ? " pulse" : ""}`} />
        <span className="stage-room-label">{host ? t("联机中 · 你是房主", "Hosting a room") : t("联机中 · 已加入房间", "In a room")}</span>
        <span className="stage-room-meta">
          {LEVELS[level].label} · {t(`${people} 人`, `${people} in room`)}
        </span>
      </button>
      {share && (
        <button type="button" className="stage-room-copy" onClick={() => void copy(share, host ? t("房间码", "Room code") : t("地址", "Address"))} title={host ? t("复制房间码", "Copy room code") : t("复制地址", "Copy address")}>
          <Icon name="copy" />
          <span className="mono truncate">{share}</span>
        </button>
      )}
    </motion.div>
  );
}

function InstallProgressView() {
  const install = useStore((s) => s.install);
  const cancel = useStore((s) => s.cancelInstall);
  if (!install) return null;
  const ratio = progressRatio(install.done, install.total, install.bytes, install.totalBytes);
  return (
    <div className="install">
      <div className="row gap-12">
        <span className="install-percent mono">{ratio == null ? "··" : Math.round(ratio * 100)}</span>
        <div className="col fill gap-2">
          <span className="install-stage">{stageLabel(install.stage)}</span>
          <span className="install-detail mono">
            {install.total > 0 && t(`${install.done} / ${install.total} 个文件`, `${install.done} / ${install.total} files`)}
            {install.totalBytes > 0 && ` · ${formatBytes(install.bytes)} / ${formatBytes(install.totalBytes)}`}
            {install.speed != null && install.speed > 1 && ` · ${formatSpeed(install.speed)}`}
          </span>
        </div>
        <Button size="sm" variant="hero" onClick={cancel}>
          {t("取消", "Cancel")}
        </Button>
      </div>
      {ratio == null ? <ProgressBar ratio={null} height={8} /> : <BlockProgress ratio={ratio} />}
    </div>
  );
}

/** Saves, memory and mods (Java for a vanilla world) for the current world,
    just below its ground; each opens the place where it is changed. (The
    account already lives in the sidebar.) */
function Facts({ instance, worlds }: { instance: Instance; worlds: WorldList | null }) {
  const s = usePick((s) => ({ settings: s.settings, openManage: s.openManage, insights: s.modInsights }));
  const stats = useWorldStats(instance);
  const autoMemory = useAutoMemory(instance);
  const automatic = instance.memoryMb == null && s.settings.memoryAuto;
  const memory = instance.memoryMb ?? (automatic ? (autoMemory ?? s.settings.memoryMb) : s.settings.memoryMb);
  const memorySub = instance.memoryMb != null ? t("世界单独设置", "Set for this world") : automatic ? t("自动 · 按本机内存与模组数", "Auto · from this computer's memory and mod count") : t("使用全局设置", "Global setting");
  const javaCustom = instance.javaPath && instance.javaPath !== "auto" ? instance.javaPath : !instance.javaPath && s.settings.javaPath ? s.settings.javaPath : null;
  const javaMajor = javaCustom?.match(/(?:jdk|jre|java)[-_]?(\d+)/i)?.[1];
  const javaName = javaCustom ? (javaMajor ? `Java ${javaMajor}` : t("指定路径", "Custom path")) : t("自动", "Auto");

  return (
    <motion.div className="facts" variants={item}>
      <button type="button" className="fact" onClick={() => s.openManage(instance.id, "data")}>
        <span className="fact-label">{t("存档", "Saves")}</span>
        <div className="row items-baseline gap-6">
          <Roll className="fact-number" value={worlds ? worlds.worlds.length : "··"} />
          <span className="fact-unit">{t("个", "")}</span>
        </div>
        <span className="fact-sub truncate">
          {!worlds ? t("正在读取…", "Loading…") : worlds.worlds[0] ? t(`最近的存档：${worlds.worlds[0].folder}`, `Latest: ${worlds.worlds[0].folder}`) : t("还没有存档", "No saves yet")}
        </span>
        <Icon name="chevronRight" className="fact-go" />
      </button>
      <button type="button" className="fact" onClick={() => s.openManage(instance.id)}>
        <span className="fact-label">{t("内存", "Memory")}</span>
        <div className="row items-baseline gap-6">
          <Roll className="fact-number" value={memory >= 1024 && memory % 1024 === 0 ? memory / 1024 : memory} />
          <span className="fact-unit">{memory >= 1024 && memory % 1024 === 0 ? "GB" : "MB"}</span>
          <span className="fact-blocks" aria-hidden>
            {Array.from({ length: Math.min(16, Math.max(1, Math.round(memory / 1024))) }, (_, i) => (
              <i key={i} />
            ))}
          </span>
        </div>
        <span className="fact-sub">{memorySub}</span>
        <Icon name="chevronRight" className="fact-go" />
      </button>
      {instance.loader !== "vanilla" ? (
        <ModsFact instance={instance} stats={stats} insights={s.insights} />
      ) : (
        <button type="button" className="fact" onClick={() => s.openManage(instance.id)}>
          <span className="fact-label">Java</span>
          <span className="fact-number fact-word">{javaName}</span>
          <span className="fact-sub truncate">{javaCustom ? javaCustom : t("按游戏版本匹配，缺少时自动下载", "Matched to the game version, downloaded if missing")}</span>
          <Icon name="chevronRight" className="fact-go" />
        </button>
      )}
    </motion.div>
  );
}

/** A number that rolls up into place when it changes, as on a counter:
    travelling to another world turns its facts over. */
function Roll({ value, className }: { value: string | number; className: string }) {
  // The first value is simply there; later ones roll in.
  const mounted = useRef(false);
  useEffect(() => {
    mounted.current = true;
  }, []);
  return (
    <motion.span key={value} className={`roll ${className}`} initial={mounted.current ? { opacity: 0, y: 8 } : false} animate={{ opacity: 1, y: 0, transition: normal }}>
      {value}
    </motion.span>
  );
}

/** A modded world's mods: how many, and what can be done about them now. */
function ModsFact({ instance, stats, insights }: { instance: Instance; stats: WorldStats | null; insights: ModInsights }) {
  const openManage = useStore((s) => s.openManage);
  // Updates are known once the mods tab has checked this world.
  const updates = insights.id === instance.id && insights.checked ? Object.values(insights.byFile).filter((info) => info?.update).length : 0;
  const off = stats?.mods != null && stats.modsOn != null ? stats.mods - stats.modsOn : 0;
  return (
    <button type="button" className="fact" onClick={() => openManage(instance.id, "mods")}>
      <span className="fact-label">{t("模组", "Mods")}</span>
      <div className="row items-baseline gap-6">
        <Roll className="fact-number" value={stats?.mods ?? "··"} />
        <span className="fact-unit">{t("个", "")}</span>
      </div>
      <span className={`fact-sub truncate${updates > 0 ? " fact-sub-go" : ""}`}>
        {!instance.installed
          ? t("未安装", "Not installed")
          : !stats
            ? t("正在读取…", "Loading…")
            : updates > 0
              ? t(`${updates} 个可更新`, `${updates} can be updated`)
              : stats.mods === 0
                ? t("还没有模组", "No mods yet")
                : off > 0
                  ? t(`${off} 个已停用`, `${off} disabled`)
                  : t("全部已启用", "All enabled")}
      </span>
      <Icon name="chevronRight" className="fact-go" />
    </button>
  );
}

/** The current world's servers with live status; join in one click. */
function Multiplayer({ instance, list, half }: { instance: Instance; list: ServerEntry[]; half: boolean }) {
  const openManage = useStore((s) => s.openManage);
  return (
    <motion.section variants={item} initial="initial" animate="animate" style={{ marginTop: 30, minWidth: 0 }}>
      <SectionTitle
        trailing={
          <button type="button" className="link" onClick={() => openManage(instance.id, "servers")}>
            {list.length > 2 ? t(`全部 ${list.length} 个`, `All ${list.length}`) : t("管理服务器", "Manage servers")} →
          </button>
        }
      >
        {t("多人游戏", "Multiplayer")}
      </SectionTitle>
      <div className={`server-grid home-servers${half ? " stacked" : ""}`}>
        {list.slice(0, 2).map((server) => (
          <ServerCard key={`${server.index}:${server.address}`} instance={instance} server={server} compact />
        ))}
      </div>
    </motion.section>
  );
}

/** The newest screenshots of the current world: a film strip, or a 2×2
    grid beside the servers. */
function RecentShots({ instance, shots, half }: { instance: Instance; shots: Screenshot[]; half: boolean }) {
  const s = usePick((s) => ({ openManage: s.openManage, openModal: s.openModal }));
  return (
    <motion.section variants={item} initial="initial" animate="animate" style={{ marginTop: 30, minWidth: 0 }}>
      <SectionTitle
        trailing={
          <button type="button" className="link" onClick={() => s.openManage(instance.id, "shots")}>
            {t("全部 ", "All ")}{shots.length}{t(" 张 →", " →")}
          </button>
        }
      >
        {t("最近的截图", "Recent screenshots")}
      </SectionTitle>
      <div className={half ? "shot-grid" : "shot-strip"}>
        {shots.slice(0, half ? 4 : 5).map((shot, i) => (
          <ShotTile key={shot.name} id={instance.id} shot={shot} index={i} label={dayLabel(shotTime(shot))} onOpen={() => s.openModal({ kind: "shot", id: instance.id, name: shot.name })} />
        ))}
      </div>
    </motion.section>
  );
}

/** What came out lately, from Mojang's own feed; each opens its wiki page,
    or starts a world on that version. Quietly absent when offline. */
function News() {
  const notes = usePatchNotes();
  const s = usePick((s) => ({ openUrl: s.openUrl }));
  if (!notes || notes.length === 0) return null;
  return (
    <motion.section variants={item} style={{ marginTop: 30 }}>
      <SectionTitle>{t("版本动态", "What's new")}</SectionTitle>
      <div className="news">
        {notes.slice(0, 3).map((note) => (
          <article key={note.version} className="news-item">
            <div className="row gap-8">
              <span className={`chip${note.kind === "release" ? " chip-accent" : ""}`}>{note.kind === "release" ? t("正式版", "Release") : t("快照", "Snapshot")}</span>
              <span className="news-title truncate" title={note.title}>
                {note.version}
                {updateName(note.version) ? ` · ${updateName(note.version)}` : ""}
              </span>
              <span className="spacer" />
              <span className="faint mono" style={{ fontSize: "var(--text-xs)", whiteSpace: "nowrap" }}>
                {note.date.slice(0, 10)}
              </span>
            </div>
            {note.summary && <p className="news-summary">{note.summary}</p>}
            <div className="row gap-6">
              <Button size="sm" variant="ghost" icon="external" onClick={() => void s.openUrl(wikiUrl(note.version))}>
                {t("更新内容", "Changelog")}
              </Button>
              <Button size="sm" variant="ghost" icon="plus" onClick={() => useStore.setState({ modal: { kind: "create", version: note.version } })}>
                {t("用这个版本新建", "New world on it")}
              </Button>
            </div>
          </article>
        ))}
      </div>
    </motion.section>
  );
}

/** With a single instance, the next step is another world. */
function NewWorlds() {
  const s = usePick((s) => ({ openCreate: s.openCreate, fileBusy: s.fileBusy, chooseFiles: s.chooseFiles, fileOperation: s.fileOperation }));
  return (
    <motion.section variants={item} style={{ marginTop: 30 }}>
      <SectionTitle>{t("开辟新世界", "Start another world")}</SectionTitle>
      <div className="starts">
        <button type="button" className="start" onClick={s.openCreate}>
          <span className="start-icon">
            <Icon name="plus" />
          </span>
          <span className="col fill gap-4">
            <span className="start-title">{t("新建世界", "New world")}</span>
            <span className="start-text">{t("原版、Fabric、Quilt、Forge、NeoForge", "Vanilla, Fabric, Quilt, Forge, NeoForge")}</span>
          </span>
          <Kbd>{MOD_KEY}N</Kbd>
        </button>
        <button type="button" className="start" disabled={s.fileBusy} onClick={() => useStore.getState().openModal({ kind: "import" })}>
          <span className="start-icon">
            <Icon name="archive" />
          </span>
          <span className="col fill gap-4">
            <span className="start-title">{t("导入", "Import")}</span>
            <span className="start-text">{t("整合包，或官方启动器、HMCL、PCL、Prism 里已有的游戏", "A modpack, or games you already have in the official launcher, HMCL, PCL or Prism")}</span>
          </span>
          <Icon name="chevronRight" className="faint" />
        </button>
      </div>
    </motion.section>
  );
}

/** The first step: a Microsoft account that owns the game. Offline
    accounts open after it, on the Accounts page. */
function FirstSignIn({ onMore }: { onMore: () => void }) {
  const s = usePick((s) => ({ ready: s.loginConfigured, busy: s.authBusy || s.auth !== null, startLogin: s.startLogin, openSettings: s.openSettings }));
  return (
    <span className="first-sign-in">
      <Button size="sm" variant="primary" icon="shield" busy={s.busy} onClick={() => (s.ready ? void s.startLogin("browser") : s.openSettings("login"))}>
        {s.ready ? t("用微软账号登录", "Sign in with Microsoft") : t("配置微软登录", "Set up Microsoft sign-in")}
      </Button>
      <button type="button" className="link first-sign-in-more" onClick={onMore}>
        {t("其他登录方式 →", "Other ways to sign in →")}
      </button>
    </span>
  );
}

/** First run: the same title screen, with the three steps to a first game. */
function Welcome({ hasInstances }: { hasInstances: boolean }) {
  const s = usePick((s) => ({ setPage: s.setPage, openCreate: s.openCreate, lastError: s.lastError }));
  const parallax = useParallax();
  const account = useActiveAccount();
  const steps = [
    {
      title: t("登录微软账号", "Sign in with Microsoft"),
      text: t("离线账号在验证过拥有 Java 版后才能添加", "Offline accounts open after a sign-in that owns Java Edition"),
      done: account !== null,
      action: <FirstSignIn onMore={() => s.setPage("accounts")} />,
    },
    {
      title: t("创建世界", "Create a world"),
      text: null,
      done: hasInstances,
      action: (
        <Button size="sm" variant="primary" icon="plus" onClick={s.openCreate}>
          {t("选择版本", "Choose version")}
        </Button>
      ),
    },
    {
      title: t("开始游戏", "Play"),
      text: t("首次启动会自动下载游戏文件和所需的 Java", "The first start downloads the game files and the Java it needs"),
      done: false,
      action: null,
    },
  ];
  return (
    <section ref={parallax.ref} onMouseMove={parallax.onMouseMove} onMouseLeave={parallax.onMouseLeave} className={`stage welcome ${skyPhase()}`} style={biomeStyle(BIOMES.plains, skyPhase())}>
      <HeroSky seed="welcome" />
      <div className="stage-land">
        <Landscape seed="welcome" unit={PX} cols={150} ground={[6, 8]} sky={8} treesFrom={2} treeChance={0.3} far={[12, 16]} near={[9, 12]} soilDepth={2} />
      </div>
      <StageBar />
      <div className="stage-body">
        <AnimatePresence>{s.lastError && <ErrorBanner key="error" error={s.lastError} instance={null} />}</AnimatePresence>
        <div className="row gap-16">
          <BrandMark size={52} />
          <div className="col gap-6">
            <h1 className="stage-title">{t("三步开始游戏", "Three steps to play")}</h1>
            <span className="stage-meta">{t("每个世界的版本、存档、模组和内存设置互相独立。", "Each world has its own version, saves, mods and memory.")}</span>
          </div>
        </div>
        <motion.div className="steps" variants={stagger} initial="initial" animate="animate">
          {steps.map((step, i) => (
            <motion.div key={step.title} className={`step${step.done ? " done" : ""}`} variants={item}>
              <div className="step-index mono">{step.done ? <Icon name="check" /> : i + 1}</div>
              <div className="step-title">{step.title}</div>
              {step.text && <div className="step-text">{step.text}</div>}
              {!step.done && step.action}
            </motion.div>
          ))}
        </motion.div>
      </div>
    </section>
  );
}
