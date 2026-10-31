import { AnimatePresence, motion } from "motion/react";
import { useEffect, useMemo, useRef, useState } from "react";

import { call, toError } from "../api/backend";
import type { Cleanup, DownloadSource, LaunchConfig, Settings, StorageUsage, Theme } from "../api/types";
import { Icon, type IconName } from "../components/Icon";
import { Button, Field, Input, Segmented, Switch } from "../components/ui";
import { JavaPicker } from "../components/JavaPicker";
import { LaunchOptions, launchProblem } from "../components/LaunchOptions";
import { rememberLanguage, t, type Language } from "../lib/i18n";
import { formatBytes, validGuid } from "../lib/format";
import { glide, item, normal, stagger } from "../lib/motion";
import { MOD_KEY } from "../lib/platform";
import { usePrefs } from "../lib/prefs";
import { matchItem, queryWords } from "../lib/search";
import { Toolbar } from "../layout/Shell";
import { usePick, useStore } from "../store";

const AZURE_PORTAL = "https://portal.azure.com/#view/Microsoft_AAD_RegisteredApps/ApplicationsListBlade";

interface Form {
  memory: string;
  memoryAuto: boolean;
  concurrency: string;
  clientId: string;
  javaPath: string;
  downloadSource: DownloadSource;
  curseforgeKey: string;
  proxyMode: Settings["proxyMode"];
  proxyUrl: string;
  launch: LaunchConfig;
}

const toForm = (s: Settings): Form => ({
  memory: String(s.memoryMb),
  memoryAuto: s.memoryAuto,
  concurrency: String(s.maxConcurrentDownloads),
  clientId: s.clientId,
  javaPath: s.javaPath ?? "",
  downloadSource: s.downloadSource,
  curseforgeKey: s.curseforgeKey,
  proxyMode: s.proxyMode,
  proxyUrl: s.proxyUrl,
  launch: s.launch,
});

/** Whether one field differs; text fields ignore surrounding spaces. */
const fieldChanged = (a: Form, b: Form, key: keyof Form) =>
  key === "launch" ? JSON.stringify(a.launch) !== JSON.stringify(b.launch) : String(a[key]).trim() !== String(b[key]).trim();

const sameForm = (a: Form, b: Form) => (Object.keys(a) as (keyof Form)[]).every((key) => !fieldChanged(a, b, key));

/** http(s):// or socks5(h):// with a host; SOCKS needs a port too. */
const validProxy = (url: string) => /^(https?:\/\/[^\s/:@]+(:\d{1,5})?|socks5h?:\/\/[^\s/:@]+:\d{1,5})\/?$/i.test(url.replace(/\/\/[^\s/@]*@/, "//"));

/** `2.5 GB` for uneven totals, `8 GB` for whole gigabytes. */
const formatGb = (mb: number) => (mb % 1024 === 0 ? String(mb / 1024) : (mb / 1024).toFixed(1));

