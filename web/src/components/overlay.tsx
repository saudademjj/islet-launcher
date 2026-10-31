import { AnimatePresence, motion, useIsPresent } from "motion/react";
import { useEffect, useRef, useState, type ReactNode, type UIEvent } from "react";

import { restoreFocus, trapTab } from "../lib/focus";
import { panelIn } from "../lib/motion";
import { SMALL_PX } from "../lib/pixelScale";
import { useStore, type Toast } from "../store";
import { Icon } from "./Icon";
import { Sign } from "./Pixel";
import { SceneHeader } from "./SceneHeader";
import { t } from "../lib/i18n";

/** Centered dialog over a solid scrim. Mount inside <AnimatePresence>. */
export function Modal({
  title,
  subtitle,
  icon,
  wide,
  onClose,
  footer,
  children,
  bodyClass,
  scene,
}: {
  /** Replaces the default header, e.g. with a world's own <SceneHeader>. */
  scene?: ReactNode;
  title?: ReactNode;
  subtitle?: ReactNode;
  icon?: ReactNode;
  wide?: boolean;
  onClose: () => void;
  footer?: ReactNode;
  children: ReactNode;
  bodyClass?: string;
}) {
  // Focus moves in (unless a field took it with autoFocus) and goes back
  // to where it was on close; the app behind is inert meanwhile.
  const panel = useRef<HTMLDivElement>(null);
  // Read while rendering, before the app behind turns inert and drops it.
  const before = useRef(typeof document === "undefined" ? null : document.activeElement);
  const present = useIsPresent();
  useEffect(() => {
    const dialog = panel.current;
    const from = before.current;
    if (dialog && !dialog.contains(document.activeElement)) dialog.focus({ preventScroll: true });
    return () => restoreFocus(dialog, from);
  }, []);
  return (
    // While it fades out, clicks go through to the app behind instead of
    // landing on the fading scrim.
    <motion.div className="overlay-root" initial="initial" animate="animate" exit="exit" style={present ? undefined : { pointerEvents: "none" }}>
      <motion.div
        className="scrim"
        onClick={onClose}
        variants={{ initial: { opacity: 0 }, animate: { opacity: 1, transition: { duration: 0.2 } }, exit: { opacity: 0, transition: { duration: 0.16 } } }}
      />
      <motion.div ref={panel} className={`modal${wide ? " wide" : ""}`} variants={panelIn} role="dialog" aria-modal tabIndex={-1} onKeyDown={(e) => trapTab(e, panel.current)}>
        {/* A dialog with no world of its own still opens onto one: a low
            strip of land, the same family as the world dialogs. */}
        {scene ?? <SceneHeader seed={typeof title === "string" ? title : "launcher"} title={title} meta={subtitle} aside={icon} onClose={onClose} className="plain" />}
        <div className={bodyClass ?? "modal-body"}>{children}</div>
        {footer && <div className="modal-footer">{footer}</div>}
      </motion.div>
    </motion.div>
  );
}

export function Toasts() {
  const toasts = useStore((s) => s.toasts);
  return (
    <div className="toasts" aria-live="polite">
      <AnimatePresence initial={false}>
        {toasts.map((toast) => (
          <ToastItem key={toast.id} toast={toast} />
        ))}
      </AnimatePresence>
    </div>
  );
}

/** How long a toast stays: errors long enough to read and copy. */
const TOAST_MS = { info: 3200, success: 3200, error: 10000 };

/** One toast. Pointing at it or tabbing into it holds it; text can be
    selected, and only the ✕ closes it. */
