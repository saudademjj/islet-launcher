import { useEffect, useMemo, useState, type ReactNode } from "react";

import { call, toError } from "../api/backend";
import type { GameOptions, Instance } from "../api/types";
import { Button, Notice, Segmented, Slider, Switch } from "../components/ui";
import { useFormGuard } from "../lib/formGuard";
import { usePick } from "../store";
import { t } from "../lib/i18n";

interface Form {
  lang: string;
  renderDistance: number;
  maxFps: number;
  guiScale: number;
  fov: number;
  fullscreen: boolean;
  vsync: boolean;
  volume: number;
}

const LANGUAGES = [
  { value: "zh_cn", label: t("简体中文", "简体中文") },
  { value: "zh_tw", label: t("繁體中文", "繁體中文") },
  { value: "en_us", label: "English" },
  { value: "ja_jp", label: t("日本語", "日本語") },
];

/** The game's defaults for options the file does not set. */
function toForm(options: GameOptions, autoChinese: boolean): Form {
  return {
    lang: options.lang ?? (autoChinese ? "zh_cn" : "en_us"),
    renderDistance: options.renderDistance ?? 12,
    maxFps: options.maxFps ?? 120,
    guiScale: options.guiScale ?? 0,
    fov: options.fov ?? 70,
    fullscreen: options.fullscreen ?? false,
    vsync: options.vsync ?? true,
    volume: options.volume ?? 100,
  };
}

const same = (a: Form, b: Form) => (Object.keys(a) as (keyof Form)[]).every((key) => a[key] === b[key]);

const fovName = (fov: number) => (fov === 70 ? t("普通", "Normal") : fov === 110 ? t("极速", "Quake Pro") : fov < 70 ? t("窄", "Narrow") : t("广", "Wide"));

/** A top-down view of the loaded area: the lit square grows with the render
    distance, the center block is the player. */
function ChunkView({ distance }: { distance: number }) {
  const size = 11;
  const radius = Math.max(1, Math.round((distance / 32) * 5));
  const mid = (size - 1) / 2;
  return (
    <span className="chunk-view" aria-hidden style={{ gridTemplateColumns: `repeat(${size}, 1fr)` }}>
      {Array.from({ length: size * size }, (_, i) => {
        const x = (i % size) - mid;
        const y = Math.floor(i / size) - mid;
        const lit = Math.max(Math.abs(x), Math.abs(y)) <= radius;
        return <i key={i} className={x === 0 && y === 0 ? "me" : lit ? "lit" : undefined} />;
      })}
    </span>
  );
}

function Row({ label, hint, value, children }: { label: string; hint?: string; value?: ReactNode; children: ReactNode }) {
  return (
    <div className="opt-row">
      <div className="col" style={{ gap: 2, minWidth: 0 }}>
        <span className="opt-name">{label}</span>
        {hint && <span className="opt-hint">{hint}</span>}
      </div>
      <div className="opt-control">{children}</div>
      {value !== undefined && <span className="opt-value mono">{value}</span>}
    </div>
  );
}

/** The game's own options.txt: language, view and sound, saved for the
    next launch. */
