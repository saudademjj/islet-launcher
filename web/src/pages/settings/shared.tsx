import { motion } from "motion/react";
import { useEffect, useRef } from "react";

import type { Theme } from "../../api/types";
import type { IconName } from "../../components/Icon";
import { Button, CardHeader, Segmented } from "../../components/ui";
import { rememberLanguage, t, type Language } from "../../lib/i18n";
import { item } from "../../lib/motion";
import { usePick, useStore } from "../../store";

export const REPOSITORY = "https://github.com/saudademjj/islet-launcher";

export const SECTIONS: { id: string; title: string; icon: IconName }[] = [
  { id: "game", title: t("游戏", "Game"), icon: "cpu" },
  { id: "download", title: t("下载", "Download"), icon: "download" },
  { id: "storage", title: t("存储", "Storage"), icon: "disk" },
  { id: "look", title: t("外观", "Appearance"), icon: "palette" },
  { id: "login", title: t("微软登录", "Microsoft sign-in"), icon: "shield" },
  { id: "about", title: t("关于", "About"), icon: "info" },
];

export const THEMES: { value: Theme; label: string }[] = [
  { value: "system", label: t("跟随系统", "System") },
  { value: "light", label: t("浅色", "Light") },
  { value: "dark", label: t("深色", "Dark") },
];

/** The launcher in miniature, in the theme's own colours: forest sidebar,
    the home stage cut down to the stone, the start button, three facts. */
export function ThemePreview() {
  return (
    <span className="tp">
      <span className="tp-side">
        <span className="tp-nav on" />
        <span className="tp-nav" />
        <span className="tp-nav" />
      </span>
      <span className="tp-main">
        <span className="tp-hero">
          <span className="tp-title" />
          <span className="tp-button" />
          <span className="tp-ground">
            <i className="g" />
            <i className="d" />
            <i className="s" />
          </span>
        </span>
        <span className="tp-facts">
          <i />
          <i />
          <i />
        </span>
      </span>
    </span>
  );
}

export function Presets({ values, current, label, onPick }: { values: number[]; current: number; label: (v: number) => string; onPick: (v: number) => void }) {
  return (
    <div className="presets">
      {values.map((v) => (
        <button key={v} type="button" className="preset mono" aria-pressed={current === v} onClick={() => onPick(v)}>
          {label(v)}
        </button>
      ))}
    </div>
  );
}

/** Interface language. Switching saves it and reloads the page, so every
    string (also those built when modules load) changes together. */
export function LanguageRow() {
  const s = usePick((s) => ({ settings: s.settings, busy: s.runningIds.length > 0 || s.install != null || s.fileBusy, toast: s.toast }));
  const choose = async (language: Language) => {
    if (language === s.settings.language) return;
    if (s.busy) return s.toast(t("请先关闭游戏、等安装或文件操作结束，再切换语言", "Close the game and wait for installs or file work to finish before switching the language"), "error");
    const saved = await useStore.getState().mutate("update_settings", { settings: { ...useStore.getState().settings, language } });
    if (!saved) return;
    rememberLanguage(language);
    window.location.reload();
  };
  return (
    <div className="setting-row">
      <div className="col">
        {/* Both names, so it can be found in either language. */}
        <span className="setting-name">{t("语言 · Language", "Language · 语言")}</span>
        <span className="field-hint">{t("切换后启动器会刷新一次", "The launcher refreshes once after switching")}</span>
      </div>
      <Segmented
        value={s.settings.language}
        onChange={(language) => void choose(language)}
        options={[
          { value: "system", label: t("跟随系统", "System") },
          { value: "zh", label: t("简体中文", "简体中文") },
          { value: "en", label: "English" },
        ]}
      />
    </div>
  );
}

/** Shown in the save bar when the player tried to switch pages, or to quit
    the launcher, with edits unsaved. */
export function LeaveQuestion({ count }: { count: number }) {
  const ref = useRef<HTMLDivElement>(null);
  const leavePage = useStore((s) => s.leavePage);
  const quitting = useStore((s) => s.leavingPage === "quit");
  // Enter saves, the way the bar's own button would.
  useEffect(() => ref.current?.querySelector<HTMLButtonElement>(".btn-primary")?.focus(), []);
  return (
    <div ref={ref} className="save-bar-question" role="alertdialog" aria-label={quitting ? t("退出前保存更改？", "Save changes before quitting?") : t("切换页面前保存更改？", "Save changes before leaving?")}>
      <span className="save-bar-mark" />
      <span className="save-bar-text">
        {quitting
          ? t(`退出前保存这 ${count} 项更改？`, `Save ${count} ${count === 1 ? "change" : "changes"} before quitting?`)
          : t(`离开前保存这 ${count} 项更改？`, `Save ${count} ${count === 1 ? "change" : "changes"} before leaving?`)}
      </span>
      <div className="spacer" />
      <Button variant="hero" size="sm" onClick={() => useStore.setState({ leavingPage: null })}>
        {quitting ? t("取消退出", "Don't quit") : t("继续编辑", "Keep editing")}
      </Button>
      <Button variant="hero" size="sm" onClick={() => void leavePage(false)}>
        {quitting ? t("放弃并退出", "Discard and quit") : t("放弃修改", "Discard")}
      </Button>
      <Button variant="primary" size="sm" icon="check" onClick={() => void leavePage(true)}>
        {quitting ? t("保存并退出", "Save and quit") : t("保存", "Save")}
      </Button>
    </div>
  );
}

export function Section({ id, icon, title, text, children }: { id: string; icon: IconName; title: string; text?: string; children: React.ReactNode }) {
  return (
    <motion.section id={`set-${id}`} className="card settings-section" variants={item}>
      <CardHeader icon={icon} title={title} text={text} />
      <div className="card-body col gap-20">
        {children}
      </div>
    </motion.section>
  );
}