export function SettingsPage() {
  const s = usePick((s) => ({
    settings: s.settings,
    appInfo: s.appInfo,
    systemMemory: s.systemMemory,
    fileBusy: s.fileBusy,
    showAzureHelp: s.showAzureHelp,
    loginConfigured: s.loginConfigured,
    reduceMotion: s.reduceMotion,
    mutate: s.mutate,
    toast: s.toast,
    open: s.open,
    openUrl: s.openUrl,
    relocate: s.relocate,
    chooseFiles: s.chooseFiles,
    checkLogin: s.checkLogin,
    setReduceMotion: s.setReduceMotion,
  }));
  const saved = useMemo(() => toForm(s.settings), [s.settings]);
  const [form, setForm] = useState(saved);
  const dirty = !sameForm(form, saved);

  // Unsaved edits survive outside changes (theme, active account); a clean
  // form follows the saved settings.
  const previous = useRef(saved);
  useEffect(() => {
    setForm((current) => (sameForm(current, previous.current) ? saved : current));
    previous.current = saved;
  }, [saved]);

  const set = (patch: Partial<Form>) => setForm((f) => ({ ...f, ...patch }));

  // Section index: follows the scroll position, jumps on click.
  const scroller = useRef<HTMLDivElement>(null);
  const [current, setCurrent] = useState(SECTIONS[0].id);
  const onScroll = () => {
    const el = scroller.current;
    if (!el) return;
    let id = SECTIONS[0].id;
    for (const sec of SECTIONS) {
      const node = el.querySelector<HTMLElement>(`#set-${sec.id}`);
      if (node && node.offsetTop - el.scrollTop <= 140) id = sec.id;
    }
    if (el.scrollTop + el.clientHeight >= el.scrollHeight - 4) id = SECTIONS[SECTIONS.length - 1].id;
    setCurrent(id);
  };
  const jump = (id: string) => {
    scroller.current?.querySelector(`#set-${id}`)?.scrollIntoView({ behavior: s.reduceMotion ? "auto" : "smooth", block: "start" });
  };
  // Search reads the rendered rows (each direct child of a section's body),
  // so every setting can be found without keeping a second list in step.
  // A match stays and is marked; the rest of the page steps out of the way.
  const [query, setQuery] = useState("");
  // A release with Microsoft sign-in built in keeps the Azure setup out of
  // the way; players only need it to use an app of their own.
  const builtInLogin = s.loginConfigured && s.settings.clientId === "";
  const [ownApp, setOwnApp] = useState(false);
  const ownAppShown = !builtInLogin || ownApp || query.trim() !== "";
  const [hits, setHits] = useState<Record<string, number> | null>(null);
  useEffect(() => {
    const root = scroller.current;
    if (!root) return;
    const words = queryWords(query);
    const sections = [...root.querySelectorAll<HTMLElement>(".settings-section")];
    if (words.length === 0) {
      root.querySelectorAll(".search-hit, .search-miss").forEach((el) => el.classList.remove("search-hit", "search-miss"));
      setHits(null);
      return;
    }
    const counts: Record<string, number> = {};
    for (const section of sections) {
      const id = section.id.replace("set-", "");
      // Only the name brings the whole section; its description lists a few
      // of the rows and would bring the rest along with them.
      const title = section.querySelector(".settings-head .card-title")?.textContent ?? "";
      const whole = matchItem(title, [], words) !== null;
      let count = 0;
      for (const row of section.querySelectorAll<HTMLElement>(":scope > .card-body > *")) {
        const label = row.querySelector(".field-label, .setting-name")?.textContent ?? "";
        const hit = matchItem(label, [row.textContent ?? ""], words) !== null;
        row.classList.toggle("search-hit", hit);
        row.classList.toggle("search-miss", !hit && !whole);
        if (hit) count++;
      }
      section.classList.toggle("search-miss", count === 0 && !whole);
      counts[id] = whole ? Math.max(count, 1) : count;
    }
    setHits(counts);
    root.scrollTop = 0;
    const first = SECTIONS.find((sec) => counts[sec.id] > 0);
    if (first) setCurrent(first.id);
    // The rows change with the form; their text is what is searched.
  }, [query, form, s.settings]);
  const found = hits ? Object.values(hits).reduce((a, b) => a + b, 0) : 0;

  // The command palette can ask for a section; go there once the page is up.
  const focus = useStore((state) => state.settingsSection);
  useEffect(() => {
    if (!focus) return;
    const timer = setTimeout(() => {
      jump(focus);
      useStore.setState({ settingsSection: null });
    }, 80);
    return () => clearTimeout(timer);
  }, [focus]);

  const memory = Number(form.memory.trim());
  const memoryOk = Number.isInteger(memory) && memory >= 512 && memory <= 32768;
  const concurrency = Number(form.concurrency.trim());
  const concurrencyOk = Number.isInteger(concurrency) && concurrency >= 1 && concurrency <= 64;
  const clientId = form.clientId.trim();
  const clientOk = clientId === "" || validGuid(clientId);
  const proxyOk = form.proxyMode !== "custom" || validProxy(form.proxyUrl.trim());

  const fail = (message: string) => {
    s.toast(message, "error");
    return false;
  };
  /** Saves every field on the page; false when something is invalid or the save failed. */
  const save = async (): Promise<boolean> => {
    if (!form.memoryAuto && !memoryOk) return fail(t("内存需要是 512–32768 之间的整数（MB）", "Memory must be a whole number from 512 to 32768 (MB)"));
    if (!concurrencyOk) return fail(t("下载并发数需要在 1–64 之间", "Download concurrency must be between 1 and 64"));
    if (!clientOk) return fail(t("Client ID 格式不正确，应为 GUID", "The Client ID must be a GUID"));
    if (!proxyOk) return fail(t("代理地址应类似 http://127.0.0.1:7890 或 socks5://127.0.0.1:7891", "The proxy should look like http://127.0.0.1:7890 or socks5://127.0.0.1:7891"));
    const problem = launchProblem(form.launch);
    if (problem) return fail(problem);
    const next: Settings = {
      ...s.settings,
      // Keep the manually entered value around for when automatic mode is
      // switched off later, but never persist an invalid hidden input.
      memoryMb: memoryOk ? memory : s.settings.memoryMb,
      memoryAuto: form.memoryAuto,
      maxConcurrentDownloads: concurrency,
      clientId,
      javaPath: form.javaPath.trim() || null,
      downloadSource: form.downloadSource,
      curseforgeKey: form.curseforgeKey.trim(),
      proxyMode: form.proxyMode,
      proxyUrl: form.proxyUrl.trim(),
      launch: form.launch,
    };
    if (!(await s.mutate("update_settings", { settings: next }, t("设置已保存", "Settings saved")))) return false;
    setForm(toForm(useStore.getState().settings));
    void s.checkLogin();
    return true;
  };
  // Switching pages with unsaved edits asks first, the way a world's
  // manage dialog does.
  const saveLatest = useRef(save);
  saveLatest.current = save;
  useEffect(() => {
    useStore.getState().setPageGuard(dirty ? { save: () => saveLatest.current(), discard: () => setForm(toForm(useStore.getState().settings)) } : null);
  }, [dirty]);
  useEffect(() => () => useStore.getState().setPageGuard(null), []);
  const leaving = useStore((state) => state.leavingPage);

  const setTheme = (theme: Theme) => void s.mutate("update_settings", { settings: { ...s.settings, theme } });
  const prefs = usePrefs();

  // ⌘S saves while there is something to save.
  useEffect(() => {
    if (!dirty) return;
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "s") {
        e.preventDefault();
        void saveLatest.current();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [dirty]);
  const changes = (Object.keys(saved) as (keyof Form)[]).filter((key) => fieldChanged(form, saved, key)).length;

  return (
    <>
      <Toolbar title={t("设置", "Settings")} meta={dirty ? <span className="tone-warn">{t("有未保存的更改", "Unsaved changes")}</span> : t("开关和外观立即生效；其余改动在底部一起保存", "Switches and appearance apply at once; other edits are saved together at the bottom")}>
        <div className="settings-search">
          <Input
            icon="search"
            placeholder={t("搜索设置", "Search settings")}
            aria-label={t("搜索设置", "Search settings")}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape" && query) {
                e.stopPropagation();
                setQuery("");
              } else if (e.key === "Enter") {
                scroller.current?.querySelector(".search-hit")?.scrollIntoView({ behavior: s.reduceMotion ? "auto" : "smooth", block: "center" });
              }
            }}
          />
        </div>
      </Toolbar>
      {/* Unsaved edits get a bar that cannot be missed; the toolbar stays quiet. */}
      <AnimatePresence>
        {dirty && (
          <motion.div
            className="save-bar"
            role="region"
            aria-label={t("未保存的更改", "Unsaved changes")}
            initial={{ y: 70, opacity: 0 }}
            animate={{ y: 0, opacity: 1, transition: normal }}
            exit={{ y: 70, opacity: 0, transition: { duration: 0.16 } }}
          >
            {leaving ? (
              <LeaveQuestion count={changes} />
            ) : (
              <>
                <span className="save-bar-mark" />
                <span className="save-bar-text">{changes}{t(" 项更改未保存", " unsaved changes")}</span>
                <span className="kbd">{MOD_KEY}S</span>
                <div className="spacer" />
                <Button variant="hero" size="sm" onClick={() => setForm(saved)}>
                  {t("还原", "Revert")}
                </Button>
                <Button variant="primary" size="sm" icon="check" onClick={() => void save()}>
                  {t("保存", "Save")}
                </Button>
              </>
            )}
          </motion.div>
        )}
      </AnimatePresence>
      <div className={`content${hits ? " settings-searching" : ""}`} ref={scroller} onScroll={onScroll}>
        <div className="page settings-layout">
          <nav className="settings-index" aria-label={t("设置分区", "Settings sections")}>
            {SECTIONS.map((sec) => (
              <button
                key={sec.id}
                type="button"
                className="index-item"
                aria-current={current === sec.id ? "true" : undefined}
                disabled={hits ? !hits[sec.id] : false}
                onClick={() => jump(sec.id)}
              >
                {current === sec.id && <motion.span layoutId="settings-index" className="index-highlight" transition={glide} />}
                <Icon name={sec.icon} size={15} />
                {sec.title}
                {hits && hits[sec.id] > 0 && <span className="index-count">{hits[sec.id]}</span>}
              </button>
            ))}
          </nav>
        <motion.div className="settings" variants={stagger} initial="initial" animate="animate">
          {hits && found === 0 && (
            <div className="settings-none">
              {t(`没有找到和「${query.trim()}」有关的设置`, `No settings match "${query.trim()}"`)}
              <Button size="sm" variant="ghost" onClick={() => setQuery("")}>
                {t("清除搜索", "Clear search")}
              </Button>
            </div>
          )}
          <Section id="game" icon="cpu" title={t("游戏", "Game")} text={t("新世界默认使用的内存、Java 和启动行为", "Memory, Java and launch behavior for worlds")}>
            <Field
              label={t("默认游戏内存", "Default game memory")}
              hint={
                form.memoryAuto
                  ? t("按本机物理内存和模组数量分配；世界单独设置的内存仍然优先", "Set from this computer's memory and the mod count; a world's own setting still wins")
                  : memoryOk
                    ? t("世界可以单独覆盖；建议不超过本机内存的一半", "Worlds can override it; keep it under half of this computer's memory")
                    : t("需要是 512–32768 之间的整数（MB）", "Must be a whole number from 512 to 32768 (MB)")
              }
              tone={form.memoryAuto || memoryOk ? undefined : "bad"}
            >
              <div className="col" style={{ gap: 10 }}>
                <div>
                  <Segmented
                    value={form.memoryAuto ? "auto" : "manual"}
                    options={[
                      { value: "auto", label: t("自动", "Auto") },
                      { value: "manual", label: t("手动", "Manual") },
                    ]}
                    onChange={(mode) => set({ memoryAuto: mode === "auto" })}
                  />
                </div>
                {form.memoryAuto ? (
                  <span className="muted" style={{ fontSize: "var(--text-sm)" }}>
                    {s.systemMemory
                      ? t(`推荐 ${formatGb(s.systemMemory.recommendedMb)} GB · 本机 ${formatGb(s.systemMemory.totalMb)} GB 内存`, `Recommended ${formatGb(s.systemMemory.recommendedMb)} GB · ${formatGb(s.systemMemory.totalMb)} GB installed`)
                      : t("正在读取本机内存…", "Reading memory…")}
                  </span>
                ) : (
                  <div className="row">
                    <Input mono value={form.memory} invalid={!memoryOk} onChange={(e) => set({ memory: e.target.value })} style={{ width: 120 }} />
                    <span className="muted">MB</span>
                    <div className="spacer" />
                    <Presets values={[2048, 4096, 6144, 8192]} current={Number(form.memory)} label={(mb) => `${mb / 1024} G`} onPick={(mb) => set({ memory: String(mb) })} />
                  </div>
                )}
              </div>
            </Field>
            <div className="setting-row">
              <div className="col">
                <span className="setting-name">{t("新世界使用启动器的语言", "New worlds use the launcher's language")}</span>
                <span className="field-hint">{t("第一次启动前把游戏语言设成和启动器一样；已有的游戏设置不受影响", "Sets the game language to match the launcher before the first launch; existing game settings are left alone")}</span>
              </div>
              <Switch
                label={t("新世界使用启动器的语言", "New worlds use the launcher's language")}
                checked={s.settings.autoChinese}
                onChange={(autoChinese) => void s.mutate("update_settings", { settings: { ...s.settings, autoChinese } }, autoChinese ? t("新世界会使用启动器的语言", "New worlds will use the launcher's language") : t("新世界不再自动设置语言", "New worlds no longer get a language set"))}
              />
            </div>
            <div className="setting-row">
              <div className="col">
                <span className="setting-name">{t("游戏退出后自动备份存档", "Back up saves after playing")}</span>
                <span className="field-hint">
                  {t(
                    `只备份这次玩过的存档，不用等；每个存档保留最近 ${s.settings.autoBackupKeep} 份自动备份，手动备份不会被删`,
                    `Only the saves you just played, in the background. The newest ${s.settings.autoBackupKeep} automatic backups of each save are kept; ones you make yourself never go`,
                  )}
                </span>
              </div>
              {s.settings.autoBackup && (
                <Presets
                  values={[3, 5, 10, 20]}
                  current={s.settings.autoBackupKeep}
                  label={(n) => String(n)}
                  onPick={(autoBackupKeep) => void s.mutate("update_settings", { settings: { ...s.settings, autoBackupKeep } }, t(`每个存档保留 ${autoBackupKeep} 份自动备份`, `Keeping ${autoBackupKeep} automatic backups per save`))}
                />
              )}
              <Switch
                label={t("游戏退出后自动备份存档", "Back up saves after playing")}
                checked={s.settings.autoBackup}
                onChange={(autoBackup) => void s.mutate("update_settings", { settings: { ...s.settings, autoBackup } }, autoBackup ? t("游戏退出后会自动备份玩过的存档", "Saves you play will be backed up after the game exits") : t("不再自动备份存档", "Saves are no longer backed up automatically"))}
              />
            </div>
            {s.appInfo.platform !== "macos" && (
              <div className="setting-row">
                <div className="col">
                  <span className="setting-name">{t("用独立显卡运行游戏", "Play on the discrete GPU")}</span>
                  <span className="field-hint">
                    {s.appInfo.platform === "windows"
                      ? t("有两块显卡的笔记本上，让 Java 用性能更好的那块，帧数通常高得多；只有一块显卡时没有影响", "On laptops with two GPUs, Java runs on the faster one, usually for far more frames; no effect with a single GPU")
                      : t("检测到两块显卡时设置 DRI_PRIME=1；NVIDIA 专有驱动请在启动选项里用 prime-run", "Sets DRI_PRIME=1 when two GPUs are found; with NVIDIA's own driver, use prime-run as the wrapper in launch options")}
                  </span>
                </div>
                <Switch
                  label={t("用独立显卡运行游戏", "Play on the discrete GPU")}
                  checked={s.settings.dedicatedGpu}
                  onChange={(dedicatedGpu) => void s.mutate("update_settings", { settings: { ...s.settings, dedicatedGpu } }, dedicatedGpu ? t("下次开始游戏时使用独立显卡", "The discrete GPU is used from the next start") : t("不再指定显卡", "The GPU is no longer chosen"))}
                />
              </div>
            )}
            <div className="setting-row">
              <div className="col">
                <span className="setting-name">{t("游戏启动后隐藏启动器", "Hide the launcher while playing")}</span>
                <span className="field-hint">{t("游戏窗口出现后才收起，可从托盘、任务栏或程序坞恢复；游戏退出后自动回来", "Steps aside once the game's window is up. Restore it from the tray, taskbar or Dock; it returns when the game exits")}</span>
              </div>
              <Switch
                label={t("游戏启动后隐藏启动器", "Hide the launcher while playing")}
                checked={s.settings.hideOnLaunch}
                onChange={(hideOnLaunch) => void s.mutate("update_settings", { settings: { ...s.settings, hideOnLaunch } }, hideOnLaunch ? t("游戏启动后会隐藏启动器", "The launcher hides while playing") : t("游戏启动后启动器保持显示", "The launcher stays visible while playing"))}
              />
            </div>
            <div className="setting-row">
              <div className="col">
                <span className="setting-name">{t("关闭窗口后驻留托盘", "Keep running in the tray when closed")}</span>
                <span className="field-hint">{t("关闭按钮只收起窗口，游戏和下载继续；点击托盘图标恢复，从托盘菜单退出启动器", "Closing only hides the window; games and downloads continue. Click the tray icon to restore it, or use its menu to quit")}</span>
              </div>
              <Switch label={t("关闭窗口后驻留托盘", "Keep running in the tray when closed")} checked={s.settings.closeToTray !== false}
                onChange={(closeToTray) => void s.mutate("update_settings", { settings: { ...s.settings, closeToTray } }, closeToTray ? t("关闭窗口后会留在托盘", "Closing keeps the launcher in the tray") : t("关闭窗口时会退出；任务进行中会先询问", "Closing exits the launcher, with confirmation while work is in progress"))} />
            </div>
            <Field label="Java" hint={t("留空时自动使用匹配的 Java，缺少时下载 Temurin JRE", "Leave empty to use a matching Java; Temurin JRE is downloaded if missing")}>
              <div className="row">
                <Input mono placeholder={t("留空自动匹配", "Empty for automatic")} value={form.javaPath} onChange={(e) => set({ javaPath: e.target.value })} />
                <Button
                  icon="folder"
                  onClick={async () => {
                    const [path] = await s.chooseFiles("java");
                    if (path) set({ javaPath: path });
                  }}
                >
                  {t("选择文件", "Choose file")}
                </Button>
              </div>
              <JavaPicker value={form.javaPath.trim()} onChange={(javaPath) => set({ javaPath })} />
            </Field>
            <GlobalLaunchOptions value={form.launch} onChange={(launch) => set({ launch })} changed={fieldChanged(form, saved, "launch")} />
          </Section>

          <Section id="download" icon="download" title={t("下载", "Download")} text={t("下载源、并发数和数据存放位置", "Download source, parallel downloads and where data lives")}>
            <Field label={t("下载源", "Download source")} hint={t("镜像在国内更快，失败时自动改用官方源；文件都会用官方校验值核对", "The mirror is faster in mainland China and falls back to the official source; every file is checked against official hashes")}>
              <div>
                <Segmented
                  value={form.downloadSource}
                  options={[
                    { value: "bmclapi", label: t("BMCLAPI 镜像", "BMCLAPI mirror") },
                    { value: "official", label: t("Mojang 官方", "Mojang official") },
                  ]}
                  onChange={(downloadSource) => set({ downloadSource })}
                />
              </div>
            </Field>
            <Field
              label={t("同时下载数", "Parallel downloads")}
              hint={concurrencyOk ? t("默认 32；游戏资源多是小文件，并发越高越快。网络或镜像不稳定时可以调低", "Default 32; game assets are mostly small files, so more is faster. Lower it if the network or mirror is unstable") : t("需要在 1–64 之间", "Must be between 1 and 64")}
              tone={concurrencyOk ? undefined : "bad"}
            >
              <div className="row">
                <Input mono value={form.concurrency} invalid={!concurrencyOk} onChange={(e) => set({ concurrency: e.target.value })} style={{ width: 120 }} />
                <span className="muted">{t("个文件", " files")}</span>
                <div className="spacer" />
                <Presets values={[8, 16, 32, 64]} current={concurrency} label={(n) => String(n)} onPick={(n) => set({ concurrency: String(n) })} />
              </div>
            </Field>
            <Field
              label={t("网络代理", "Network proxy")}
              hint={
                !proxyOk
                  ? t("例如 http://127.0.0.1:7890 或 socks5://127.0.0.1:7891", "For example http://127.0.0.1:7890 or socks5://127.0.0.1:7891")
                  : form.proxyMode === "system"
                    ? t("使用系统设置里的代理（例如 Clash 的「系统代理」）；没有设置时直接连接", "Uses the proxy from the system settings (such as Clash's system proxy); connects directly when there is none")
                    : form.proxyMode === "none"
                      ? t("所有下载和登录都直接连接，不经过代理", "Every download and sign-in connects directly, without a proxy")
                      : t("下载、登录和搜索都经过这个代理；游戏本身不受影响", "Downloads, sign-in and search go through this proxy; the game itself is not affected")
              }
              tone={proxyOk ? undefined : "bad"}
            >
              <div className="row">
                <Segmented
                  value={form.proxyMode}
                  options={[
                    { value: "system", label: t("跟随系统", "System") },
                    { value: "none", label: t("不用代理", "None") },
                    { value: "custom", label: t("自定义", "Custom") },
                  ]}
                  onChange={(proxyMode) => set({ proxyMode })}
                />
                {form.proxyMode === "custom" && (
                  <Input mono value={form.proxyUrl} invalid={!proxyOk} placeholder="http://127.0.0.1:7890" onChange={(e) => set({ proxyUrl: e.target.value })} style={{ flex: 1, minWidth: 0 }} />
                )}
              </div>
            </Field>
            <Field
              label="CurseForge API Key"
              hint={t("填写后可以在 CurseForge 搜索和安装模组、资源包、光影，并导入 CurseForge 整合包。只保存在本机。", "Lets you search and install mods, resource packs and shaders from CurseForge, and import CurseForge modpacks. Stored on this computer only.")}
              action={
                <button type="button" className="link" onClick={() => void s.openUrl("https://console.curseforge.com/")}>
                  {t("申请 API Key", "Get an API key")}
                </button>
              }
            >
              <Input mono type="password" autoComplete="off" value={form.curseforgeKey} placeholder="$2a$10$…" onChange={(e) => set({ curseforgeKey: e.target.value })} />
            </Field>
            <Field label={t("下载与游戏数据目录", "Downloads and game data folder")} hint={t("迁移需选择空文件夹：游戏、Java、模组和缓存会复制过去，原目录保留为备份。", "Moving needs an empty folder: games, Java, mods and caches are copied over and the old folder stays as a backup.")}>
              <div className="row">
                <div className="path-box mono selectable truncate">{s.appInfo.dataDir}</div>
                <Button icon="folder" onClick={() => void s.open({ kind: "dataDir" })}>
                  {t("打开", "Open")}
                </Button>
                <Button disabled={dirty || s.fileBusy} onClick={() => void s.relocate()}>
                  {s.fileBusy ? t("处理中…", "Working…") : dirty ? t("请先保存设置", "Save settings first") : t("选择目录并迁移", "Choose folder and move")}
                </Button>
              </div>
            </Field>
          </Section>

          <StorageSection />

          <Section id="look" icon="palette" title={t("外观", "Appearance")} text={t("主题和动效，立即生效", "Theme and motion, applied at once")}>
            <LanguageRow />
            <div className="themes">
              {THEMES.map((theme) => (
                <button key={theme.value} type="button" className="theme" aria-pressed={s.settings.theme === theme.value} onClick={() => setTheme(theme.value)}>
                  <span className={`theme-preview ${theme.value}`}>
                    <ThemePreview />
                    {theme.value === "system" && (
                      <span className="theme-preview-half">
                        <ThemePreview />
                      </span>
                    )}
                  </span>
                  <span className="theme-label">
                    {s.settings.theme === theme.value && <Icon name="check" size={13} />}
                    {theme.label}
                  </span>
                </button>
              ))}
            </div>
            <div className="setting-row">
              <div className="col">
                <span className="setting-name">{t("减少动效", "Reduce motion")}</span>
                <span className="field-hint">{t("关闭过渡和滑动动画；系统开启「减少动态效果」时也会自动减少", "Turns off transitions and sliding animations; also follows the system's Reduce Motion")}</span>
              </div>
              <Switch label={t("减少动效", "Reduce motion")} checked={s.reduceMotion} onChange={s.setReduceMotion} />
            </div>
            <div className="setting-row">
              <div className="col">
                <span className="setting-name">{t("状态栏显示技术信息", "Technical details in the status bar")}</span>
                <span className="field-hint">{t("下载源、并发数、Java、内存和平台；关闭时只在悬停版本号时显示", "Download source, parallel downloads, Java, memory and platform; when off, they show on hovering the version")}</span>
              </div>
              <Switch label={t("状态栏显示技术信息", "Technical details in the status bar")} checked={prefs.devInfo} onChange={prefs.setDevInfo} />
            </div>
          </Section>

          <Section
            id="login"
            icon="shield"
            title={t("微软登录", "Microsoft sign-in")}
            text={
              builtInLogin
                ? t("这个版本已内置，可以直接用微软账号登录", "Built into this release; sign in with a Microsoft account directly")
                : s.settings.clientId
                  ? t("正在使用你填写的 Azure 应用", "Using the Azure app you entered")
                  : t("这个版本没有内置，需要填写一个 Azure 应用的 Client ID", "Not built into this release; it needs an Azure app's Client ID")
            }
          >
            {builtInLogin && (
              <>
                <div className="setting-row">
                  <div className="col">
                    <span className="setting-name">{t("可以直接登录", "Ready to sign in")}</span>
                    <span className="field-hint">{t("在账号页添加微软账号即可，不需要任何配置", "Add a Microsoft account on the Accounts page; nothing to set up")}</span>
                  </div>
                  <Button size="sm" icon="user" onClick={() => useStore.getState().setPage("accounts")}>
                    {t("去登录", "Sign in")}
                  </Button>
                </div>
                <button type="button" className="launch-options-toggle" aria-expanded={ownAppShown} onClick={() => setOwnApp(!ownApp)}>
                  <Icon name={ownAppShown ? "chevronDown" : "chevronRight"} size={13} />
                  {t("高级：改用自己的 Azure 应用", "Advanced: use your own Azure app")}
                </button>
              </>
            )}
            {ownAppShown && (
              <>
                <Field
                  label={t("Azure 应用 Client ID", "Azure app Client ID")}
                  tone={clientId === "" ? undefined : clientOk ? "ok" : "bad"}
                  hint={
                    clientId === ""
                      ? s.loginConfigured
                        ? t("已使用发行版内置的 Client ID；填写后将改用你自己的应用", "Using the Client ID built into this release; fill one in to use your own app")
                        : t("填写 Azure 应用的 Application (client) ID，不需要 client secret", "The Azure app's Application (client) ID; no client secret needed")
                      : clientOk
                        ? t("格式正确", "Looks right")
                        : t("格式不正确，应为 8-4-4-4-12 位的 GUID", "Wrong format; it should be a GUID like 8-4-4-4-12")
                  }
                  action={
                    <button type="button" className="link" onClick={() => useStore.setState({ showAzureHelp: !s.showAzureHelp })}>
                      {s.showAzureHelp ? t("收起说明", "Hide steps") : t("如何获取 Client ID", "How to get a Client ID")}
                    </button>
                  }
                >
                  <Input mono placeholder="00000000-0000-0000-0000-000000000000" value={form.clientId} invalid={!clientOk} onChange={(e) => set({ clientId: e.target.value })} />
                </Field>
                <AnimatePresence initial={false}>
                  {s.showAzureHelp && (
                    <motion.div initial={{ height: 0, opacity: 0 }} animate={{ height: "auto", opacity: 1, transition: normal }} exit={{ height: 0, opacity: 0 }} style={{ overflow: "hidden" }}>
                      <ol className="help-steps">
                        <li>{t("Microsoft Entra ID → 应用注册 → 新注册", "Microsoft Entra ID → App registrations → New registration")}</li>
                        <li>{t("受支持的账户类型选择「仅个人 Microsoft 帐户」", "For supported account types choose \"Personal Microsoft accounts only\"")}</li>
                        <li>
                          {t("重定向 URI 选择「移动和桌面应用程序」，填写 ", "For the redirect URI choose \"Mobile and desktop applications\" and enter ")}<span className="mono">http://localhost</span>
                        </li>
                        <li>{t("在「身份验证」中启用「允许公共客户端流」", "Under Authentication turn on \"Allow public client flows\"")}</li>
                        <li>{t("创建后复制「应用程序(客户端) ID」粘贴到上面", "Copy the \"Application (client) ID\" and paste it above")}</li>
                      </ol>
                      <Button size="sm" icon="external" onClick={() => void s.openUrl(AZURE_PORTAL)}>
                        {t("打开 Azure Portal", "Open Azure Portal")}
                      </Button>
                    </motion.div>
                  )}
                </AnimatePresence>
              </>
            )}
          </Section>

          <Section id="about" icon="info" title={t("关于", "About")}>
            <div className="setting-row">
              <div className="col">
                <span className="setting-name">{t("版本", "Version")}</span>
                <span className="field-hint mono">
                  {t("草岛 ", "Islet ")}{s.appInfo.version} · {s.appInfo.platform} {s.appInfo.arch}
                </span>
              </div>
              <span className="faint" style={{ fontSize: "var(--text-xs)" }}>
                {t("不收集任何遥测数据", "No telemetry is collected")}
              </span>
            </div>
            <div className="setting-row">
              <div className="col">
                <span className="setting-name">{t("开源许可", "License")}</span>
                <span className="field-hint">
                  {t("GPL-3.0 开源。非 Mojang 或 Microsoft 官方产品，与其无关；Minecraft 是 Mojang Studios 的商标。", "Open source under GPL-3.0. Not an official Minecraft product and not affiliated with Mojang or Microsoft; Minecraft is a trademark of Mojang Studios.")}
                </span>
              </div>
              <Button size="sm" variant="ghost" icon="external" onClick={() => void useStore.getState().openUrl(REPOSITORY)}>
                {t("源代码", "Source code")}
              </Button>
            </div>
            <UpdateRows />
          </Section>
        </motion.div>
        </div>
      </div>
    </>
  );
}

