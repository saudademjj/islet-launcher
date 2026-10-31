import { useState } from "react";

import type { LaunchConfig } from "../api/types";
import { Icon } from "./Icon";
import { Field, Input } from "./ui";
import { t } from "../lib/i18n";

export const EMPTY_LAUNCH: LaunchConfig = { jvmArgs: "", gameArgs: "", windowWidth: null, windowHeight: null, env: "", wrapper: "", preLaunch: "", postExit: "" };

/** Problems the backend would refuse, said before saving. */
export function launchProblem(config: LaunchConfig): string | null {
  const quotes = (text: string) => {
    let quote: string | null = null;
    for (let i = 0; i < text.length; i++) {
      const c = text[i];
      if (c === "\\" && quote !== "'") i++;
      else if (quote ? c === quote : c === '"' || c === "'") quote = quote ? null : c;
    }
    return quote === null;
  };
  if (![config.jvmArgs, config.gameArgs, config.wrapper].every(quotes)) return t("参数里有没配对的引号", "Unmatched quote in the arguments");
  if ((config.windowWidth == null) !== (config.windowHeight == null)) return t("窗口宽度和高度需要一起填写", "Fill in both window width and height");
  for (const size of [config.windowWidth, config.windowHeight]) if (size != null && !(size >= 200 && size <= 16384)) return t("窗口宽高需要是 200–16384 之间的数字", "Window size must be a number from 200 to 16384");
  for (const line of config.env.split("\n").map((l) => l.trim())) {
    if (!line || line.startsWith("#")) continue;
    const at = line.indexOf("=");
    if (at <= 0 || /\s/.test(line.slice(0, at))) return t(`环境变量「${line}」需要写成 名称=值`, `Write the environment variable "${line}" as NAME=value`);
  }
  return null;
}

/** The launch options of a world or of every world. With `inherited`, empty
    fields say what they fall back to. Folded until opened, since few
    players need them. */
export function LaunchOptions({ value, onChange, disabled, inherited }: { value: LaunchConfig; onChange: (next: LaunchConfig) => void; disabled?: boolean; inherited?: LaunchConfig }) {
  const used = Object.entries(value).some(([, v]) => v != null && v !== "");
  const [open, setOpen] = useState(used);
  const set = (patch: Partial<LaunchConfig>) => onChange({ ...value, ...patch });
  const size = (text: string) => (text.trim() === "" ? null : Number.isFinite(Number(text)) ? Math.round(Number(text)) : NaN);
  const problem = launchProblem(value);
  const inheritedWindow = inherited?.windowWidth ? `${inherited.windowWidth}×${inherited.windowHeight}` : null;
  return (
    <div className="launch-options">
      <button type="button" className="launch-options-toggle" aria-expanded={open} onClick={() => setOpen(!open)}>
        <Icon name={open ? "chevronDown" : "chevronRight"} />
        {t("高级启动选项", "Advanced launch options")}
        {used && !open && <span className="chip">{t("已设置", "Set")}</span>}
      </button>
      {open && (
        <div className="col gap-16">
          <Field label={t("游戏窗口大小", "Game window size")} hint={inheritedWindow ? t(`留空使用全局的 ${inheritedWindow}`, `Leave empty for the global ${inheritedWindow}`) : t("留空使用游戏默认的 854×480；全屏在「游戏选项」里切换", "Leave empty for the game's default 854×480; fullscreen is under Game options")}>
            <div className="row">
              <Input mono style={{ width: 100 }} disabled={disabled} placeholder={t("宽", "Width")} value={value.windowWidth ?? ""} onChange={(e) => set({ windowWidth: size(e.target.value) })} />
              <span className="muted">×</span>
              <Input mono style={{ width: 100 }} disabled={disabled} placeholder={t("高", "Height")} aria-label={t("游戏窗口高度", "Game window height")} value={value.windowHeight ?? ""} onChange={(e) => set({ windowHeight: size(e.target.value) })} />
              {[
                [1280, 720],
                [1600, 900],
                [1920, 1080],
              ].map(([w, h]) => (
                <button key={w} type="button" className="filter-chip" disabled={disabled} onClick={() => set({ windowWidth: w, windowHeight: h })}>
                  {w}×{h}
                </button>
              ))}
            </div>
          </Field>
          <Field label={t("JVM 参数", "JVM arguments")} hint={inherited?.jvmArgs ? t(`加在全局参数「${inherited.jvmArgs}」后面`, `Added after the global "${inherited.jvmArgs}"`) : t("例如 -XX:+UseZGC；写 -Xmx 会覆盖内存设置", "e.g. -XX:+UseZGC; an -Xmx here overrides the memory setting")}>
            <Input mono disabled={disabled} value={value.jvmArgs} placeholder="-XX:+UseG1GC" onChange={(e) => set({ jvmArgs: e.target.value })} />
          </Field>
          <Field label={t("游戏参数", "Game arguments")} hint={t("加在游戏参数最后，例如 --demo", "Added after the game's arguments, e.g. --demo")}>
            <Input mono disabled={disabled} value={value.gameArgs} onChange={(e) => set({ gameArgs: e.target.value })} />
          </Field>
          <Field label={t("包装命令", "Wrapper command")} hint={inherited?.wrapper ? t(`留空使用全局的「${inherited.wrapper}」`, `Leave empty for the global "${inherited.wrapper}"`) : t("游戏在这个命令里运行，例如 Linux 上的 gamemoderun、prime-run", "The game runs inside this command, e.g. gamemoderun or prime-run on Linux")}>
            <Input mono disabled={disabled} value={value.wrapper} onChange={(e) => set({ wrapper: e.target.value })} />
          </Field>
          <Field label={t("启动前命令", "Pre-launch command")} hint={inherited?.preLaunch ? t(`留空使用全局的命令`, `Leave empty for the global command`) : t("每次启动前在游戏目录里运行并等它结束，失败则不启动；输出写进游戏日志", "Runs in the game folder before each start and is waited for; if it fails the game doesn't start. Output goes to the game log")}>
            <Input mono disabled={disabled} value={value.preLaunch} placeholder={t("例如 ./sync-config.sh", "e.g. ./sync-config.sh")} onChange={(e) => set({ preLaunch: e.target.value })} />
          </Field>
          <Field label={t("退出后命令", "Post-exit command")} hint={inherited?.postExit ? t(`留空使用全局的命令`, `Leave empty for the global command`) : t("游戏关闭后在游戏目录里运行，退出码在 MC_EXIT_CODE 里；输出写进游戏日志", "Runs in the game folder after the game closes, with its exit code in MC_EXIT_CODE. Output goes to the game log")}>
            <Input mono disabled={disabled} value={value.postExit} placeholder={t("例如 ./backup-saves.sh", "e.g. ./backup-saves.sh")} onChange={(e) => set({ postExit: e.target.value })} />
          </Field>
          <Field label={t("环境变量", "Environment variables")} hint={t("每行一个 名称=值；# 开头的行是注释", "One NAME=value per line; lines starting with # are comments")}>
            <textarea className="input mono launch-env" aria-label={t("环境变量", "Environment variables")} disabled={disabled} value={value.env} rows={3} spellCheck={false} onChange={(e) => set({ env: e.target.value })} />
          </Field>
          {problem && (
            <span className="field-hint bad">
              {problem}
            </span>
          )}
        </div>
      )}
    </div>
  );
}
