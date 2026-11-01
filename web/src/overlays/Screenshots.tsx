import { AnimatePresence, motion, useIsPresent } from "motion/react";
import { useEffect, useState } from "react";

import { toError } from "../api/backend";
import type { Instance, Screenshot } from "../api/types";
import { Icon } from "../components/Icon";
import { Button, Empty } from "../components/ui";
import { formatBytes } from "../lib/format";
import { EASE_OUT, fast, normal } from "../lib/motion";
import { shotTime, shotUrl, useScreenshots, useShots } from "../lib/screenshots";
import { usePick, useStore, type Modal } from "../store";
import { t } from "../lib/i18n";

const pad = (n: number) => String(n).padStart(2, "0");

/** "今天", "昨天", "10月26日", or with the year when it is not this one. */
export function dayLabel(millis: number, now = new Date()): string {
  const date = new Date(millis);
  const start = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const days = Math.round((start(now) - start(date)) / 86_400_000);
  if (days === 0) return t("今天", "Today");
  if (days === 1) return t("昨天", "Yesterday");
  const md = t(`${date.getMonth() + 1}月${date.getDate()}日`, `${["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][date.getMonth()]} ${date.getDate()}`);
  return date.getFullYear() === now.getFullYear() ? md : t(`${date.getFullYear()}年${md}`, `${md}, ${date.getFullYear()}`);
}

/** "10月" this year, "2025年12月" before. */
function monthLabel(millis: number, now = new Date()): string {
  const date = new Date(millis);
  const month = t(`${date.getMonth() + 1}月`, `${["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"][date.getMonth()]}`);
  return date.getFullYear() === now.getFullYear() ? month : t(`${date.getFullYear()}年${month}`, `${month} ${date.getFullYear()}`);
}