export function OptionsTab({ instance, busy }: { instance: Instance; busy: boolean }) {
  const s = usePick((s) => ({ autoChinese: s.settings.autoChinese, toast: s.toast }));
  const [saved, setSaved] = useState<GameOptions | null>(null);
  const [form, setForm] = useState<Form | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let live = true;
    call<GameOptions>("read_game_options", { id: instance.id })
      .then((options) => {
        if (!live) return;
        setSaved(options);
        setForm(toForm(options, s.autoChinese));
      })
      .catch((error) => live && s.toast(toError(error).message, "error"));
    return () => {
      live = false;
    };
  }, [instance.id, instance.lastLaunchedAt]);

  const base = useMemo(() => (saved ? toForm(saved, s.autoChinese) : null), [saved, s.autoChinese]);
  const save = async () => {
    if (!form) return false;
    setSaving(true);
    try {
      const next = await call<GameOptions>("write_game_options", { id: instance.id, options: form });
      setSaved(next);
      setForm(toForm(next, s.autoChinese));
      s.toast(t("游戏选项已保存，下次启动生效", "Game options saved; they apply from the next launch"), "success");
      return true;
    } catch (error) {
      s.toast(toError(error).message, "error");
      return false;
    } finally {
      setSaving(false);
    }
  };
  // Only real edits ask before leaving; a file that doesn't exist yet doesn't.
  useFormGuard(!!form && !!base && !same(form, base), save);
  if (!form || !base || !saved) {
    return (
      <div className="manage-scroll">
        <div className="saves-empty muted">
          <span className="spinner" />{t(" 正在读取游戏选项…", " Reading game options…")}
        </div>
      </div>
    );
  }
  const dirty = !same(form, base) || !saved.exists;
  const set = (patch: Partial<Form>) => setForm({ ...form, ...patch });
  const languages = LANGUAGES.some((l) => l.value === form.lang) ? LANGUAGES : [...LANGUAGES, { value: form.lang, label: form.lang }];

  return (
    <>
      <div className="manage-scroll">
        {!saved.exists && (
          <Notice icon="info" title={t("这个世界还没有启动过", "This world hasn't been started yet")}>
            {t("还没有 options.txt。保存后，第一次启动就会使用这里的设置", "There's no options.txt yet. Save, and the first start uses these settings")}{s.autoChinese ? t("；不保存也会自动使用简体中文", "; without saving, the game starts in Simplified Chinese") : ""}{t("。", ".")}
          </Notice>
        )}
        <div className="opt-group">
          <div className="opt-group-title">{t("语言与界面", "Language & interface")}</div>
          <Row label={t("游戏语言", "Game language")} hint={t("1.11 以前的版本会自动换成旧格式", "Versions before 1.11 get the old format automatically")}>
            <Segmented value={form.lang} options={languages} disabled={busy} onChange={(lang) => set({ lang })} />
          </Row>
          <Row label={t("界面缩放", "GUI scale")} hint={t("按钮和文字的大小；自动会按窗口大小选择", "Size of buttons and text; Auto picks by window size")}>
            <Segmented
              value={String(form.guiScale)}
              disabled={busy}
              options={[
                { value: "0", label: t("自动", "Auto") },
                { value: "1", label: "1×" },
                { value: "2", label: "2×" },
                { value: "3", label: "3×" },
                { value: "4", label: "4×" },
              ]}
              onChange={(value) => set({ guiScale: Number(value) })}
            />
          </Row>
        </div>

        <div className="opt-group">
          <div className="opt-group-title">{t("画面", "Video")}</div>
          <Row label={t("渲染距离", "Render distance")} hint={t("越远越吃内存和显卡；低配电脑建议 8 以内", "Farther uses more memory and GPU; keep it at 8 or less on modest computers")} value={t(`${form.renderDistance} 区块`, `${form.renderDistance} chunks`)}>
            <div className="row" style={{ gap: 14, width: "100%" }}>
              <Slider label={t("渲染距离", "Render distance")} min={2} max={32} value={form.renderDistance} disabled={busy} onChange={(renderDistance) => set({ renderDistance })} />
              <ChunkView distance={form.renderDistance} />
            </div>
          </Row>
          <Row label={t("视场角", "Field of view")} hint={t("70 是默认值", "70 is the default")} value={`${form.fov}° · ${fovName(form.fov)}`}>
            <Slider label={t("视场角", "Field of view")} min={30} max={110} value={form.fov} disabled={busy} onChange={(fov) => set({ fov })} />
          </Row>
          <Row label={t("最大帧率", "Max frame rate")} value={form.maxFps >= 260 ? t("无限制", "Unlimited") : t(`${form.maxFps} 帧`, `${form.maxFps} fps`)}>
            <Slider label={t("最大帧率", "Max frame rate")} min={10} max={260} step={10} value={form.maxFps} disabled={busy} onChange={(maxFps) => set({ maxFps })} />
          </Row>
          <Row label={t("全屏", "Fullscreen")} hint={t("也可以在游戏里按 F11 切换", "You can also press F11 in game")}>
            <Switch label={t("全屏", "Fullscreen")} checked={form.fullscreen} onChange={(fullscreen) => !busy && set({ fullscreen })} />
          </Row>
          <Row label={t("垂直同步", "VSync")} hint={t("消除画面撕裂；开启后帧率不超过屏幕刷新率", "Stops tearing; the frame rate then stays at the screen's refresh rate")}>
            <Switch label={t("垂直同步", "VSync")} checked={form.vsync} onChange={(vsync) => !busy && set({ vsync })} />
          </Row>
        </div>

        <div className="opt-group">
          <div className="opt-group-title">{t("声音", "Sound")}</div>
          <Row label={t("主音量", "Master volume")} value={form.volume === 0 ? t("静音", "Muted") : `${form.volume}%`}>
            <Slider label={t("主音量", "Master volume")} min={0} max={100} step={5} value={form.volume} disabled={busy} onChange={(volume) => set({ volume })} />
          </Row>
        </div>
      </div>
      <div className="modal-footer">
        <span className="muted" style={{ fontSize: "var(--text-xs)", marginRight: "auto" }}>
          {t("写入游戏目录的 options.txt；其他游戏设置保持不变", "Written to options.txt in the game folder; other game settings stay as they are")}
        </span>
        <Button variant="ghost" disabled={busy || same(form, base)} onClick={() => setForm(base)}>
          {t("还原", "Revert")}
        </Button>
        <Button variant="primary" icon="check" disabled={busy || !dirty} busy={saving} onClick={() => void save()}>
          {t("保存", "Save")}
        </Button>
      </div>
    </>
  );
}
