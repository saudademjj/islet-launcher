import { motion } from "motion/react";
import { Fragment, createContext, forwardRef, useContext, useId, type ButtonHTMLAttributes, type InputHTMLAttributes, type ReactNode } from "react";

import { identiconCells } from "../lib/identicon";
import { glide } from "../lib/motion";
import type { InstanceState } from "../store";
import { Icon, type IconName } from "./Icon";
import { PX, Sign, type SignMark } from "./Pixel";
import { t } from "../lib/i18n";

// ----- buttons ---------------------------------------------------------------

/** `hero` sits on the forest panels; the others on paper. `go` is the
    outlined green for repeated row actions (join, enter), so `primary`
    stays the one solid green in its area. */
type Variant = "default" | "primary" | "danger" | "ghost" | "danger-ghost" | "hero" | "go";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: "sm" | "md" | "lg" | "xl";
  icon?: IconName;
  busy?: boolean;
}

export function Button({ variant = "default", size = "md", icon, busy, className, children, disabled, ...rest }: ButtonProps) {
  const classes = ["btn"];
  if (variant !== "default") classes.push(`btn-${variant}`);
  if (size !== "md") classes.push(`btn-${size}`);
  if (!children) classes.push("btn-icon");
  if (className) classes.push(className);
  return (
    <button type="button" className={classes.join(" ")} disabled={disabled || busy} {...rest}>
      {busy ? <span className="spinner" /> : icon && <Icon name={icon} />}
      {children}
    </button>
  );
}

// ----- inputs ----------------------------------------------------------------

/** A Field's ids, so the control inside is named by its label and described
    by its hint without each caller wiring them up. */
const FieldContext = createContext<{ id: string; labelId: string; hintId?: string } | null>(null);

interface InputProps extends InputHTMLAttributes<HTMLInputElement> {
  mono?: boolean;
  invalid?: boolean;
  icon?: IconName;
}

export const Input = forwardRef<HTMLInputElement, InputProps>(function Input({ mono, invalid, icon, className, ...rest }, ref) {
  // Inside a Field the input takes the label's id, unless it names itself
  // (a second input in the same field must).
  const field = useContext(FieldContext);
  const input = (
    <input
      ref={ref}
      className={["input", mono && "mono", invalid && "invalid", className].filter(Boolean).join(" ")}
      spellCheck={false}
      autoCorrect="off"
      autoCapitalize="off"
      aria-invalid={invalid || undefined}
      {...rest}
      id={rest.id ?? (rest["aria-label"] ? undefined : field?.id)}
      aria-describedby={rest["aria-describedby"] ?? field?.hintId}
    />
  );
  if (!icon) return input;
  return (
    <div className="input-wrap">
      <Icon name={icon} />
      {input}
    </div>
  );
});

export function Field({ label, hint, tone, children, action }: { label: ReactNode; hint?: ReactNode; tone?: "ok" | "bad"; children: ReactNode; action?: ReactNode }) {
  const id = useId();
  const ids = { id: `${id}control`, labelId: `${id}label`, hintId: hint ? `${id}hint` : undefined };
  return (
    <FieldContext.Provider value={ids}>
      <div className="field">
        <div className="row" style={{ justifyContent: "space-between" }}>
          {/* A real label: clicking it focuses the input, and screen readers
              read it as the input's name. */}
          <label className="field-label" id={ids.labelId} htmlFor={ids.id}>
            {label}
          </label>
          {action}
        </div>
        {children}
        {hint && (
          <span id={ids.hintId} className={`field-hint${tone ? ` ${tone}` : ""}`}>
            {hint}
          </span>
        )}
      </div>
    </FieldContext.Provider>
  );
}

// ----- segmented control / tabs ---------------------------------------------

export function Segmented<T extends string>({ value, options, onChange, disabled }: { value: T; options: { value: T; label: string }[]; onChange: (v: T) => void; disabled?: boolean }) {
  const id = useId();
  // In a Field the choices are named by its label.
  const field = useContext(FieldContext);
  return (
    <div className="segmented" role="group" aria-labelledby={field?.labelId}>
      {options.map((o) => (
        <button key={o.value} type="button" aria-pressed={o.value === value} disabled={disabled} onClick={() => onChange(o.value)}>
          {o.value === value && <motion.span layoutId={`${id}-thumb`} className="segmented-thumb" transition={glide} />}
          {o.label}
        </button>
      ))}
    </div>
  );
}

