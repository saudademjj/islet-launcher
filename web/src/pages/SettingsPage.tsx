import { AnimatePresence, motion } from "motion/react";
import { useEffect, useMemo, useRef, useState } from "react";

import type { DownloadSource, LaunchConfig, Settings, Theme } from "../api/types";
import { Icon } from "../components/Icon";
import { Button, Field, Input, Segmented, Switch } from "../components/ui";
import { JavaPicker } from "../components/JavaPicker";
import { LaunchOptionsFields, launchProblem } from "../components/LaunchOptions";
import { t } from "../lib/i18n";
import { validGuid } from "../lib/format";
import { glide, normal, stagger } from "../lib/motion";
import { MOD_KEY } from "../lib/platform";
import { usePrefs } from "../lib/prefs";
import { matchItem, queryWords } from "../lib/search";
import { Toolbar } from "../layout/Shell";
import { usePick, useStore } from "../store";
import { LanguageRow, LeaveQuestion, Presets, REPOSITORY, SECTIONS, Section, THEMES, ThemePreview } from "./settings/shared";
import { StorageSection } from "./settings/StorageSection";
import { UpdateRows } from "./settings/UpdateRows";

export { LeaveQuestion } from "./settings/shared";

const AZURE_PORTAL = "https://portal.azure.com/#view/Microsoft_AAD_RegisteredApps/ApplicationsListBlade";

interface Form {
  memory: string;
  memoryAuto: boolean;
  concurrency: string;
  clientId: string;
  discordApplicationId: string;
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
  discordApplicationId: s.discordApplicationId ?? "",
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
  const searching = query.trim() !== "";
  const prefs = usePrefs();

  const memory = Number(form.memory.trim());
  const memoryOk = Number.isInteger(memory) && memory >= 512 && memory <= 32768;
  const concurrency = Number(form.concurrency.trim());
  const concurrencyOk = Number.isInteger(concurrency) && concurrency >= 1 && concurrency <= 64;
  const clientId = form.clientId.trim();
  const clientOk = clientId === "" || validGuid(clientId);
  const discordId = form.discordApplicationId.trim();
  const discordOk = discordId === "" || /^\d{17,22}$/.test(discordId);
  const proxyOk = form.proxyMode !== "custom" || validProxy(form.proxyUrl.trim());
  const launchUsed = Object.values(form.launch).some((v) => v != null && v !== "");
  const launchInvalid = !discordOk || launchProblem(form.launch) !== null;

  // Low-frequency disclosures stay folded until opened, unless initialized
  // with custom values, searching, or displaying a validation error.
  const [javaOpen, setJavaOpen] = useState(Boolean(s.settings.javaPath));
  const javaShown = javaOpen || searching;

  const [advLaunchOpen, setAdvLaunchOpen] = useState(launchUsed || Boolean(s.settings.discordApplicationId));
  const advLaunchShown = advLaunchOpen || searching || launchInvalid;

  const builtInCurseforge = Boolean(s.appInfo.curseforgeConfigured);
  const dlCustom = concurrency !== 32 || (builtInCurseforge && form.curseforgeKey.trim() !== "");
  const [advDownloadOpen, setAdvDownloadOpen] = useState(
    s.settings.maxConcurrentDownloads !== 32 || (builtInCurseforge && s.settings.curseforgeKey.trim() !== ""),
  );
  const advDownloadShown = advDownloadOpen || searching || !concurrencyOk;

  const lookCustom = !prefs.intro || prefs.devInfo;
  const [lookDetailsOpen, setLookDetailsOpen] = useState(lookCustom);
  const lookDetailsShown = lookDetailsOpen || searching;