export function clockLabel(millis: number): string {
  const date = new Date(millis);
  return `${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

/** One thumbnail; it lifts on hover and opens the viewer. */
export function ShotTile({ id, shot, index, onOpen, label }: { id: string; shot: Screenshot; index: number; onOpen: () => void; label?: string }) {
  const [loaded, setLoaded] = useState(false);
  return (
    <motion.button
      type="button"
      className={`shot${loaded ? " loaded" : ""}`}
      onClick={onOpen}
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0, transition: { ...normal, delay: Math.min(index, 12) * 0.025 } }}
      whileHover={{ y: -3, transition: fast }}
      whileTap={{ y: 1, transition: fast }}
      aria-label={t(`查看截图 ${shot.name}`, `View screenshot ${shot.name}`)}
    >
      <img src={shotUrl(id, shot)} alt="" loading="lazy" decoding="async" draggable={false} onLoad={() => setLoaded(true)} />
      <span className="shot-label mono">{label ?? `${dayLabel(shotTime(shot))} ${clockLabel(shotTime(shot))}`}</span>
    </motion.button>
  );
}

/** The instance's screenshots by month, newest first. */
export function ScreenshotsTab({ instance }: { instance: Instance }) {
  const s = usePick((s) => ({ open: s.open, openModal: s.openModal, modal: s.modal }));
  const shots = useScreenshots(instance);
  const total = shots?.reduce((sum, shot) => sum + shot.bytes, 0) ?? 0;
  const months: { label: string; shots: Screenshot[] }[] = [];
  for (const shot of shots ?? []) {
    const label = monthLabel(shotTime(shot));
    const last = months[months.length - 1];
    if (last?.label === label) last.shots.push(shot);
    else months.push({ label, shots: [shot] });
  }
  // The viewer returns to this tab when it closes.
  const back = s.modal?.kind === "manage" ? s.modal : undefined;
  let index = 0;
  return (
    <div className="manage-scroll">
      <div className="pane-head">
        <div className="col gap-4">
          <div className="section-label">
            {t("截图", "Screenshots")}
            {shots && shots.length > 0 && (
              <span className="mono faint">
                {shots.length}{t(" 张 · ", " screenshots · ")}{formatBytes(total)}
              </span>
            )}
          </div>
          <span className="pane-hint">{t("在游戏里按 F2 截图。删除的截图会移到启动器的回收站目录。", "Press F2 in game for screenshots. Deleted ones move to the launcher's recycle folder.")}</span>
        </div>
        <div className="spacer" />
        <Button size="sm" variant="ghost" icon="folder" onClick={() => void s.open({ kind: "screenshotsDir", id: instance.id })}>
          {t("截图文件夹", "Screenshots folder")}
        </Button>
      </div>
      {shots == null ? (
        <div className="saves-empty muted">
          <span className="spinner" />{t(" 正在读取截图…", " Loading screenshots…")}
        </div>
      ) : shots.length === 0 ? (
        <Empty
          art={
            <span className="empty-pixel">
              <Icon name="camera" size={48} />
            </span>
          }
          title={t("还没有截图", "No screenshots yet")}
        >
          {t("游戏里按 F2 截图", "Press F2 in game")}
        </Empty>
      ) : (
        months.map((month) => (
          <section key={month.label} className="shot-day">
            <div className="shot-day-label">
              {month.label}
              <span className="mono faint">{month.shots.length}{t(" 张", " screenshots")}</span>
            </div>
            <div className="shot-grid">
              {month.shots.map((shot) => (
                <ShotTile key={shot.name} id={instance.id} shot={shot} index={index++} onOpen={() => s.openModal({ kind: "shot", id: instance.id, name: shot.name, back })} />
              ))}
            </div>
          </section>
        ))
      )}
    </div>
  );
}

/** Full-size viewer over a solid backdrop: arrows step through the day's
    pictures, the bar names and acts on the current one. */
export function ShotViewer({ id, name, back }: { id: string; name: string; back?: Modal }) {
  const s = usePick((s) => ({ instance: s.instances.find((i) => i.id === id) ?? null, open: s.open, toast: s.toast, closeModal: s.closeModal }));
  const present = useIsPresent();
  const shots = useShots((st) => st.lists[id]) ?? [];
  const index = shots.findIndex((shot) => shot.name === name);
  const shot = index >= 0 ? shots[index] : null;
  const [full, setFull] = useState<string | null>(null);
  const [confirm, setConfirm] = useState(false);
  const [direction, setDirection] = useState(0);

  const go = (delta: number) => {
    const next = shots[index + delta];
    if (!next) return;
    setDirection(delta);
    setConfirm(false);
    useStore.setState({ modal: { kind: "shot", id, name: next.name, back } });
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "ArrowLeft") go(-1);
      if (e.key === "ArrowRight") go(1);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  if (!shot) return null;
  const remove = async () => {
    if (!confirm) {
      setConfirm(true);
      return;
    }
    const next = shots[index + 1] ?? shots[index - 1] ?? null;
    try {
      await useShots.getState().recycle(id, shot.name);
      s.toast(t("截图已移到回收站目录", "Screenshot moved to the recycle folder"), "success");
      if (next) {
        setConfirm(false);
        useStore.setState({ modal: { kind: "shot", id, name: next.name, back } });
      } else s.closeModal();
    } catch (error) {
      s.toast(toError(error).message, "error");
    }
  };
  const taken = shotTime(shot);

  return (
    <motion.div className="overlay-root viewer" initial="initial" animate="animate" exit="exit" style={present ? undefined : { pointerEvents: "none" }}>
      <motion.div className="viewer-backdrop" onClick={s.closeModal} variants={{ initial: { opacity: 0 }, animate: { opacity: 1, transition: { duration: 0.2 } }, exit: { opacity: 0, transition: { duration: 0.16 } } }} />
      <motion.figure
        className="viewer-frame"
        variants={{
          initial: { opacity: 0, scale: 0.97, y: 10 },
          animate: { opacity: 1, scale: 1, y: 0, transition: { duration: 0.26, ease: EASE_OUT } },
          exit: { opacity: 0, scale: 0.98, transition: { duration: 0.14 } },
        }}
      >
        <div className="viewer-stage">
          <AnimatePresence initial={false} custom={direction} mode="popLayout">
            <motion.div
              key={shot.name}
              className="viewer-picture"
              custom={direction}
              initial={{ opacity: 0, x: direction * 40 }}
              animate={{ opacity: 1, x: 0, transition: { duration: 0.24, ease: EASE_OUT } }}
              exit={{ opacity: 0, x: direction * -40, transition: { duration: 0.14 } }}
            >
              {/* The cached thumbnail shows at once; the full picture covers it when decoded. */}
              <img className="viewer-thumb" src={shotUrl(id, shot)} alt="" draggable={false} />
              <img
                className={`viewer-full${full === shot.name ? " ready" : ""}`}
                src={shotUrl(id, shot, true)}
                alt={shot.name}
                draggable={false}
                decoding="async"
                onLoad={() => setFull(shot.name)}
              />
            </motion.div>
          </AnimatePresence>
          <button type="button" className="viewer-nav prev" disabled={index <= 0} onClick={() => go(-1)} aria-label={t("上一张", "Previous")}>
            <Icon name="chevronLeft" />
          </button>
          <button type="button" className="viewer-nav next" disabled={index >= shots.length - 1} onClick={() => go(1)} aria-label={t("下一张", "Next")}>
            <Icon name="chevronRight" />
          </button>
        </div>
        <figcaption className="viewer-bar">
          <div className="col gap-2">
            <span className="viewer-title truncate">
              {s.instance?.name ?? t("截图", "Screenshots")} · {dayLabel(taken)} {clockLabel(taken)}
            </span>
            <span className="viewer-meta mono truncate">
              {index + 1} / {shots.length} · {formatBytes(shot.bytes)} · {shot.name}
            </span>
          </div>
          <div className="spacer" />
          <Button size="sm" variant="hero" icon="external" onClick={() => void s.open({ kind: "screenshot", id, name: shot.name })}>
            {t("用系统应用打开", "Open with system app")}
          </Button>
          <Button size="sm" variant="hero" icon="folder" aria-label={t("打开截图文件夹", "Open the screenshots folder")} title={t("打开截图文件夹", "Open the screenshots folder")} onClick={() => void s.open({ kind: "screenshotsDir", id })} />
          <Button size="sm" variant={confirm ? "danger" : "hero"} icon="trash" onClick={() => void remove()} onBlur={() => setConfirm(false)}>
            {confirm ? t("确认删除", "Confirm delete") : t("删除", "Delete")}
          </Button>
          <Button size="sm" variant="hero" icon="close" aria-label={t("关闭", "Close")} onClick={s.closeModal} />
        </figcaption>
      </motion.figure>
    </motion.div>
  );
}