const REPOSITORY = "https://github.com/saudademjj/islet-launcher";

const SECTIONS: { id: string; title: string; icon: IconName }[] = [
  { id: "game", title: t("游戏", "Game"), icon: "cpu" },
  { id: "download", title: t("下载", "Download"), icon: "download" },
  { id: "storage", title: t("存储", "Storage"), icon: "disk" },
  { id: "look", title: t("外观", "Appearance"), icon: "palette" },
  { id: "login", title: t("微软登录", "Microsoft sign-in"), icon: "shield" },
  { id: "about", title: t("关于", "About"), icon: "info" },
];

const THEMES: { value: Theme; label: string }[] = [
  { value: "system", label: t("跟随系统", "System") },
  { value: "light", label: t("浅色", "Light") },
  { value: "dark", label: t("深色", "Dark") },
];

/** The launcher in miniature, in the theme's own colours: forest sidebar,
    the home stage cut down to the stone, the start button, three facts. */
function ThemePreview() {
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

function Presets({ values, current, label, onPick }: { values: number[]; current: number; label: (v: number) => string; onPick: (v: number) => void }) {
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

/** Checking for a newer launcher. Off entirely when the build has no
    update source. */
function UpdateRows() {
  const s = usePick((s) => ({ settings: s.settings, update: s.update, mutate: s.mutate, checkUpdate: s.checkUpdate, openUrl: s.openUrl, toast: s.toast }));
  const [available, setAvailable] = useState<boolean | null>(null);
  const [checking, setChecking] = useState(false);
  const [installing, setInstalling] = useState(false);
  useEffect(() => {
    call<boolean>("update_source").then(setAvailable, () => setAvailable(false));
  }, []);
  if (available === false) {
    return <span className="field-hint">{t("这个版本没有配置更新地址，请到发布页查看新版本。", "This build has no update source; check the release page for new versions.")}</span>;
  }
  return (
    <>
      <div className="setting-row">
        <div className="col">
          <span className="setting-name">{t("启动时检查更新", "Check for updates at start")}</span>
          <span className="field-hint">{t("只读取一个版本信息文件；有新版本时在状态栏提示，由你决定是否下载。下载的安装包会先核对校验值再打开", "Reads one version file; a new version shows in the status bar and downloading is up to you. A downloaded package is checked against its hash before it opens")}</span>
        </div>
        <Switch label={t("系统通知", "System notifications")} checked={s.settings.systemNotifications !== false} onChange={systemNotifications => void s.mutate("update_settings", { settings: { ...s.settings, systemNotifications } })} />
        <Field label={t("Discord 应用 ID（留空关闭）", "Discord application ID (empty disables)")} hint={t("填写你在 Discord Developer Portal 创建的应用 ID，游戏启动时展示世界与版本。", "Use an application ID from Discord Developer Portal to show the world and game version while playing.")}>
          <Input defaultValue={s.settings.discordApplicationId ?? ""} inputMode="numeric" onBlur={e => { const discordApplicationId = e.target.value.trim(); if (/^\d{17,22}$/.test(discordApplicationId) || !discordApplicationId) void s.mutate("update_settings", { settings: { ...s.settings, discordApplicationId } }); }} />
        </Field>
        <Switch
          label={t("启动时检查更新", "Check for updates at start")}
          checked={s.settings.checkUpdates}
          onChange={(checkUpdates) => void s.mutate("update_settings", { settings: { ...s.settings, checkUpdates } })}
        />
      </div>
      <div className="row">
        {s.update ? (
          <>
            <span style={{ flex: 1 }}>
              {t("新版本 ", "New version ")}<span className="mono">{s.update.version}</span>
              {s.update.notes ? ` · ${s.update.notes}` : ""}
            </span>
            {s.update.package ? (
              <>
                <Button variant="ghost" icon="external" onClick={() => void s.openUrl(s.update!.url)}>
                  {t("更新说明", "Release notes")}
                </Button>
                <Button
                  variant="primary"
                  icon="download"
                  busy={installing}
                  onClick={async () => {
                    setInstalling(true);
                    try {
                      const message = await call<string>("install_update");
                      if (message) s.toast(message, "success");
                    } catch (error) {
                      s.toast(toError(error).message, "error");
                    }
                    setInstalling(false);
                  }}
                >
                  {installing ? t("正在下载并校验…", "Downloading and checking…") : t("下载并安装", "Download and install")}
                </Button>
              </>
            ) : (
              <Button variant="primary" icon="external" onClick={() => void s.openUrl(s.update!.url)}>
                {t("去下载", "Download")}
              </Button>
            )}
          </>
        ) : (
          <>
            <span className="muted" style={{ flex: 1 }}>
              {t("没有发现新版本", "You're up to date")}
            </span>
            <Button
              icon="refresh"
              busy={checking}
              onClick={async () => {
                setChecking(true);
                await s.checkUpdate();
                setChecking(false);
              }}
            >
              {t("检查更新", "Check for updates")}
            </Button>
          </>
        )}
      </div>
    </>
  );
}

/** Interface language. Switching saves it and reloads the page, so every
    string (also those built when modules load) changes together. */
function LanguageRow() {
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
        <span className="field-hint">{t("界面和提示信息的语言；切换后启动器会刷新一次", "For the interface and messages; the launcher refreshes once after switching")}</span>
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

/** Launch options every world starts from; saved on their own button so
    they do not wait for the rest of the form. */
/** Launch options for every world; saved with the rest of the page. */
function GlobalLaunchOptions({ value, onChange, changed }: { value: LaunchConfig; onChange: (next: LaunchConfig) => void; changed: boolean }) {
  return (
    <div className="col" style={{ gap: 10 }}>
      <LaunchOptions value={value} onChange={onChange} />
      {changed && (
        <span className="field-hint">
          {t("对所有世界生效；世界自己的设置会叠加或覆盖它们。", "Applies to every world; a world's own options add to or override them.")}
        </span>
      )}
    </div>
  );
}

/** Shown in the save bar when the player tried to switch pages, or to quit
    the launcher, with edits unsaved. */
function LeaveQuestion({ count }: { count: number }) {
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

/** What the launcher keeps on disk, and what can go: everything cleaned
    here is downloaded or rebuilt again when needed. */
function StorageSection() {
  const s = usePick((s) => ({ fileBusy: s.fileBusy, filesChanged: s.filesChanged, fileOperation: s.fileOperation, openModal: s.openModal, open: s.open, openRecycle: s.openRecycle }));
  const [usage, setUsage] = useState<StorageUsage | null>(null);
  const [measuring, setMeasuring] = useState(false);
  const measure = () => {
    setMeasuring(true);
    call<StorageUsage>("storage_usage")
      .then(setUsage)
      .catch(() => setUsage(null))
      .finally(() => setMeasuring(false));
  };
  // Measuring walks every file, so it waits to be asked; after a cleanup it
  // measures again by itself.
  useEffect(() => {
    if (usage) measure();
  }, [s.filesChanged]);
  const clean = (target: Cleanup) => void s.fileOperation(null, { kind: "clean", target });
  const rows: { label: string; bytes: number; hint: string; action?: React.ReactNode }[] = usage
    ? [
        { label: t("世界", "Worlds"), bytes: usage.worlds, hint: t("存档、模组、配置和截图", "Saves, mods, configs and screenshots") },
        { label: t("游戏资源文件", "Game assets"), bytes: usage.assets, hint: t("声音、语言和材质；所有世界共用", "Sounds, languages and textures; shared by all worlds") },
        { label: t("运行库", "Libraries"), bytes: usage.libraries, hint: t("游戏和加载器的库；所有世界共用", "Game and loader libraries; shared by all worlds") },
        {
          label: t("版本文件", "Version files"),
          bytes: usage.versions,
          hint: usage.unusedVersions.length ? t(`${usage.unusedVersions.length} 个版本已经没有世界在用：${usage.unusedVersions.slice(0, 4).join("、")}${usage.unusedVersions.length > 4 ? " …" : ""}`, `${usage.unusedVersions.length} versions no world uses: ${usage.unusedVersions.slice(0, 4).join(", ")}${usage.unusedVersions.length > 4 ? " …" : ""}`) : t("都有世界在用", "All in use"),
          action: usage.unusedVersions.length > 0 && (
            <Button size="sm" disabled={s.fileBusy} onClick={() => clean({ kind: "unusedVersions" })}>
              {t("清理 ", "Free ")}{formatBytes(usage.unusedVersionsBytes)}
            </Button>
          ),
        },
        {
          label: t("回收站", "Recycle bin"),
          bytes: usage.recycle,
          hint: t("删除的世界、模组、存档和截图", "Deleted worlds, mods, saves and screenshots"),
          action: (
            <Button size="sm" disabled={usage.recycle === 0} onClick={() => void s.openRecycle()}>
              {t("查看", "View")}
            </Button>
          ),
        },
        {
          label: t("备份", "Backups"),
          bytes: usage.backups,
          hint: t("世界和存档的备份 .zip，不会自动删除", "Backup .zip files of worlds and saves; never removed automatically"),
          action: (
            <Button size="sm" icon="folder" onClick={() => void s.open({ kind: "dataDir" })}>
              {t("打开", "Open")}
            </Button>
          ),
        },
        {
          label: t("截图缩略图", "Screenshot thumbnails"),
          bytes: usage.thumbnails,
          hint: t("需要时会重新生成", "Made again when needed"),
          action: usage.thumbnails > 0 && (
            <Button size="sm" disabled={s.fileBusy} onClick={() => clean({ kind: "thumbnails" })}>
              {t("清理", "Clean")}
            </Button>
          ),
        },
        {
          label: t("启动器日志", "Launcher logs"),
          bytes: usage.logs,
          hint: t("清理一周以前的日志", "Removes logs older than a week"),
          action: (
            <Button size="sm" disabled={s.fileBusy} onClick={() => clean({ kind: "oldLogs" })}>
              {t("清理", "Clean")}
            </Button>
          ),
        },
      ]
    : [];
  const total = usage ? usage.worlds + usage.assets + usage.libraries + usage.versions + usage.recycle + usage.backups + usage.thumbnails + usage.logs + usage.java.reduce((sum, j) => sum + j.bytes, 0) : 0;
  return (
    <Section id="storage" icon="disk" title={t("存储", "Storage")} text={t("启动器占用的空间；这里清理的都能在需要时重新下载或生成", "Space the launcher uses; anything cleaned here is downloaded or rebuilt when needed")}>
      {!usage ? (
        <div className="row">
          <span className="muted" style={{ flex: 1 }}>
            {t("统计需要遍历所有文件，资源文件多时要几秒钟。", "Measuring reads every file and can take a few seconds.")}
          </span>
          <Button icon="disk" busy={measuring} onClick={measure}>
            {t("统计占用", "Measure")}
          </Button>
        </div>
      ) : (
        <>
          <div className="storage-total">
            <span className="fact-number">{formatBytes(total)}</span>
            <span className="muted">{t("共占用", "in total")}</span>
            <span className="spacer" />
            <Button size="sm" variant="ghost" icon="refresh" busy={measuring} onClick={measure}>
              {t("重新统计", "Measure again")}
            </Button>
          </div>
          <div className="storage-bar" aria-hidden>
            {rows.map((row) => (
              <span key={row.label} style={{ flexGrow: Math.max(row.bytes, 1) }} title={`${row.label} ${formatBytes(row.bytes)}`} />
            ))}
          </div>
          <div className="list">
            {rows.map((row) => (
              <div key={row.label} className="list-row storage-row">
                <div className="col" style={{ flex: 1, minWidth: 0, gap: 2 }}>
                  <span style={{ fontWeight: 600 }}>{row.label}</span>
                  <span className="faint truncate" style={{ fontSize: "var(--text-xs)" }}>
                    {row.hint}
                  </span>
                </div>
                <span className="mono">{formatBytes(row.bytes)}</span>
                {row.action}
              </div>
            ))}
            {usage.java.map((java) => (
              <div key={java.path} className="list-row storage-row">
                <div className="col" style={{ flex: 1, minWidth: 0, gap: 2 }}>
                  <span style={{ fontWeight: 600 }}>Java {java.major}</span>
                  <span className="faint truncate" style={{ fontSize: "var(--text-xs)" }} title={java.path}>
                    {java.unused ? t("已安装的世界都不需要它，删除后需要时会重新下载", "No installed world needs it; it downloads again if needed") : t("有世界正在用", "In use")}
                  </span>
                </div>
                <span className="mono">{formatBytes(java.bytes)}</span>
                {java.unused && (
                  <Button size="sm" disabled={s.fileBusy} onClick={() => clean({ kind: "java", path: java.path })}>
                    {t("删除", "Delete")}
                  </Button>
                )}
              </div>
            ))}
          </div>
        </>
      )}
    </Section>
  );
}

function Section({ id, icon, title, text, children }: { id: string; icon: IconName; title: string; text?: string; children: React.ReactNode }) {
  return (
    <motion.section id={`set-${id}`} className="card settings-section" variants={item}>
      <div className="settings-head">
        <span className="settings-icon">
          <Icon name={icon} size={17} />
        </span>
        <div className="col" style={{ gap: 2 }}>
          <span className="card-title">{title}</span>
          {text && <span className="field-hint">{text}</span>}
        </div>
      </div>
      <div className="card-body col" style={{ gap: 20 }}>
        {children}
      </div>
    </motion.section>
  );
}