  // A release with Microsoft sign-in built in keeps the Azure setup out of
  // the way; players only need it to use an app of their own.
  const builtInLogin = s.loginConfigured && s.settings.clientId === "";
  const [ownApp, setOwnApp] = useState(false);
  const ownAppShown = !builtInLogin || ownApp || searching || !clientOk || s.showAzureHelp;
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
      const title = section.querySelector(".card-header .card-title")?.textContent ?? "";
      const whole = matchItem(title, [], words) !== null;
      let count = 0;
      for (const row of section.querySelectorAll<HTMLElement>(":scope > .card-body > *")) {
        if (row.classList.contains("launch-options-toggle")) {
          row.classList.remove("search-hit");
          row.classList.toggle("search-miss", !whole);
          continue;
        }
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
    // The rows change with the form and folded groups; their text is what is searched.
  }, [query, form, s.settings, javaShown, advLaunchShown, advDownloadShown, lookDetailsShown, ownAppShown]);
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

  const fail = (message: string) => {
    s.toast(message, "error");
    return false;
  };
  /** Saves every field on the page; false when something is invalid or the save failed. */
  const save = async (): Promise<boolean> => {
    if (!form.memoryAuto && !memoryOk) return fail(t("内存需要是 512–32768 之间的整数（MB）", "Memory must be a whole number from 512 to 32768 (MB)"));
    if (!concurrencyOk) return fail(t("同时下载数需要在 1–64 之间", "Parallel downloads must be between 1 and 64"));
    if (!clientOk) return fail(t("Client ID 格式不正确", "Invalid Client ID format"));
    if (!discordOk) return fail(t("Discord 应用 ID 格式不正确，应为 17–22 位数字", "Discord application ID must be 17 to 22 digits"));
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
      discordApplicationId: discordId,
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
      <Toolbar title={t("设置", "Settings")} meta={dirty ? <span className="tone-warn">{t("有未保存的更改", "Unsaved changes")}</span> : undefined}>
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
                <Icon name={sec.icon} />
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
          <Section id="game" icon="cpu" title={t("游戏", "Game")}>
            <Field
              label={t("默认游戏内存", "Default game memory")}
              hint={
                form.memoryAuto
                  ? t("按电脑内存和模组数量自动分配", "Sized automatically from your memory and mods")
                  : memoryOk
                    ? t("建议不超过电脑内存的一半", "Keep under half of your computer's memory")
                    : t("需要是 512–32768 之间的整数（MB）", "Must be a whole number from 512 to 32768 (MB)")
              }
              tone={form.memoryAuto || memoryOk ? undefined : "bad"}
            >
              <div className="col gap-12">
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
                <span className="field-hint">{t("首次启动时自动设为启动器语言", "Matches the launcher language on first start")}</span>
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
                    `退出后自动备份刚玩过的存档，保留最近 ${s.settings.autoBackupKeep} 份`,
                    `Backs up played saves on exit, keeping the latest ${s.settings.autoBackupKeep}`,
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
                      ? t("双显卡设备优先使用独立显卡，提升帧率", "Use the faster GPU on dual-GPU laptops")
                      : t("双显卡设备自动启用独立显卡（NVIDIA 请在启动选项使用 prime-run）", "Use the discrete GPU on dual-GPU systems (NVIDIA: use prime-run)")}
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
                <span className="field-hint">{t("进入游戏后收起，退出后恢复", "Hides once the game opens, returns on exit")}</span>
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
                <span className="field-hint">{t("仅收起窗口，游戏和下载在后台继续", "Keeps games and downloads running in the background")}</span>
              </div>
              <Switch label={t("关闭窗口后驻留托盘", "Keep running in the tray when closed")} checked={s.settings.closeToTray !== false}
                onChange={(closeToTray) => void s.mutate("update_settings", { settings: { ...s.settings, closeToTray } }, closeToTray ? t("关闭窗口后会留在托盘", "Closing keeps the launcher in the tray") : t("关闭窗口时会退出；任务进行中会先询问", "Closing exits the launcher, with confirmation while work is in progress"))} />
            </div>
            <div className="setting-row">
              <div className="col">
                <span className="setting-name">{t("系统通知", "System notifications")}</span>
                <span className="field-hint">{t("安装、部署或导出完成后提醒", "Notify when installs, deploys, or exports finish")}</span>
              </div>
              <Switch
                label={t("系统通知", "System notifications")}
                checked={s.settings.systemNotifications !== false}
                onChange={(systemNotifications) => void s.mutate("update_settings", { settings: { ...s.settings, systemNotifications } })}
              />
            </div>
            <button type="button" className="launch-options-toggle" aria-expanded={javaShown} onClick={() => setJavaOpen(!javaShown)}>
              <Icon name={javaShown ? "chevronDown" : "chevronRight"} />
              Java
              {!javaShown && <span className="chip">{form.javaPath.trim() ? t("指定", "Custom") : t("自动", "Auto")}</span>}
            </button>
            {javaShown && (
              <Field label="Java" hint={t("留空自动匹配，缺少时自动下载", "Auto-matched when empty; downloaded if missing")}>
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
            )}
            <button type="button" className="launch-options-toggle" aria-expanded={advLaunchShown} onClick={() => setAdvLaunchOpen(!advLaunchShown)}>
              <Icon name={advLaunchShown ? "chevronDown" : "chevronRight"} />
              {t("高级启动与联动", "Advanced launch & integrations")}
              {!advLaunchShown && (launchUsed || discordId !== "") && <span className="chip">{t("已设置", "Set")}</span>}
            </button>
            {advLaunchShown && (
              <>
                <Field
                  label={t("Discord 应用 ID（留空关闭）", "Discord application ID (empty disables)")}
                  tone={discordId === "" ? undefined : discordOk ? "ok" : "bad"}
                  hint={
                    discordOk
                      ? t("在 Discord 展示当前游玩的世界与版本", "Show your current world and version on Discord")
                      : t("格式不正确，应为 17–22 位数字", "Wrong format; it should be 17 to 22 digits")
                  }
                >
                  <Input
                    mono
                    inputMode="numeric"
                    value={form.discordApplicationId}
                    invalid={!discordOk}
                    placeholder="1234567890123456789"
                    onChange={(e) => set({ discordApplicationId: e.target.value })}
                  />
                </Field>
                <LaunchOptionsFields value={form.launch} onChange={(launch) => set({ launch })} />
                {fieldChanged(form, saved, "launch") && (
                  <span className="field-hint">
                    {t("对所有世界生效，可在单个世界中覆盖", "Applies to all worlds; can be overridden per world")}
                  </span>
                )}
              </>
            )}
          </Section>

          <Section id="download" icon="download" title={t("下载", "Download")}>
            <Field label={t("下载源", "Download source")} hint={t("国内推荐镜像源，失败时自动切换官方源", "Mirror is faster in mainland China, with official fallback")}>
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
              label={t("网络代理", "Network proxy")}
              hint={
                !proxyOk
                  ? t("例如 http://127.0.0.1:7890 或 socks5://127.0.0.1:7891", "For example http://127.0.0.1:7890 or socks5://127.0.0.1:7891")
                  : form.proxyMode === "system"
                    ? t("跟随系统代理设置", "Follow system proxy settings")
                    : form.proxyMode === "none"
                      ? t("不经过代理，直接连接", "Connect directly without a proxy")
                      : t("仅用于下载、登录和搜索，不影响游戏", "Applies to downloads, sign-in, and search only")
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
            {!builtInCurseforge && (
              <Field
                label="CurseForge API Key"
                hint={t("用于搜索和安装 CurseForge 内容", "Needed to browse and install CurseForge content")}
                action={
                  <button type="button" className="link" onClick={() => void s.openUrl("https://console.curseforge.com/")}>
                    {t("申请 API Key", "Get an API key")}
                  </button>
                }
              >
                <Input
                  mono
                  type="password"
                  autoComplete="off"
                  value={form.curseforgeKey}
                  placeholder="$2a$10$…"
                  onChange={(e) => set({ curseforgeKey: e.target.value })}
                />
              </Field>
            )}
            <Field label={t("下载与游戏数据目录", "Downloads and game data folder")} hint={t("选择空文件夹迁移，原目录保留备份", "Pick an empty folder to move data; the old folder is kept as backup")}>
              <div className="row wrap">
                <div className="path-box mono selectable truncate">{s.appInfo.dataDir}</div>
                <Button icon="folder" onClick={() => void s.open({ kind: "dataDir" })}>
                  {t("打开", "Open")}
                </Button>
                <Button disabled={dirty || s.fileBusy} onClick={() => void s.relocate()}>
                  {s.fileBusy ? t("处理中…", "Working…") : dirty ? t("请先保存设置", "Save settings first") : t("选择目录并迁移", "Choose folder and move")}
                </Button>
              </div>
            </Field>
            <button type="button" className="launch-options-toggle" aria-expanded={advDownloadShown} onClick={() => setAdvDownloadOpen(!advDownloadShown)}>
              <Icon name={advDownloadShown ? "chevronDown" : "chevronRight"} />
              {t("高级下载设置", "Advanced download settings")}
              {!advDownloadShown && dlCustom && <span className="chip">{t("已自定义", "Custom")}</span>}
            </button>
            {advDownloadShown && (
              <>
                <Field
                  label={t("同时下载数", "Parallel downloads")}
                  hint={concurrencyOk ? t("默认 32，网络不稳定时可调低", "Default 32; lower on unstable networks") : t("需要在 1–64 之间", "Must be between 1 and 64")}
                  tone={concurrencyOk ? undefined : "bad"}
                >
                  <div className="row">
                    <Input mono value={form.concurrency} invalid={!concurrencyOk} onChange={(e) => set({ concurrency: e.target.value })} style={{ width: 120 }} />
                    <span className="muted">{t("个文件", " files")}</span>
                    <div className="spacer" />
                    <Presets values={[8, 16, 32, 64]} current={concurrency} label={(n) => String(n)} onPick={(n) => set({ concurrency: String(n) })} />
                  </div>
                </Field>
                {builtInCurseforge && (
                  <Field
                    label="CurseForge API Key"
                    hint={t("已内置可用，留空即可，也可填自己的 Key", "Built in and ready; leave blank or enter your own key")}
                    action={
                      <button type="button" className="link" onClick={() => void s.openUrl("https://console.curseforge.com/")}>
                        {t("申请 API Key", "Get an API key")}
                      </button>
                    }
                  >
                    <Input
                      mono
                      type="password"
                      autoComplete="off"
                      value={form.curseforgeKey}
                      placeholder={t("已内置（留空使用默认）", "Built in (leave blank)")}
                      onChange={(e) => set({ curseforgeKey: e.target.value })}
                    />
                  </Field>
                )}
              </>
            )}
          </Section>

          <StorageSection />

          <Section id="look" icon="palette" title={t("外观", "Appearance")}>
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
                    {s.settings.theme === theme.value && <Icon name="check" />}
                    {theme.label}
                  </span>
                </button>
              ))}
            </div>
            <div className="setting-row">
              <div className="col">
                <span className="setting-name">{t("减少动效", "Reduce motion")}</span>
                <span className="field-hint">{t("系统开启「减少动态效果」时也会自动减少", "Also follows the system's Reduce Motion")}</span>
              </div>
              <Switch label={t("减少动效", "Reduce motion")} checked={s.reduceMotion} onChange={s.setReduceMotion} />
            </div>
            <button type="button" className="launch-options-toggle" aria-expanded={lookDetailsShown} onClick={() => setLookDetailsOpen(!lookDetailsShown)}>
              <Icon name={lookDetailsShown ? "chevronDown" : "chevronRight"} />
              {t("界面细节", "Interface details")}
              {!lookDetailsShown && lookCustom && <span className="chip">{t("已自定义", "Custom")}</span>}
            </button>
            {lookDetailsShown && (
              <>
                <div className="setting-row">
                  <div className="col">
                    <span className="setting-name">{t("启动动画", "Intro animation")}</span>
                    <span className="field-hint">{t("开启「减少动效」时不播放", "Skipped while Reduce motion is on")}</span>
                  </div>
                  <Switch label={t("启动动画", "Intro animation")} checked={prefs.intro} onChange={prefs.setIntro} />
                </div>
                <div className="setting-row">
                  <div className="col">
                    <span className="setting-name">{t("状态栏显示技术信息", "Technical details in the status bar")}</span>
                    <span className="field-hint">{t("在状态栏显示下载源、Java、内存和平台信息", "Show download source, Java, memory, and platform in the status bar")}</span>
                  </div>
                  <Switch label={t("状态栏显示技术信息", "Technical details in the status bar")} checked={prefs.devInfo} onChange={prefs.setDevInfo} />
                </div>
              </>
            )}
          </Section>

          <Section
            id="login"
            icon="shield"
            title={t("微软登录", "Microsoft sign-in")}
            text={
              builtInLogin
                ? t("已内置，可直接登录", "Built in and ready to sign in")
                : s.settings.clientId
                  ? t("已配置自定义 Azure 应用", "Using your custom Azure app")
                  : t("未内置，需填写 Azure 应用 ID", "Needs an Azure Client ID")
            }
          >
            {builtInLogin && (
              <>
                <div className="setting-row">
                  <div className="col">
                    <span className="setting-name">{t("可以直接登录", "Ready to sign in")}</span>
                  </div>
                  <Button size="sm" icon="user" onClick={() => useStore.getState().setPage("accounts")}>
                    {t("去登录", "Sign in")}
                  </Button>
                </div>
                <button type="button" className="launch-options-toggle" aria-expanded={ownAppShown} onClick={() => setOwnApp(!ownApp)}>
                  <Icon name={ownAppShown ? "chevronDown" : "chevronRight"} />
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
                        ? t("已使用内置配置，填写后可改为自己的应用", "Using built-in config; fill in to override")
                        : t("填写 Azure 应用 ID，无需密钥", "Enter your Azure Application (client) ID")
                      : clientOk
                        ? t("格式正确", "Looks right")
                        : t("格式不正确，请检查是否复制完整", "Invalid format; check that the full ID was copied")
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
            </div>
            <div className="setting-row">
              <div className="col">
                <span className="setting-name">{t("开源许可", "License")}</span>
                <span className="field-hint">
                  {t("GPL-3.0 开源。非 Mojang 或 Microsoft 官方产品。", "Open source under GPL-3.0. Not an official Mojang or Microsoft product.")}
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