/** `group` on a tab starts a new group: a short divider goes before it. */
export function Tabs<T extends string>({ value, tabs, onChange }: { value: T; tabs: { value: T; label: ReactNode; group?: boolean }[]; onChange: (v: T) => void }) {
  const id = useId();
  return (
    <div className="tabs" role="tablist">
      {tabs.map((tab) => (
        <Fragment key={tab.value}>
          {tab.group && <span className="tab-divider" aria-hidden />}
          <button type="button" role="tab" className="tab" aria-selected={tab.value === value} tabIndex={tab.value === value ? 0 : -1} onClick={() => onChange(tab.value)} onKeyDown={(e) => {
            // Arrow keys move between tabs, as in a native tab bar.
            if (e.key !== "ArrowRight" && e.key !== "ArrowLeft") return;
            const i = tabs.findIndex((x) => x.value === value);
            const next = tabs[(i + (e.key === "ArrowRight" ? 1 : tabs.length - 1)) % tabs.length];
            onChange(next.value);
            const buttons = e.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>('[role="tab"]');
            buttons?.[tabs.indexOf(next)]?.focus();
          }}>
            {tab.label}
            {tab.value === value && <motion.span layoutId={`${id}-tab`} className="tab-indicator" transition={glide} />}
          </button>
        </Fragment>
      ))}
    </div>
  );
}

/** A range input on a solid track; the filled part is its own block. */
export function Slider({ value, min, max, step = 1, onChange, disabled, label }: { value: number; min: number; max: number; step?: number; onChange: (v: number) => void; disabled?: boolean; label: string }) {
  const ratio = Math.min(1, Math.max(0, (value - min) / (max - min)));
  return (
    <span className={`slider${disabled ? " disabled" : ""}`}>
      <span className="slider-track" />
      <span className="slider-fill" style={{ width: `calc(${ratio} * (100% - 14px) + 7px)` }} />
      <input type="range" min={min} max={max} step={step} value={value} disabled={disabled} aria-label={label} onChange={(e) => onChange(Number(e.target.value))} />
    </span>
  );
}

export function Switch({ checked, onChange, label }: { checked: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <button type="button" role="switch" aria-checked={checked} aria-label={label} className="switch" onClick={() => onChange(!checked)}>
      <motion.span className="switch-knob" animate={{ x: checked ? 14 : 0 }} transition={glide} />
    </button>
  );
}

// ----- status, chips, keys ----------------------------------------------------

export type Tone = "accent" | "warn" | "danger" | "neutral";

export function Status({ tone, children, pulse }: { tone: Tone; children: ReactNode; pulse?: boolean }) {
  return <span className={`status tone-${tone}${pulse ? " pulse" : ""}`}>{children}</span>;
}

const STATE_LABEL: Record<InstanceState, [string, Tone, boolean]> = {
  running: [t("运行中", "Running"), "accent", true],
  starting: [t("启动中", "Starting"), "warn", true],
  installing: [t("安装中", "Installing"), "accent", true],
  queued: [t("排队中", "Queued"), "neutral", false],
  ready: [t("就绪", "Ready"), "neutral", false],
  notInstalled: [t("未安装", "Not installed"), "warn", false],
};

export function InstanceStatus({ state }: { state: InstanceState }) {
  const [label, tone, pulse] = STATE_LABEL[state];
  return (
    <Status tone={tone} pulse={pulse}>
      {label}
    </Status>
  );
}

export function Chip({ children }: { children: ReactNode }) {
  return <span className="chip">{children}</span>;
}

export function Kbd({ children }: { children: ReactNode }) {
  return <span className="kbd">{children}</span>;
}

// ----- progress ---------------------------------------------------------------

/** `ratio` null shows an indeterminate sweep. */
export function ProgressBar({ ratio, height = 6 }: { ratio: number | null; height?: number }) {
  return (
    <div className={`progress${ratio == null ? " indeterminate" : ""}`} style={{ height }}>
      <div className="progress-fill" style={ratio == null ? undefined : { transform: `scaleX(${ratio})` }} />
    </div>
  );
}

/** Progress as a row of blocks, for the forest hero. `grow` plays the
    finish once: each block turns from dirt to grass, left to right. */
export function BlockProgress({ ratio, cells = 32, grow }: { ratio: number; cells?: number; grow?: boolean }) {
  const filled = Math.round(Math.min(1, Math.max(0, ratio)) * cells);
  return (
    <div className={`blocks${grow ? " grow" : ""}`} role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(ratio * 100)}>
      {Array.from({ length: cells }, (_, i) => (
        <span key={i} className={i < filled ? (i === filled - 1 && !grow ? "on lead" : "on") : undefined} style={grow ? { animationDelay: `${i * 18}ms` } : undefined} />
      ))}
    </div>
  );
}