function ToastItem({ toast }: { toast: Toast }) {
  const dismiss = useStore((s) => s.dismissToast);
  const copy = useStore((s) => s.copy);
  const [held, setHeld] = useState(false);
  const left = useRef(TOAST_MS[toast.kind]);
  useEffect(() => {
    if (held) return;
    const started = Date.now();
    const timer = window.setTimeout(() => dismiss(toast.id), left.current);
    return () => {
      window.clearTimeout(timer);
      left.current = Math.max(1200, left.current - (Date.now() - started));
    };
  }, [held, toast.id, dismiss]);
  return (
    <motion.div
      layout
      className={`toast ${toast.kind}`}
      role={toast.kind === "error" ? "alert" : "status"}
      initial={{ opacity: 0, x: 24, scale: 0.98 }}
      animate={{ opacity: 1, x: 0, scale: 1, transition: { duration: 0.24, ease: [0.2, 0.8, 0.2, 1] } }}
      exit={{ opacity: 0, x: 16, transition: { duration: 0.14, ease: [0.4, 0, 1, 1] } }}
      onMouseEnter={() => setHeld(true)}
      onMouseLeave={() => setHeld(false)}
      onFocus={() => setHeld(true)}
      onBlur={() => setHeld(false)}
    >
      <Sign mark={toast.kind === "success" ? "check" : toast.kind === "error" ? "alert" : "info"} tone={toast.kind === "success" ? "ok" : toast.kind === "error" ? "danger" : undefined} unit={SMALL_PX} />
      <span className="toast-text selectable">{toast.message}</span>
      {toast.kind === "error" && (
        <button type="button" className="toast-act" onClick={() => void copy(toast.message, t("错误信息", "Error message"))}>
          {t("复制", "Copy")}
        </button>
      )}
      {toast.action && (
        <button
          type="button"
          className="toast-act"
          onClick={() => {
            toast.action?.run();
            dismiss(toast.id);
          }}
        >
          {toast.action.label}
        </button>
      )}
      <button type="button" className="toast-close" aria-label={t("关闭", "Close")} title={t("关闭", "Close")} onClick={() => dismiss(toast.id)}>
        <Icon name="close" />
      </button>
    </motion.div>
  );
}

/** Fixed-height rows; only the visible window is in the DOM. */
export function VirtualList<T>({
  items,
  rowHeight,
  render,
  className,
  style,
  stickToBottom,
  scrollToIndex,
  onPinnedChange,
}: {
  items: T[];
  rowHeight: number;
  render: (item: T, index: number) => ReactNode;
  className?: string;
  style?: React.CSSProperties;
  /** Follow new rows while the view is already at the bottom. */
  stickToBottom?: boolean;
  scrollToIndex?: number | null;
  /** The reader scrolled away from the bottom, or back to it. */
  onPinnedChange?: (pinned: boolean) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [height, setHeight] = useState(400);
  const pinned = useRef(true);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const observer = new ResizeObserver(() => setHeight(el.clientHeight));
    observer.observe(el);
    setHeight(el.clientHeight);
    return () => observer.disconnect();
  }, []);

  // Turning following on goes to the newest row at once.
  useEffect(() => {
    const el = ref.current;
    if (el && stickToBottom) {
      pinned.current = true;
      el.scrollTop = el.scrollHeight;
    }
  }, [stickToBottom]);

  useEffect(() => {
    const el = ref.current;
    if (el && stickToBottom && pinned.current) {
      el.scrollTop = el.scrollHeight;
    }
  }, [items, stickToBottom]);

  useEffect(() => {
    const el = ref.current;
    if (el && scrollToIndex != null && scrollToIndex >= 0) {
      const top = scrollToIndex * rowHeight;
      if (top < el.scrollTop || top + rowHeight > el.scrollTop + el.clientHeight) {
        el.scrollTop = Math.max(0, top - el.clientHeight / 2 + rowHeight / 2);
      }
    }
  }, [scrollToIndex, rowHeight]);

  const onScroll = (e: UIEvent<HTMLDivElement>) => {
    const el = e.currentTarget;
    setScrollTop(el.scrollTop);
    const atBottom = el.scrollTop + el.clientHeight >= el.scrollHeight - rowHeight * 1.5;
    if (atBottom !== pinned.current) onPinnedChange?.(atBottom);
    pinned.current = atBottom;
  };

  const overscan = 8;
  const start = Math.max(0, Math.floor(scrollTop / rowHeight) - overscan);
  const end = Math.min(items.length, Math.ceil((scrollTop + height) / rowHeight) + overscan);
  const visible = [];
  for (let i = start; i < end; i++) {
    visible.push(
      <div key={i} style={{ position: "absolute", top: i * rowHeight, left: 0, right: 0, height: rowHeight }}>
        {render(items[i], i)}
      </div>,
    );
  }
  return (
    <div ref={ref} className={`virtual${className ? ` ${className}` : ""}`} style={style} onScroll={onScroll}>
      <div className="virtual-inner" style={{ height: items.length * rowHeight }}>
        {visible}
      </div>
    </div>
  );
}