// ----- pixel art ----------------------------------------------------------------

/** Mirrored 5×5 pattern; `build` makes the cells appear one by one.
    `accent` marks the current item on paper, `hero` sits on forest. */
export function Identicon({ seed, size = 36, accent, hero, build }: { seed: string; size?: number; accent?: boolean; hero?: boolean; build?: boolean }) {
  const cells = identiconCells(seed);
  // Cells are whole multiples of the scenery pixel.
  const cell = Math.max(1, Math.round((size * 0.62) / 5 / PX)) * PX;
  const radius = size >= 40 ? 6 : size >= 28 ? 4 : 3;
  let order = 0;
  return (
    <div className={`identicon${accent ? " accent" : ""}${hero ? " hero" : ""}`} style={{ width: size, height: size, borderRadius: radius }}>
      <div className="identicon-grid" style={{ gridTemplateColumns: `repeat(5, ${cell}px)`, gridAutoRows: `${cell}px` }}>
        {cells.flatMap((row, y) =>
          row.map((on, x) => {
            if (!on) return <span key={`${x}-${y}`} />;
            const delay = build ? 0.12 + (order++) * 0.028 : 0;
            return build ? (
              <motion.span
                key={`${seed}-${x}-${y}`}
                className="identicon-cell"
                initial={{ opacity: 0, scale: 0.4 }}
                animate={{ opacity: 1, scale: 1 }}
                transition={{ delay, duration: 0.18, ease: [0.2, 0.8, 0.2, 1] }}
              />
            ) : (
              <span key={`${x}-${y}`} className="identicon-cell" />
            );
          }),
        )}
      </div>
    </div>
  );
}

/** The one card header: an icon tile, the title with an optional count,
    a line of explanation under it, and actions at the far end. */
export function CardHeader({ icon, title, meta, text, children }: { icon?: IconName; title: ReactNode; meta?: ReactNode; text?: ReactNode; children?: ReactNode }) {
  return (
    <div className="card-header">
      {icon && (
        <span className="card-icon">
          <Icon name={icon} />
        </span>
      )}
      <div className="card-heading">
        <span className="card-title">
          {title}
          {meta != null && <span className="card-meta">{meta}</span>}
        </span>
        {text && <span className="card-text">{text}</span>}
      </div>
      {children}
    </div>
  );
}

/** Small heading above a group: a pixel square, a label, optional trailing. */
export function SectionTitle({ children, trailing }: { children: ReactNode; trailing?: ReactNode }) {
  return (
    <div className="section-title">
      <span className="section-mark" />
      {children}
      <div className="spacer" />
      {trailing}
    </div>
  );
}

// ----- empty states & notices ------------------------------------------------

export function Empty({ icon, art, title, children, actions }: { icon?: IconName; art?: ReactNode; title: ReactNode; children?: ReactNode; actions?: ReactNode }) {
  return (
    <div className="empty">
      {art ??
        (icon && (
          <span className="empty-icon">
            <Icon name={icon} size={32} />
          </span>
        ))}
      <div className="empty-title">{title}</div>
      {children && <div style={{ maxWidth: 420 }}>{children}</div>}
      {actions && <div className="row" style={{ marginTop: 6 }}>{actions}</div>}
    </div>
  );
}

/** A raised card under a pixel signpost, its mark painted in the tone.
    `dark` is for the forest panels (the home stage); `detail` is the
    technical text, folded away behind the readable message. */
export function Notice({ tone, icon = "alert", title, children, actions, detail, dark }: { tone?: "warn" | "danger"; icon?: SignMark; title: ReactNode; children?: ReactNode; actions?: ReactNode; detail?: string | null; dark?: boolean }) {
  return (
    <div className={["notice", tone, dark && "dark"].filter(Boolean).join(" ")} role={tone === "danger" ? "alert" : undefined}>
      <Sign mark={icon} tone={tone ?? (icon === "check" ? "ok" : undefined)} />
      <div className="notice-title">{title}</div>
      {actions && <div className="notice-actions">{actions}</div>}
      {children && <div className="notice-body">{children}</div>}
      {detail && (
        <details className="notice-detail">
          <summary>{t("详细信息", "Details")}</summary>
          <pre className="selectable">{detail}</pre>
        </details>
      )}
    </div>
  );
}
