import { AnimatePresence, motion } from "motion/react";
import { useEffect, useMemo, useRef, useState } from "react";

import { call, toError } from "../api/backend";
import type { ContentKind, Instance, LastModChange, LocalMod, ModInsight, ModMeta, ModProject, ModVersion, PackFile, PackInfo, VersionSummary } from "../api/types";
import { BrandMark, PixelIcon, Sign, type PixelIconName } from "../components/Pixel";
import { ModIcon } from "../components/ModIcon";
import { ProjectDetail } from "../components/ProjectDetail";
import { Icon } from "../components/Icon";
import { BiomePicker } from "../components/BiomePicker";
import { JavaPicker } from "../components/JavaPicker";
import { LoaderPicker, useLoaderSelectionValid } from "../components/LoaderPicker";
import { LaunchOptions, launchProblem } from "../components/LaunchOptions";
import { Modal } from "../components/overlay";
import { SceneHeader } from "../components/SceneHeader";
import { Button, Empty, Field, Identicon, Input, Notice, Segmented, Switch, Tabs } from "../components/ui";
import { formatBytes, formatCount, formatPlayTime, formatRelative, formatReleaseDate, loaderLabel, updateName, versionTypeLabel } from "../lib/format";
import { useModIcons, usePackInsights } from "../lib/contentIcons";
import { FILTER_GROUPS, SORTS, type FilterChoice, type SearchSort } from "../lib/contentFilters";
import { findChineseMods, findModName, hasChinese, slugOfFile, zhDescription, type ModName } from "../lib/modNames";
import { useFormGuard } from "../lib/formGuard";
import { normal } from "../lib/motion";
import { SMALL_PX } from "../lib/pixelScale";
import { matchItem, queryWords } from "../lib/search";
import { useWorlds } from "../lib/worlds";
import { useServers } from "../lib/servers";
import { useShots } from "../lib/screenshots";
import { OptionsTab } from "./OptionsTab";
import { ScreenshotsTab } from "./Screenshots";
import { ServersTab } from "./ServersTab";
import { useAutoMemory, usePick, useStore, type ManageTab, type SearchSource } from "../store";
import { isEnglish, t } from "../lib/i18n";

const PAGE_SIZE = 10;
/** Tags a long filter group shows before "更多". */
const FOLDED = 10;
/** Placeholder rows while the first page of results loads. */
const SKELETON_ROWS = 6;

export function ManageInstance({ id, tab }: { id: string; tab: ManageTab }) {
  const s = usePick((s) => ({
    instance: s.instances.find((i) => i.id === id) ?? null,
    modCount: s.localMods.length,
    busy: s.fileBusy || s.runningIds.includes(id) || s.launchPending === id || s.install?.instanceId === id,
    closeModal: s.closeModal,
    leaving: s.leaving,
    leave: s.leave,
  }));
  const serverCount = useServers((st) => st.lists[id]?.length ?? 0);
  const shotCount = useShots((st) => st.lists[id]?.length ?? 0);
  // Counts for the tab strip; each tab reloads its own list when shown.
  useEffect(() => {
    void useServers.getState().load(id);
    void useShots.getState().load(id);
  }, [id]);
  if (!s.instance) return null;
  const setTab = (next: ManageTab) => useStore.getState().setManageTab(next);
  // Jump to the download page for a kind of content, optionally searching.
  const browse = (kind: ContentKind, query = "") => {
    setTab("browse");
    const store = useStore.getState();
    useStore.setState({ modQuery: query });
    if (kind === "mod" && s.instance?.loader === "vanilla") store.setBrowseKind(kind);
    else store.searchMods(id, query, 0, kind);
  };

  return (
    <Modal
      wide
      onClose={s.closeModal}
      bodyClass="manage-body"
      scene={
        <SceneHeader
          seed={s.instance.id}
          eyebrow={t("管理世界", "Manage world")}
          title={s.instance.name}
          meta={
            <>
              <span className="mono">{s.instance.versionId}</span>
              <span className="dot" />
              {loaderLabel(s.instance.loader)}
              <span className="dot" />
              {formatRelative(s.instance.lastLaunchedAt)}
              {s.instance.playSeconds >= 60 && (
                <>
                  <span className="dot" />
                  {t("已玩 ", "Played ")}{formatPlayTime(s.instance.playSeconds)}
                </>
              )}
            </>
          }
          aside={<Identicon seed={s.instance.id} size={40} hero />}
          onClose={s.closeModal}
          // Every tab keeps the narrow strip; the forms and lists get the room.
          compact
          strip={
            <Tabs
              value={tab}
              onChange={setTab}
              tabs={[
                { value: "general", label: t("常规", "General") },
                { value: "options", label: t("游戏选项", "Game options") },
                { value: "mods", group: true, label: <>{t("模组", "Mods")}{s.modCount > 0 && <span className="tab-count">{s.modCount}</span>}</> },
                { value: "packs", label: t("资源包与光影", "Packs & shaders") },
                { value: "browse", label: t("下载", "Download") },
                { value: "data", group: true, label: t("存档", "Saves") },
                { value: "servers", label: <>{t("多人游戏", "Multiplayer")}{serverCount > 0 && <span className="tab-count">{serverCount}</span>}</> },
                { value: "shots", label: <>{t("截图", "Screenshots")}{shotCount > 0 && <span className="tab-count">{shotCount}</span>}</> },
              ]}
            />
          }
        />
      }
    >
      {s.busy && (
        <div className="manage-busy">
          <Sign tone="warn" unit={SMALL_PX} />
          {t("游戏运行、下载或文件操作期间不能修改世界", "The world can't be changed while the game runs, downloads, or file work is going on")}
        </div>
      )}
      <AnimatePresence mode="wait" initial={false}>
        <motion.div
          key={tab}
          className="manage-pane"
          initial={{ opacity: 0, x: 8 }}
          animate={{ opacity: 1, x: 0, transition: normal }}
          exit={{ opacity: 0, x: -6, transition: { duration: 0.1 } }}
        >
          {tab === "general" && <GeneralTab instance={s.instance} busy={s.busy} />}
          {tab === "mods" && <ModsTab instance={s.instance} busy={s.busy} browse={browse} />}
          {tab === "packs" && <PacksTab instance={s.instance} browse={browse} />}
          {tab === "browse" && <BrowseTab instance={s.instance} busy={s.busy} />}
          {tab === "data" && <DataTab instance={s.instance} busy={s.busy} />}
          {tab === "options" && <OptionsTab instance={s.instance} busy={s.busy} />}
          {tab === "servers" && <ServersTab instance={s.instance} busy={s.busy} />}
          {tab === "shots" && <ScreenshotsTab instance={s.instance} />}
        </motion.div>
      </AnimatePresence>
      {s.leaving && <LeaveBar quitting={s.leaving === "quit"} onAnswer={(save) => void s.leave(save)} />}
    </Modal>
  );
}

/** Asks before unsaved edits are thrown away; it covers the tab's footer.
    Quitting the launcher asks here too, then goes on quitting. */
function LeaveBar({ quitting, onAnswer }: { quitting: boolean; onAnswer: (save: boolean) => void }) {
  const ref = useRef<HTMLDivElement>(null);
  // Enter saves, the way the footer's own button would.
  useEffect(() => ref.current?.querySelector<HTMLButtonElement>(".btn-primary")?.focus(), []);
  return (
    <div ref={ref} className="save-bar manage-leave" role="alertdialog" aria-label={quitting ? t("退出前保存修改？", "Save changes before quitting?") : t("离开前保存修改？", "Save changes before leaving?")}>
      <span className="save-bar-mark" />
      <span className="save-bar-text">{quitting ? t("退出前保存修改？", "Save changes before quitting?") : t("离开前保存修改？", "Save changes before leaving?")}</span>
      <div className="spacer" />
      <Button variant="hero" size="sm" onClick={() => useStore.setState({ leaving: null })}>
        {quitting ? t("取消退出", "Don't quit") : t("继续编辑", "Keep editing")}
      </Button>
      <Button variant="hero" size="sm" onClick={() => onAnswer(false)}>
        {quitting ? t("放弃并退出", "Discard and quit") : t("放弃修改", "Discard")}
      </Button>
      <Button variant="primary" size="sm" icon="check" onClick={() => onAnswer(true)}>
        {quitting ? t("保存并退出", "Save and quit") : t("保存", "Save")}
      </Button>
    </div>
  );
}

type JavaMode = "auto" | "inherit" | "custom";

function GeneralTab({ instance, busy }: { instance: Instance; busy: boolean }) {
  const s = usePick((s) => ({
    globalMemory: s.settings.memoryMb,
    memoryAuto: s.settings.memoryAuto,
    operationTask: s.operationTask,
    fileBusy: s.fileBusy,
    mutate: s.mutate,
    fileOperation: s.fileOperation,
    toast: s.toast,
    closeModal: s.closeModal,
    chooseFiles: s.chooseFiles,
    downloadJava: s.downloadJava,
    scanJavaDirectory: s.scanJavaDirectory,
    accounts: s.accounts,
    globalLaunch: s.settings.launch,
    versions: s.versions,
    versionsLoaded: s.versionsLoaded,
    loadVersions: s.loadVersions,
  }));
  const [name, setName] = useState(instance.name);
  const [launch, setLaunch] = useState(instance.launch);
  const [accountId, setAccountId] = useState(instance.accountId ?? "");
  const [targetVersion, setTargetVersion] = useState("");
  const [confirmVersion, setConfirmVersion] = useState(false);
  const launchOk = launchProblem(launch) === null;
  useEffect(() => {
    if (!s.versionsLoaded) s.loadVersions();
  }, []);
  const target = s.versions.find((v) => v.id === targetVersion.trim()) ?? null;
  const [memory, setMemory] = useState(instance.memoryMb ? String(instance.memoryMb) : "");
  const [loader, setLoader] = useState(instance.loader);
  const [loaderVersion, setLoaderVersion] = useState<string | null>(instance.loaderVersion);
  const [java, setJava] = useState(instance.javaPath ?? "");
  // Only a new loader has to be checked against the online list; renaming
  // a Fabric world or changing its memory works offline.
  const loaderChanged = loader !== instance.loader || loaderVersion !== instance.loaderVersion;
  const loaderOk = useLoaderSelectionValid(loader, loaderVersion) || !loaderChanged;
  const mode: JavaMode = java === "auto" ? "auto" : java === "" ? "inherit" : "custom";
  const memoryValue = memory.trim() === "" ? null : Number(memory.trim());
  const memoryOk = memoryValue == null || (Number.isInteger(memoryValue) && memoryValue >= 512 && memoryValue <= 32768);
  const autoMemory = useAutoMemory(instance) ?? s.globalMemory;
  const memoryHint = !memoryOk
    ? t("需要是 512–32768 之间的整数", "Must be a whole number from 512 to 32768")
    : s.memoryAuto
      ? t(`留空使用自动（约 ${autoMemory} MB）`, `Leave empty for automatic (about ${autoMemory} MB)`)
      : t(`留空使用全局设置（${s.globalMemory} MB）`, `Leave empty for the global setting (${s.globalMemory} MB)`);
  const [saving, setSaving] = useState(false);
  const [javaOpen, setJavaOpen] = useState(mode === "custom");
  const saved = {
    name: instance.name,
    memory: instance.memoryMb ? String(instance.memoryMb) : "",
    java: instance.javaPath ?? "",
    accountId: instance.accountId ?? "",
  };
  const dirty =
    name.trim() !== saved.name ||
    memory.trim() !== saved.memory ||
    java.trim() !== saved.java ||
    accountId !== saved.accountId ||
    loader !== instance.loader ||
    loaderVersion !== instance.loaderVersion ||
    JSON.stringify(launch) !== JSON.stringify(instance.launch);

  const save = async () => {
    if (!memoryOk) {
      s.toast(t("内存请输入 512–32768 之间的整数（MB）", "Memory must be a whole number from 512 to 32768 (MB)"), "error");
      return false;
    }
    if (!launchOk) {
      s.toast(launchProblem(launch)!, "error");
      return false;
    }
    if (!loaderOk) {
      s.toast(t("加载器版本列表还没加载成功，换加载器前请先刷新列表", "The loader versions haven't loaded; refresh the list before switching loaders"), "error");
      return false;
    }
    setSaving(true);
    const ok =
      (await s.mutate("configure_instance", { id: instance.id, name: name.trim(), memoryMb: memoryValue, javaPath: java.trim() || null, loader, loaderVersion })) &&
      (await s.mutate("configure_launch", { id: instance.id, launch, accountId: accountId || null }, t("世界设置已保存", "World settings saved")));
    setSaving(false);
    if (ok) {
      // What was saved is the new starting point.
      setName(name.trim());
      setMemory(memory.trim());
      setJava(java.trim());
    }
    return ok;
  };
  const revert = () => {
    setName(saved.name);
    setMemory(saved.memory);
    setJava(saved.java);
    setAccountId(saved.accountId);
    setLoader(instance.loader);
    setLoaderVersion(instance.loaderVersion);
    setLaunch(instance.launch);
  };
  useFormGuard(dirty, save);

  // Another Minecraft version: the loader is kept, the world installs again.
  const changeVersion = async () => {
    if (!target) return;
    if (!confirmVersion) return setConfirmVersion(true);
    setConfirmVersion(false);
    // Every save is backed up first, in the background, then the version changes.
    await s.fileOperation(instance.id, { kind: "changeVersion", versionId: target.id, versionType: target.versionType });
    setTargetVersion("");
  };

  return (
    <>
      <div className="manage-scroll">
        <div className="manage-grid">
          <Field label={t("名称", "Name")}>
            <Input value={name} disabled={busy} onChange={(e) => setName(e.target.value)} />
          </Field>
          <Field label={t("内存（MB）", "Memory (MB)")} hint={memoryHint} tone={memoryOk ? undefined : "bad"}>
            <Input mono value={memory} disabled={busy} invalid={!memoryOk} placeholder={s.memoryAuto ? t(`自动 ${autoMemory}`, `Auto ${autoMemory}`) : t(`全局 ${s.globalMemory}`, `Global ${s.globalMemory}`)} onChange={(e) => setMemory(e.target.value)} />
          </Field>
        </div>
        <Field
          label={t("Minecraft 版本", "Minecraft version")}
          hint={
            confirmVersion
              ? t("再点一次确认。换之前会自动把每个存档备份一份；模组会保留，但不一定适配新版本。", "Click again to confirm. Every save is backed up first; mods are kept but may not work with the new version.")
              : t(`现在是 ${instance.versionId}。换版本后需要重新安装，加载器保留并重新匹配版本。`, `Currently ${instance.versionId}. After switching, the world installs again; the loader stays and picks a matching version.`)
          }
          tone={confirmVersion ? "bad" : undefined}
        >
          <div className="row">
            <VersionCombo
              value={targetVersion}
              versions={s.versions}
              current={instance.versionId}
              disabled={busy}
              placeholder={s.versionsLoaded ? t("输入或选择版本号，如 1.21.4", "Type or pick a version, e.g. 1.21.4") : t("正在读取版本列表…", "Loading versions…")}
              onChange={(value) => {
                setTargetVersion(value);
                setConfirmVersion(false);
              }}
            />
            <Button
              variant={confirmVersion ? "danger" : "default"}
              disabled={busy || !target || target.id === instance.versionId}
              onBlur={() => setConfirmVersion(false)}
              onClick={() => void changeVersion()}
            >
              {confirmVersion ? t(`确认换成 ${target?.id}`, `Confirm switch to ${target?.id}`) : t("更换版本", "Switch version")}
            </Button>
          </div>
        </Field>
        {s.accounts.length > 1 && (
          <Field label={t("游玩账号", "Play as")} hint={t("这个世界固定用哪个账号进入；跟随时用账号页正在使用的账号", "The account this world always uses; \"follow\" uses the one selected on the Accounts page")}>
            <div>
              <Segmented
                value={accountId}
                disabled={busy}
                options={[{ value: "", label: t("跟随当前账号", "Follow current account") }, ...s.accounts.map((a) => ({ value: a.id, label: a.username }))]}
                onChange={setAccountId}
              />
            </div>
          </Field>
        )}
        <Field label={t("世界外观", "World look")} hint={t("只改变启动器里这个世界的背景，立即生效，不影响游戏本身", "Changes only this world's backdrop in the launcher, right away; the game itself is unaffected")}>
          <BiomePicker seed={instance.id} />
        </Field>
        <Field label={t("加载器", "Loader")}>
          <LoaderPicker
            loader={loader}
            version={loaderVersion}
            minecraft={instance.versionId}
            onLoader={setLoader}
            onVersion={setLoaderVersion}
            disabled={busy}
            hint={t("切换后需要重新安装；存档和模组文件会保留", "Switching means installing again; saves and mod files are kept")}
          />
        </Field>
        <div className="launch-options">
          <button type="button" className="launch-options-toggle" aria-expanded={javaOpen} onClick={() => setJavaOpen(!javaOpen)}>
            <Icon name={javaOpen ? "chevronDown" : "chevronRight"} size={13} />
            Java
            {!javaOpen && <span className="chip">{mode === "auto" ? t("自动匹配", "Automatic") : mode === "inherit" ? t("继承全局", "Use global") : t("指定", "Custom")}</span>}
          </button>
          {javaOpen && (
            <Field label={t("使用哪个 Java", "Which Java")} hint={t("自动匹配按 Minecraft 元数据选择 Java；指定的 Java 版本不对时，会提示所需的主版本。", "Automatic picks Java from Minecraft's metadata; if a chosen Java is the wrong version, you'll be told which one is needed.")}>
              <div className="row">
                <Segmented
                  value={mode}
                  disabled={busy}
                  options={[
                    { value: "auto", label: t("自动匹配", "Automatic") },
                    { value: "inherit", label: t("继承全局", "Use global") },
                    { value: "custom", label: t("指定", "Custom") },
                  ]}
                  onChange={(next) => setJava(next === "auto" ? "auto" : next === "inherit" ? "" : java === "auto" ? "" : java)}
                />
                <div className="spacer" />
                <Button
                  size="sm"
                  icon="folder"
                  disabled={busy}
                  onClick={async () => {
                    const [path] = await s.chooseFiles("java");
                    if (path) setJava(path);
                  }}
                >
                  {t("选择文件", "Choose file")}
                </Button>
              </div>
              {mode === "custom" && <Input mono value={java} disabled={busy} placeholder={t("Java 可执行文件路径", "Path to the Java executable")} onChange={(e) => setJava(e.target.value)} />}
              <JavaPicker value={java} onChange={setJava} maxHeight={130} />
              <div className="row" style={{ flexWrap: "wrap" }}>
                <span className="field-label" style={{ marginRight: 4 }}>
                  {t("准备运行时", "Get a runtime")}
                </span>
                {[8, 17, 21, 25].map((major) => (
                  <Button key={major} size="sm" icon="download" disabled={s.fileBusy} onClick={() => void s.downloadJava(major)}>
                    Java {major}
                  </Button>
                ))}
                <Button size="sm" variant="ghost" icon="search" onClick={() => void s.scanJavaDirectory()}>
                  {t("扫描目录", "Scan folder")}
                </Button>
                {s.operationTask && (
                  <Button size="sm" variant="danger-ghost" onClick={() => void useStore.getState().run("cancel_task", { taskId: s.operationTask })}>
                    {t("取消任务", "Cancel task")}
                  </Button>
                )}
              </div>
            </Field>
          )}
        </div>
        <LaunchOptions value={launch} onChange={setLaunch} disabled={busy} inherited={s.globalLaunch} />
      </div>
      <div className="modal-footer">
        <span className="muted" style={{ fontSize: "var(--text-xs)", marginRight: "auto" }}>
          {t("世界外观和更换版本立即生效；其余修改点保存后生效", "World look and version switches apply right away; the rest applies when you save")}
        </span>
        <Button variant="ghost" disabled={busy || !dirty} onClick={revert}>
          {t("还原", "Revert")}
        </Button>
        <Button variant="primary" icon="check" disabled={busy || !dirty || !loaderOk || !memoryOk || !launchOk} busy={saving} onClick={() => void save()}>
          {t("保存", "Save")}
        </Button>
      </div>
    </>
  );
}

/** How many rows the version list shows before asking for a narrower query. */
const VERSION_ROWS = 60;

/** A version field that filters as you type, in the launcher's own list
    style: releases while empty, every matching version (snapshots, old
    betas) once something is typed. */
function VersionCombo({ value, versions, current, disabled, placeholder, onChange }: {
  value: string;
  versions: VersionSummary[];
  current: string;
  disabled: boolean;
  placeholder: string;
  onChange: (value: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const list = useRef<HTMLDivElement>(null);
  const query = value.trim().toLowerCase();
  const matches = useMemo(
    () =>
      query
        ? versions.filter((v) => v.id.toLowerCase().includes(query) || (updateName(v.id) ?? "").toLowerCase().includes(query))
        : versions.filter((v) => v.versionType === "release"),
    [versions, query],
  );
  const shown = matches.slice(0, VERSION_ROWS);
  useEffect(() => setActive(0), [query]);
  useEffect(() => {
    list.current?.querySelector<HTMLElement>(`[data-index="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }, [active, open]);
  const pick = (v: VersionSummary) => {
    onChange(v.id);
    setOpen(false);
  };
  const onKey = (e: React.KeyboardEvent) => {
    if (e.nativeEvent.isComposing) return;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!open) return setOpen(true);
      const step = e.key === "ArrowDown" ? 1 : -1;
      setActive((i) => Math.min(shown.length - 1, Math.max(0, i + step)));
    } else if (e.key === "Enter" && open && shown[active]) {
      e.preventDefault();
      pick(shown[active]);
    } else if (e.key === "Escape" && open) {
      // Only the list closes, not the dialog behind it.
      e.stopPropagation();
      setOpen(false);
    }
  };
  return (
    <div className="version-combo">
      <Input
        mono
        role="combobox"
        aria-expanded={open}
        aria-controls="manage-version-list"
        aria-activedescendant={open && shown[active] ? `manage-version-${active}` : undefined}
        aria-autocomplete="list"
        disabled={disabled}
        value={value}
        placeholder={placeholder}
        onChange={(e) => {
          onChange(e.target.value);
          setOpen(true);
        }}
        onFocus={() => setOpen(true)}
        onBlur={() => setOpen(false)}
        onKeyDown={onKey}
      />
      {open && versions.length > 0 && (
        <div className="version-combo-list" id="manage-version-list" role="listbox" ref={list}>
          {shown.length === 0 ? (
            <div className="version-combo-note">{t("没有匹配的版本", "No matching versions")}</div>
          ) : (
            shown.map((v, index) => (
              <button
                key={v.id}
                id={`manage-version-${index}`}
                data-index={index}
                type="button"
                role="option"
                tabIndex={-1}
                className="version-row"
                aria-selected={index === active}
                // Keeps focus in the field, so the list doesn't close before the click lands.
                onMouseDown={(e) => e.preventDefault()}
                onMouseEnter={() => setActive(index)}
                onClick={() => pick(v)}
              >
                <span className="mono version-combo-id">{v.id}</span>
                {updateName(v.id) && v.versionType === "release" ? <span className="version-update">{updateName(v.id)}</span> : <span className="muted">{versionTypeLabel(v.versionType)}</span>}
                {v.id === current && <span className="badge-latest">{t("当前", "Current")}</span>}
                <span className="spacer" />
                <span className="mono faint">{formatReleaseDate(v.releaseTime)}</span>
              </button>
            ))
          )}
          {matches.length > shown.length ? (
            <div className="version-combo-note">{t(`还有 ${matches.length - shown.length} 个，继续输入缩小范围`, `${matches.length - shown.length} more; keep typing to narrow it down`)}</div>
          ) : (
            !query && <div className="version-combo-note">{t("输入版本号可以找到快照和远古版本", "Type to find snapshots and old versions")}</div>
          )}
        </div>
      )}
    </div>
  );
}

function NeedsLoader() {
  return (
    <Notice tone="warn" title={t("原版不会加载模组", "Vanilla doesn't load mods")}>
      {t("请先在「常规」中选择并保存 Fabric、Quilt、Forge 或 NeoForge。", "Choose and save Fabric, Quilt, Forge or NeoForge under General first.")}
    </Notice>
  );
}

/** The running download or file operation, with a way to cancel it. */
function OperationLine() {
  const s = usePick((s) => ({ task: s.operationTask, busy: s.fileBusy }));
  if (!s.task || !s.busy) return null;
  return (
    <div className="row muted" style={{ fontSize: "var(--text-sm)" }}>
      <span className="spinner" />{t(" 正在下载并校验文件…", " Downloading and verifying files…")}
      <div className="spacer" />
      <Button size="sm" variant="danger-ghost" onClick={() => void useStore.getState().run("cancel_task", { taskId: s.task })}>
        {t("取消", "Cancel")}
      </Button>
    </div>
  );
}

type ModFilter = "all" | "updates" | "disabled" | "broken";

/** A readable name for a mod file Modrinth does not know. */
function fileTitle(name: string): string {
  return name.replace(/\.disabled$/, "").replace(/\.jar$/i, "");
}

type ModRow = { mod: LocalMod; info?: ModInsight; meta?: ModMeta; named?: ModName; title: string };

/** Mod names sort the way they read, numbers by value. */
const titleOrder = new Intl.Collator(isEnglish ? "en" : "zh-Hans-CN", { numeric: true, sensitivity: "base" });

/** How long the latest install or update stays offered for undoing. */
const UNDO_DAYS = 7;

function ModsTab({ instance, busy, browse }: { instance: Instance; busy: boolean; browse: (kind: ContentKind, query?: string) => void }) {
  const s = usePick((s) => ({
    mods: s.localMods,
    insights: s.modInsights,
    chooseFiles: s.chooseFiles,
    fileOperation: s.fileOperation,
    open: s.open,
    openUrl: s.openUrl,
    inspectMods: s.inspectMods,
    updateMods: s.updateMods,
    openModal: s.openModal,
  }));
  const [filter, setFilter] = useState<ModFilter>("all");
  const [query, setQuery] = useState("");
  // Selection mode: several mods on, off, updated or removed at once.
  const [picked, setPicked] = useState<Set<string> | null>(null);
  // The row opened to its details, and a disable or removal that would
  // leave other mods without what they need, waiting for a yes.
  const [opened, setOpened] = useState<string | null>(null);
  const [warning, setWarning] = useState<{ names: string[]; action: "disable" | "remove"; run: () => void } | null>(null);
  const [metas, setMetas] = useState<Record<string, ModMeta>>({});
  const [lastChange, setLastChange] = useState<LastModChange | null>(null);
  const vanilla = instance.loader === "vanilla";
  const icons = useModIcons(instance.id, vanilla ? [] : s.mods);
  // Identify the files once per visit; later checks are on request.
  useEffect(() => {
    if (!vanilla && s.mods.length > 0 && !s.insights.checked && !s.insights.loading) void s.inspectMods();
  }, [vanilla, s.mods.length, s.insights.checked, s.insights.loading]);
  // Names and dependencies from inside the jars, and the update that undo
  // would take back: both follow the folder.
  useEffect(() => {
    if (vanilla) return;
    let live = true;
    call<Record<string, ModMeta>>("mod_meta", { id: instance.id }).then((found) => live && setMetas(found), () => {});
    call<LastModChange | null>("last_mod_change", { id: instance.id }).then((found) => live && setLastChange(found), () => {});
    setWarning(null);
    return () => {
      live = false;
    };
  }, [vanilla, instance.id, s.mods]);

  if (vanilla)
    return (
      <div className="manage-scroll">
        <NeedsLoader />
      </div>
    );

  const rows: ModRow[] = s.mods
    .map((mod) => {
      const info = s.insights.byFile[mod.name];
      const meta = metas[mod.name];
      const named = info ? (findModName(info.slug) ?? findModName(undefined, info.title)) : meta ? findModName(meta.id, meta.name) : undefined;
      return { mod, info, meta, named, title: info?.title ?? meta?.name ?? fileTitle(mod.name) };
    })
    // By the name shown, not the file name, so "[1.21]JEI-…" doesn't lead.
    .sort((a, b) => titleOrder.compare(a.title, b.title));
  // Which files answer for each mod id.
  const suppliers = new Map<string, ModRow[]>();
  for (const row of rows)
    for (const id of row.meta ? [row.meta.id, ...row.meta.provides] : [])
      if (id) suppliers.set(id, [...(suppliers.get(id) ?? []), row]);
  // One entry per file: Sodium needs two Fabric API modules, not two Fabric APIs.
  const needs = (row: ModRow) => {
    const all = (row.meta?.depends ?? []).map((dep) => {
      const by = suppliers.get(dep) ?? [];
      const on = by.find((other) => other.mod.enabled);
      return { dep, row: on ?? by[0], state: on ? "on" : by.length > 0 ? "off" : "missing" } as const;
    });
    return all.filter((need, index) => !need.row || all.findIndex((other) => other.row === need.row) === index);
  };
  const broken = (row: ModRow) => (row.mod.enabled ? needs(row).filter((need) => need.state !== "on") : []);
  /** Enabled mods that could no longer load once these files are off. */
  const needing = (names: string[]) => {
    const gone = new Set(names);
    return rows.filter(
      (row) =>
        row.mod.enabled &&
        !gone.has(row.mod.name) &&
        (row.meta?.depends ?? []).some((dep) => {
          const on = (suppliers.get(dep) ?? []).filter((other) => other.mod.enabled);
          return on.length > 0 && on.every((other) => gone.has(other.mod.name));
        }),
    );
  };
  const guard = (names: string[], action: "disable" | "remove", run: () => void) => {
    if (needing(names).length > 0) setWarning({ names, action, run });
    else run();
  };

  const updates = rows.filter((row) => row.info?.update);
  const disabled = rows.filter((row) => !row.mod.enabled).length;
  const brokenCount = rows.filter((row) => broken(row).length > 0).length;
  const words = queryWords(query);
  const shown = rows.filter(
    (row) =>
      (filter === "all" || (filter === "updates" ? !!row.info?.update : filter === "broken" ? broken(row).length > 0 : !row.mod.enabled)) &&
      (words.length === 0 || !!matchItem(row.title, [row.mod.name, row.named?.zh ?? "", row.meta?.id ?? ""], words)),
  );
  const chosen = picked ? rows.filter((row) => picked.has(row.mod.name)) : [];
  const chosenUpdates = chosen.filter((row) => row.info?.update);
  const pick = (name: string) =>
    setPicked((old) => {
      const next = new Set(old);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  const batch = async (operation: Parameters<typeof s.fileOperation>[1], quiet?: boolean) => {
    await s.fileOperation(instance.id, operation, quiet);
    setPicked(null);
  };
  const recent = lastChange && Date.now() - lastChange.createdMs < UNDO_DAYS * 86_400_000 ? lastChange : null;
  const state = s.insights.loading ? (
    <>
      <span className="spinner" />{t(" 正在 Modrinth 上识别…", " Identifying on Modrinth…")}
    </>
  ) : s.insights.error ? (
    t("没能连上 Modrinth，名称和更新暂时看不到", "Couldn't reach Modrinth; names and updates aren't available for now")
  ) : updates.length > 0 ? (
    <span className="good">{updates.length}{t(" 个可以更新", " can be updated")}</span>
  ) : s.insights.checked && s.mods.length > 0 ? (
    t("都是最新版本", "All up to date")
  ) : (
    t("禁用会把文件改名为 .disabled，不会删除", "Disabling renames the file to .disabled; nothing is deleted")
  );
  const warningBar = (names: string[]) =>
    warning && warning.names.join("\n") === names.join("\n") ? (
      <ModWarning
        action={warning.action}
        needing={needing(warning.names).map((row) => row.title)}
        onCancel={() => setWarning(null)}
        onConfirm={() => {
          setWarning(null);
          warning.run();
        }}
      />
    ) : null;

  return (
    <div className="manage-scroll">
      {/* The count, filters and selection stay put while a long list scrolls. */}
      <div className="mods-sticky">
        <div className="mods-head">
          <span className="mods-count mono">{s.mods.length}</span>
          <div className="col" style={{ gap: 2, minWidth: 0 }}>
            <span className="mods-title">
              {t("个模组 · ", " mods · ")}{s.mods.length - disabled}{t(" 个启用", " enabled")}
            </span>
            <span className="mods-state">{state}</span>
          </div>
          <div className="spacer" />
          <Button size="sm" variant="ghost" icon="refresh" busy={s.insights.loading} disabled={s.mods.length === 0} onClick={() => void s.inspectMods()}>
            {t("检查更新", "Check for updates")}
          </Button>
          {updates.length > 0 && (
            <Button size="sm" variant="primary" icon="download" disabled={busy} onClick={() => void s.updateMods(instance.id, updates.map((row) => row.info!.update!.versionId))}>
              {t("全部更新", "Update all")}
            </Button>
          )}
          <Button size="sm" variant="ghost" icon="edit" disabled={vanilla} onClick={() => s.openModal({ kind: "configs", id: instance.id })}>
            {t("配置文件", "Configs")}
          </Button>
          <Button size="sm" variant="ghost" icon="folder" aria-label={t("打开模组文件夹", "Open the mods folder")} title={t("打开模组文件夹", "Open the mods folder")} onClick={() => void s.open({ kind: "contentDir", id: instance.id, content: "mod" })} />
          {s.mods.length > 0 && (
            <Button size="sm" icon="search" onClick={() => browse("mod")}>
              {t("下载模组", "Find mods")}
            </Button>
          )}
          <Button
            size="sm"
            icon="plus"
            disabled={busy}
            onClick={async () => {
              const paths = await s.chooseFiles("mods");
              if (paths.length) void s.fileOperation(instance.id, { kind: "addMods", paths });
            }}
          >
            {t("添加 .jar", "Add .jar")}
          </Button>
        </div>
        <OperationLine />
        {s.mods.length > 0 && (
          <div className="row">
            <Segmented
              value={filter}
              onChange={setFilter}
              options={[
                { value: "all", label: t(`全部 ${s.mods.length}`, `All ${s.mods.length}`) },
                { value: "updates", label: t(`可更新 ${updates.length}`, `Updates ${updates.length}`) },
                { value: "disabled", label: t(`已禁用 ${disabled}`, `Disabled ${disabled}`) },
                ...(brokenCount > 0 || filter === "broken" ? [{ value: "broken" as const, label: t(`缺前置 ${brokenCount}`, `Missing deps ${brokenCount}`) }] : []),
              ]}
            />
            <div className="spacer" />
            {s.mods.length > 6 && (
              <div style={{ width: 220 }}>
                <Input icon="search" placeholder={t("筛选模组，可用拼音", "Filter mods")} value={query} onChange={(e) => setQuery(e.target.value)} />
              </div>
            )}
            <Button size="sm" variant={picked ? "default" : "ghost"} icon={picked ? "close" : "check"} onClick={() => setPicked(picked ? null : new Set())}>
              {picked ? t("完成", "Done") : t("多选", "Select")}
            </Button>
          </div>
        )}
        {picked && (
          <div className="mods-batch">
            <span className="mods-batch-count">{t(`已选 ${chosen.length} 个`, `${chosen.length} selected`)}</span>
            <button type="button" className="link" onClick={() => setPicked(chosen.length === shown.length ? new Set() : new Set(shown.map((row) => row.mod.name)))}>
              {chosen.length === shown.length && shown.length > 0 ? t("全不选", "Select none") : t("全选", "Select all")}
            </button>
            <div className="spacer" />
            <Button size="sm" variant="ghost" disabled={busy || chosen.length === 0} onClick={() => void batch({ kind: "setMods", names: chosen.map((row) => row.mod.name), enabled: true })}>
              {t("启用", "Enable")}
            </Button>
            <Button
              size="sm"
              variant="ghost"
              disabled={busy || chosen.length === 0}
              onClick={() => {
                const names = chosen.map((row) => row.mod.name);
                guard(names, "disable", () => void batch({ kind: "setMods", names, enabled: false }));
              }}
            >
              {t("禁用", "Disable")}
            </Button>
            {chosenUpdates.length > 0 && (
              <Button size="sm" icon="download" disabled={busy} onClick={() => { void s.updateMods(instance.id, chosenUpdates.map((row) => row.info!.update!.versionId)); setPicked(null); }}>
                {t(`更新 ${chosenUpdates.length} 个`, `Update ${chosenUpdates.length}`)}
              </Button>
            )}
            <Button
              size="sm"
              variant="danger"
              icon="trash"
              disabled={busy || chosen.length === 0}
              onClick={() => {
                const names = chosen.map((row) => row.mod.name);
                guard(names, "remove", () => void batch({ kind: "recycleMods", names }, true));
              }}
            >
              {t("移到回收站", "Move to recycle bin")}
            </Button>
          </div>
        )}
        {picked && warningBar(chosen.map((row) => row.mod.name))}
      </div>
      {recent && (
        <div className="mods-undo">
          <Icon name="archive" size={14} />
          <span className="truncate">
            {recent.replaced > 0 ? t(`上次更新了 ${recent.added} 个模组`, `Last update: ${recent.added} mods`) : t(`上次安装了 ${recent.added} 个模组`, `Last install: ${recent.added} mods`)}
            <span className="faint"> · {formatRelative(recent.createdMs)}</span>
          </span>
          <div className="spacer" />
          <Button size="sm" variant="ghost" disabled={busy} title={t("移除这次装进来的文件，换回被替换的旧文件", "Removes the files it added and brings back the ones it replaced")} onClick={() => void s.fileOperation(instance.id, { kind: "undoModUpdate" })}>
            {t("撤销这一次", "Undo it")}
          </Button>
        </div>
      )}
      {s.mods.length === 0 ? (
        <Empty
          art={<BrandMark size={44} />}
          title={t("还没有模组", "No mods yet")}
          actions={
            <Button size="sm" variant="primary" icon="search" onClick={() => browse("mod")}>
              {t("去下载模组", "Find mods")}
            </Button>
          }
        >
          {t("从 Modrinth 下载时，必需的前置模组会自动一起安装；也可以点「添加 .jar」导入本地文件。", "Required dependencies install along with mods from Modrinth; you can also add local files with \"Add .jar\".")}
        </Empty>
      ) : shown.length === 0 ? (
        <div className="saves-empty muted">{t("没有符合条件的模组。", "No mods match.")}</div>
      ) : (
        <div className="list mod-list">
          {shown.map((row) => {
            const { mod, info, named, title } = row;
            const open = !picked && opened === mod.name;
            const missing = broken(row);
            return (
              <div key={mod.name} className="mod-item">
                <div
                  className={`list-row mod-row${mod.enabled ? "" : " off"}${picked ? " picking clickable" : ""}${open ? " open" : ""}`}
                  aria-selected={picked ? picked.has(mod.name) : undefined}
                  onClick={picked ? () => pick(mod.name) : undefined}
                >
                  {picked && <span className={`pick-box${picked.has(mod.name) ? " on" : ""}`} aria-hidden>{picked.has(mod.name) && <Icon name="check" size={12} />}</span>}
                  <button type="button" className="mod-main" aria-expanded={picked ? undefined : open} tabIndex={picked ? -1 : undefined} onClick={picked ? undefined : () => setOpened(open ? null : mod.name)}>
                    {/* The jar's own icon shows at once and offline; Modrinth's when the jar has none. */}
                    <ModIcon src={icons[mod.name] ?? info?.iconUrl} seed={info?.projectId ?? mod.name} size={34} glyph="mod" />
                    <span className="mod-text">
                      <span className="mod-title truncate">
                        {title}
                        {!isEnglish && named && named.zh !== title && <span className="mod-zh"> · {named.zh}</span>}
                      </span>
                      <span className="mod-meta mono truncate">
                        {info ? `${info.versionNumber} · ` : row.meta?.version ? `${row.meta.version} · ` : ""}
                        {formatBytes(mod.bytes)} · {mod.name}
                      </span>
                    </span>
                  </button>
                  {missing.length > 0 && (
                    <span className="mod-broken" title={t(`缺少：${missing.map((need) => need.row?.title ?? need.dep).join("、")}`, `Missing: ${missing.map((need) => need.row?.title ?? need.dep).join(", ")}`)}>
                      {t("缺前置", "Missing dep")}
                    </span>
                  )}
                  {picked ? null : info?.update ? (
                    <button
                      type="button"
                      className="mod-update"
                      disabled={busy}
                      title={t(`更新到 ${info.update.versionNumber}`, `Update to ${info.update.versionNumber}`)}
                      onClick={() => void s.updateMods(instance.id, [info.update!.versionId])}
                    >
                      <Icon name="download" size={12} />
                      {t("更新", "Update")}
                    </button>
                  ) : !info && s.insights.checked && !s.insights.error ? (
                    <span className="mod-local" title={t("已配置的平台未识别这个文件，暂时无法检查更新", "Configured platforms haven't identified this file, so updates can't be checked yet")}>
                      {t("本地文件", "Local file")}
                    </span>
                  ) : null}
                  {!picked && (
                    <>
                      <Switch
                        label={mod.enabled ? t("禁用", "Disable") : t("启用", "Enable")}
                        checked={mod.enabled}
                        onChange={() => {
                          if (busy) return;
                          const toggle = () => void s.fileOperation(instance.id, { kind: "toggleMod", name: mod.name });
                          if (mod.enabled) guard([mod.name], "disable", toggle);
                          else toggle();
                        }}
                      />
                      <Button
                        size="sm"
                        variant="ghost"
                        icon="trash"
                        className="row-remove"
                        disabled={busy}
                        aria-label={t(`把 ${title} 移到回收站`, `Move ${title} to the recycle bin`)}
                        title={t("移到回收站", "Move to recycle bin")}
                        onClick={() => guard([mod.name], "remove", () => void s.fileOperation(instance.id, { kind: "recycle", item: "mod", name: mod.name }, true))}
                      />
                    </>
                  )}
                </div>
                {!picked && warningBar([mod.name])}
                {open && (
                  <ModDetail
                    row={row}
                    needs={needs(row)}
                    neededBy={rows.filter((other) => other !== row && row.meta && (other.meta?.depends ?? []).some((dep) => (suppliers.get(dep) ?? []).includes(row)))}
                    onFind={(dep) => browse("mod", dep)}
                    onOpenUrl={(url) => void s.openUrl(url)}
                  />
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

/** What disabling or removing would break, and the way through anyway. */
function ModWarning({ action, needing, onCancel, onConfirm }: { action: "disable" | "remove"; needing: string[]; onCancel: () => void; onConfirm: () => void }) {
  const names = needing.slice(0, 4).join(t("、", ", ")) + (needing.length > 4 ? t(` 等 ${needing.length} 个`, ` and ${needing.length - 4} more`) : "");
  return (
    <div className="mod-warn" role="alert">
      <Sign tone="warn" unit={SMALL_PX} />
      <span className="mod-warn-text">
        {action === "disable"
          ? t(`${names} 需要它，禁用后这些模组会加载失败。`, `${names} need it; they won't load once it's off.`)
          : t(`${names} 需要它，移走后这些模组会加载失败。`, `${names} need it; they won't load once it's gone.`)}
      </span>
      <Button size="sm" variant="ghost" onClick={onCancel}>
        {t("算了", "Cancel")}
      </Button>
      <Button size="sm" variant="danger-ghost" onClick={onConfirm}>
        {action === "disable" ? t("仍然禁用", "Disable anyway") : t("仍然移走", "Remove anyway")}
      </Button>
    </div>
  );
}

/** A mod opened in the list: what it is, what it needs, what needs it. */
function ModDetail({ row, needs, neededBy, onFind, onOpenUrl }: {
  row: ModRow;
  needs: { dep: string; row?: ModRow; state: "on" | "off" | "missing" }[];
  neededBy: ModRow[];
  onFind: (dep: string) => void;
  onOpenUrl: (url: string) => void;
}) {
  const { mod, info, meta } = row;
  const source = info && !info.projectId.startsWith("cf:") ? `https://modrinth.com/mod/${info.slug ?? info.projectId}` : null;
  const missing = needs.find((need) => need.state === "missing");
  const STATE = { on: "", off: t(" · 已禁用", " · disabled"), missing: t(" · 没装", " · not installed") };
  return (
    <div className="mod-detail">
      <p className="mod-detail-text selectable">{(!isEnglish && zhDescription(row.named?.slug ?? info?.slug)) || meta?.description || t("这个模组没有写简介。", "This mod has no description.")}</p>
      <dl className="mod-facts">
        {(info?.versionNumber ?? meta?.version) && (
          <>
            <dt>{t("版本", "Version")}</dt>
            <dd className="mono">{info?.versionNumber ?? meta?.version}</dd>
          </>
        )}
        {meta?.id && (
          <>
            <dt>{t("模组 ID", "Mod ID")}</dt>
            <dd className="mono selectable">{meta.id}</dd>
          </>
        )}
        <dt>{t("文件", "File")}</dt>
        <dd className="mono selectable">
          {mod.name} · {formatBytes(mod.bytes)}
        </dd>
      </dl>
      {needs.length > 0 && (
        <div className="mod-deps">
          <span className="mod-deps-label">{t("需要", "Needs")}</span>
          {needs.map((need) => (
            <span key={need.dep} className={`mod-dep ${need.state}`}>
              {need.row?.title ?? need.dep}
              {STATE[need.state]}
            </span>
          ))}
        </div>
      )}
      {neededBy.length > 0 && (
        <div className="mod-deps">
          <span className="mod-deps-label">{t("被需要", "Needed by")}</span>
          {neededBy.map((other) => (
            <span key={other.mod.name} className={`mod-dep${other.mod.enabled ? "" : " off"}`}>
              {other.title}
              {other.mod.enabled ? "" : STATE.off}
            </span>
          ))}
        </div>
      )}
      {(source || missing) && (
        <div className="row">
          {missing && (
            <Button size="sm" variant="go" icon="search" onClick={() => onFind(missing.dep)}>
              {t(`搜索缺少的 ${missing.dep}`, `Find ${missing.dep}`)}
            </Button>
          )}
          {source && (
            <Button size="sm" variant="ghost" icon="external" onClick={() => onOpenUrl(source)}>
              {t("在 Modrinth 查看", "View on Modrinth")}
            </Button>
          )}
        </div>
      )}
    </div>
  );
}

/** Whether this instance can load shader packs, and what is missing if not. */
function shaderSupport(instance: Instance, mods: LocalMod[]): { ok: boolean; text: string; need?: { slug: string; name: string } } {
  const state = (...names: string[]) => {
    const found = mods.filter((mod) => names.some((name) => mod.name.toLowerCase().includes(name)));
    return found.some((mod) => mod.enabled) ? "on" : found.length > 0 ? "off" : null;
  };
  const ready = t("进游戏后在「选项 → 视频设置 → 光影包」里选择。", "Pick one in game under Options → Video Settings → Shader Packs.");
  if (instance.loader === "vanilla") return { ok: false, text: t("原版不能加载光影包：需要 Fabric + Iris，或 Forge + Oculus。", "Vanilla can't load shader packs: you need Fabric + Iris, or Forge + Oculus.") };
  const [names, need] =
    instance.loader === "fabric" || instance.loader === "quilt"
      ? [["iris"], { slug: "iris", name: "Iris" }]
      : instance.loader === "neoforge"
        ? [["iris", "oculus"], { slug: "iris", name: "Iris" }]
        : [["oculus", "optifine"], { slug: "oculus", name: "Oculus" }];
  const loaded = state(...names);
  if (loaded === "on") return { ok: true, text: t(`已安装 ${need.name}，${ready}`, `${need.name} is installed. ${ready}`) };
  if (loaded === "off") return { ok: false, text: t(`${need.name} 已安装但被禁用，在「模组」页启用后就能加载光影包。`, `${need.name} is installed but disabled; enable it on the Mods tab to load shader packs.`) };
  return { ok: false, text: t(`${loaderLabel(instance.loader)} 需要先安装 ${need.name} 模组才能加载光影包。`, `${loaderLabel(instance.loader)} needs the ${need.name} mod to load shader packs.`), need };
}

type PackFilter = "all" | "active" | "inactive";

function PacksTab({ instance, browse }: { instance: Instance; browse: (kind: ContentKind, query?: string) => void }) {
  const s = usePick((s) => ({
    packs: s.localPacks,
    mods: s.localMods,
    open: s.open,
    chooseFiles: s.chooseFiles,
    fileOperation: s.fileOperation,
    installOptifine: s.installOptifine,
    busy: s.fileBusy || s.runningIds.includes(instance.id) || s.launchPending === instance.id,
  }));
  // Enabled resource packs, winning one first, as the game orders them.
  const order = (s.packs?.resourcepacks ?? []).filter((p) => p.active != null).sort((a, b) => a.active! - b.active!).map((p) => p.name);
  const setOrder = async (next: string[]) => {
    try {
      await call("set_resource_packs", { id: instance.id, packs: next });
      await useStore.getState().reloadPacks();
    } catch (error) {
      useStore.getState().toast(toError(error).message, "error");
    }
  };
  const move = (name: string, by: number) => {
    const at = order.indexOf(name);
    const next = [...order];
    next.splice(at, 1);
    next.splice(Math.max(0, Math.min(order.length - 1, at + by)), 0, name);
    void setOrder(next);
  };
  const shader = shaderSupport(instance, s.mods);
  const hasOptifine = (s.mods ?? []).some((mod) => mod.name.toLowerCase().startsWith("optifine"));
  const insights = usePackInsights(instance.id, s.packs);
  const [filter, setFilter] = useState<PackFilter>("all");
  const [query, setQuery] = useState("");
  const words = queryWords(query);
  const total = (s.packs?.resourcepacks.length ?? 0) + (s.packs?.shaderpacks.length ?? 0);
  // A pack is found by its file name or, once identified, its Modrinth title.
  const shown = (kind: "resourcepack" | "shader", pack: PackFile) =>
    (filter === "all" || (filter === "active") === (pack.active != null)) &&
    (words.length === 0 || matchItem(pack.name, [insights[`${kind}:${pack.name}`]?.title ?? ""], words) !== null);
  const sections = [
    {
      kind: "resourcepack" as const,
      glyph: "pack" as const,
      title: t("资源包", "Resource packs"),
      list: s.packs?.resourcepacks,
      hint: t("已启用的排在前面，上面的覆盖下面的；可以在这里启用、停用和调整顺序，下次进游戏生效。", "Enabled packs come first, upper ones override lower ones. Enable, disable and reorder them here; changes apply next time you play."),
      warn: false,
      need: undefined,
    },
    { kind: "shader" as const, glyph: "sun" as const, title: t("光影包", "Shader packs"), list: s.packs?.shaderpacks, hint: shader.text, warn: !shader.ok, need: shader.need },
  ];
  return (
    <div className="manage-scroll">
      {total > 3 && (
        <div className="row pack-tools">
          <div style={{ flex: 1 }}>
            <Input icon="search" placeholder={t("搜索资源包和光影，支持拼音", "Search resource packs and shaders")} value={query} onChange={(e) => setQuery(e.target.value)} />
          </div>
          <Segmented
            value={filter}
            onChange={setFilter}
            options={[
              { value: "all", label: t("全部", "All") },
              { value: "active", label: t("已启用", "Enabled") },
              { value: "inactive", label: t("未启用", "Not enabled") },
            ]}
          />
        </div>
      )}
      {sections.map((section) => {
        const visible = section.list?.filter((pack) => shown(section.kind, pack));
        return (
          <section key={section.kind} className="pack-section">
            <div className="pack-head">
              <span className={`pack-glyph ${section.kind}`}>
                <PixelIcon name={section.glyph} size={20} />
              </span>
              <div className="col" style={{ gap: 2, flex: 1, minWidth: 0 }}>
                <span className="pack-title">
                  {section.title}
                  {section.list && <span className="mono faint"> {section.list.length}</span>}
                </span>
                <span className={`pack-hint${section.warn ? " warn" : ""}`}>{section.hint}</span>
              </div>
              {section.need && (
                <Button size="sm" variant="ghost" icon="search" onClick={() => browse("mod", section.need!.slug)}>
                  {t("安装 ", "Install ")}{section.need.name}
                </Button>
              )}
              {section.kind === "shader" && ["vanilla", "forge", "fabric", "quilt"].includes(instance.loader) && !hasOptifine && (
                <Button
                  size="sm"
                  variant="ghost"
                  icon="download"
                  disabled={s.busy}
                  title={instance.loader === "vanilla" ? t("为原版安装 OptiFine，无需加载器", "Install OptiFine directly for vanilla") : instance.loader === "forge" ? t("装进模组文件夹，用 OptiFine 加载光影", "Puts OptiFine in the mods folder to load shaders") : t("OptiFine 加 OptiFabric；和 Sodium、Iris 不兼容", "OptiFine plus OptiFabric; doesn't work with Sodium or Iris")}
                  onClick={() => void s.installOptifine(instance.id)}
                >
                  {t("或装 OptiFine", "Or install OptiFine")}
                </Button>
              )}
              <Button
                size="sm"
                variant="ghost"
                icon="folder"
                aria-label={t(`打开${section.title}文件夹`, `Open the ${section.title} folder`)}
                title={t(`打开${section.title}文件夹`, `Open the ${section.title} folder`)}
                onClick={() => void s.open({ kind: "contentDir", id: instance.id, content: section.kind })}
              />
              <Button
                size="sm"
                variant="ghost"
                icon="plus"
                disabled={s.busy}
                title={t(`添加本地的${section.title} .zip`, `Add local ${section.title} (.zip)`)}
                onClick={async () => {
                  const item = section.kind === "shader" ? "shader" : "resourcePack";
                  const paths = await s.chooseFiles(item === "shader" ? "shaders" : "resourcePacks");
                  if (paths.length) void s.fileOperation(instance.id, { kind: "addPacks", item, paths });
                }}
              >
                {t("添加", "Add")}
              </Button>
              <Button size="sm" icon="download" onClick={() => browse(section.kind)}>
                {t("下载", "Download")}{section.kind === "shader" ? t("光影", "Shaders") : t("资源包", "Resource packs")}
              </Button>
            </div>
            {section.list == null ? (
              <div className="saves-empty muted">
                <span className="spinner" />{t(" 正在读取…", " Loading…")}
              </div>
            ) : section.list.length === 0 ? (
              <div className="saves-empty muted">{t("还没有", "No ")}{section.title}{t("。可以从 Modrinth 下载，或把 .zip 放进文件夹。", " yet. Download from Modrinth, or put .zip files in the folder.")}</div>
            ) : !visible?.length ? (
              <div className="saves-empty muted">{t("没有符合条件的", "No matching ")}{section.title}{t("。", ".")}</div>
            ) : (
              <div className={`pack-tiles${section.kind === "resourcepack" ? " rows" : ""}`}>
                {visible.map((pack, index) => {
                  const info = insights[`${section.kind}:${pack.name}`];
                  return (
                    <motion.div
                      key={pack.name}
                      className="pack-tile"
                      title={pack.name}
                      initial={{ opacity: 0, y: 4 }}
                      animate={{ opacity: 1, y: 0, transition: { ...normal, delay: Math.min(index, 10) * 0.025 } }}
                    >
                      {/* The pack's own picture as the game's list shows it; for a
                          shader pack (which has none) its Modrinth project's icon. */}
                      <ModIcon src={pack.icon ?? info?.iconUrl} seed={`${section.kind}:${pack.name}`} size={40} glyph={section.glyph} />
                      <div className="col" style={{ minWidth: 0, gap: 2, flex: 1 }}>
                        <span className="pack-name truncate">{info?.title ?? pack.name.replace(/\.zip$/i, "")}</span>
                        <span className="pack-meta mono truncate">
                          {info ? `${pack.name} · ` : pack.folder ? t("文件夹 · ", "Folder · ") : "ZIP · "}
                          {formatBytes(pack.bytes)}
                        </span>
                      </div>
                      {pack.active != null && (
                        <span className="pack-active" title={section.kind === "shader" ? t("游戏里正在使用的光影", "The shader pack in use in game") : t("游戏里已启用；数字越小越靠上，覆盖下面的包", "Enabled in game; lower numbers sit higher and override the packs below")}>
                          {section.kind === "shader" ? t("使用中", "In use") : t(`启用 #${pack.active}`, `On #${pack.active}`)}
                        </span>
                      )}
                      {section.kind === "resourcepack" && (
                        <span className="pack-order">
                          {pack.active != null ? (
                            <>
                              <Button size="sm" variant="ghost" icon="chevronUp" aria-label={t("上移", "Move up")} title={t("上移：覆盖更多", "Move up: overrides more")} disabled={s.busy || pack.active === 1} onClick={() => move(pack.name, -1)} />
                              <Button size="sm" variant="ghost" icon="chevronDown" aria-label={t("下移", "Move down")} title={t("下移", "Move down")} disabled={s.busy || pack.active === order.length} onClick={() => move(pack.name, 1)} />
                              <Button size="sm" variant="ghost" disabled={s.busy} onClick={() => void setOrder(order.filter((n) => n !== pack.name))}>
                                {t("停用", "Turn off")}
                              </Button>
                            </>
                          ) : (
                            <Button size="sm" variant="go" disabled={s.busy} onClick={() => void setOrder([pack.name, ...order])}>
                              {t("启用", "Enable")}
                            </Button>
                          )}
                        </span>
                      )}
                      <Button
                        size="sm"
                        variant="ghost"
                        icon="trash"
                        className="row-remove"
                        disabled={s.busy}
                        aria-label={t(`把 ${pack.name} 移到回收站`, `Move ${pack.name} to the recycle bin`)}
                        title={t("移到回收站", "Move to recycle bin")}
                        onClick={() => void s.fileOperation(instance.id, { kind: "recycle", item: section.kind === "shader" ? "shader" : "resourcePack", name: pack.name }, true)}
                      />
                    </motion.div>
                  );
                })}
              </div>
            )}
          </section>
        );
      })}
      <OperationLine />
    </div>
  );
}

interface ChineseResolution {
  query: string;
  candidates: ModName[];
  applied: ModName;
}

const KIND_OPTIONS: { value: ContentKind; label: string }[] = [
  { value: "mod", label: t("模组", "Mods") },
  { value: "resourcepack", label: t("资源包", "Resource packs") },
  { value: "shader", label: t("光影", "Shaders") },
  { value: "datapack", label: t("数据包", "Datapacks") },
  { value: "world", label: t("地图", "Maps") },
];

const KIND_PLACEHOLDER: Record<ContentKind, string> = {
  mod: t("搜索模组，支持中文名，例如 机械动力 / Sodium", "Search mods, e.g. Create / Sodium"),
  resourcepack: t("搜索资源包，例如 Fresh Animations / Faithful", "Search resource packs, e.g. Fresh Animations / Faithful"),
  shader: t("搜索光影，例如 Complementary / BSL", "Search shaders, e.g. Complementary / BSL"),
  datapack: t("搜索数据包，例如 Terralith / Incendium", "Search datapacks, e.g. Terralith / Incendium"),
  world: t("搜索地图，例如 SkyBlock / Parkour", "Search maps, e.g. SkyBlock / Parkour"),
};

const KIND_NAME: Record<ContentKind, string> = { mod: t("模组", "Mods"), resourcepack: t("资源包", "Resource packs"), shader: t("光影包", "Shader packs"), datapack: t("数据包", "Datapacks"), world: t("地图", "Maps") };

/** What stands in for a missing picture: a puzzle piece, a framed picture, the sun. */
const KIND_GLYPH = { mod: "mod", resourcepack: "pack", shader: "sun", datapack: "map", world: "block" } as const satisfies Record<ContentKind, PixelIconName>;

/** The shader loaders a shader pack supports, as readable tags. */
function shaderTags(categories: string[] | undefined): string[] {
  const names: Record<string, string> = { iris: "Iris", optifine: "OptiFine", canvas: "Canvas", vanilla: t("原版核心光影", "Vanilla core shaders") };
  return (categories ?? []).flatMap((c) => (names[c] ? [names[c]] : []));
}

const VERSION_TYPE: Record<string, string> = { beta: t("测试版", "Beta"), alpha: t("预览版", "Alpha") };

function BrowseTab({ instance, busy }: { instance: Instance; busy: boolean }) {
  const s = usePick((s) => ({
    search: s.modSearch,
    versions: s.modVersions,
    fileBusy: s.fileBusy,
    modQuery: s.modQuery,
    mods: s.localMods,
    searchMods: s.searchMods,
    setBrowseKind: s.setBrowseKind,
    loadModVersions: s.loadModVersions,
    installMod: s.installMod,
    insights: s.modInsights,
    packs: s.localPacks,
    inspectMods: s.inspectMods,
    curseforgeKey: s.settings.curseforgeKey,
    openSettings: s.openSettings,
    closeModal: s.closeModal,
  }));
  const [query, setQuery] = useState(s.modQuery);
  const source = s.search.source;
  const curseforge = source === "curseforge";
  const needsKey = curseforge && !s.curseforgeKey.trim();
  const packInsights = usePackInsights(instance.id, s.packs);
  const [chinese, setChinese] = useState<ChineseResolution | null>(null);
  const [noChineseMatch, setNoChineseMatch] = useState<string | null>(null);
  const [chosen, setChosen] = useState<ModName | null>(null);
  const [unfolded, setUnfolded] = useState<Record<string, boolean>>({});
  const kind = s.search.kind;
  const vanilla = instance.loader === "vanilla";
  const blocked = vanilla && kind === "mod";
  // Datapacks go into one save; the most recently played is preselected.
  const worlds = useWorlds(kind === "datapack" ? instance : null);
  const [save, setSave] = useState<string | null>(null);
  const saves = [...(worlds?.worlds ?? [])].sort((a, b) => (b.modifiedMs ?? 0) - (a.modifiedMs ?? 0));
  const target = saves.some((w) => w.folder === save) ? save : (saves[0]?.folder ?? null);
  const install = (version: string) => void s.installMod(instance.id, version, kind === "datapack" ? (target ?? undefined) : undefined);
  const noSave = kind === "datapack" && !target;

  // openManage (for example from a crash fix) only fills in modQuery and
  // starts a raw search. Resolve a Chinese query against the catalog and
  // replace that search with the matching Modrinth slug.
  useEffect(() => {
    const text = s.modQuery.trim();
    setQuery(s.modQuery);
    setChosen(null);
    if (!text || !hasChinese(text) || kind !== "mod") {
      setChinese(null);
      setNoChineseMatch(null);
      return;
    }
    const candidates = findChineseMods(text);
    if (candidates.length === 0) {
      setChinese(null);
      setNoChineseMatch(text);
      return;
    }
    const applied = candidates[0];
    setChinese({ query: text, candidates, applied });
    setNoChineseMatch(null);
    s.searchMods(instance.id, applied.slug, 0);
  }, [s.modQuery, instance.id]);

  // Opening the page shows what is popular for this version right away.
  useEffect(() => {
    if (s.search.key === "" && !blocked && !needsKey && !s.modQuery.trim()) s.searchMods(instance.id, "", 0);
  }, []);

  // Mods are matched to results by their Modrinth project (from the file
  // hashes) or, before that answer arrives, by the slug in the file name.
  useEffect(() => {
    if (kind === "mod" && !vanilla && s.mods.length > 0 && !s.insights.checked && !s.insights.loading) void s.inspectMods();
  }, [kind, s.mods.length]);
  const installed = (hit: ModProject): { update: boolean; version: string | null } | null => {
    if (kind === "mod") {
      const insight = Object.values(s.insights.byFile).find((info) => info?.projectId === hit.project_id);
      if (insight) return { update: Boolean(insight.update), version: insight.versionNumber };
      const bySlug = hit.slug && s.mods.some((mod) => slugOfFile(mod.name) === hit.slug);
      return bySlug ? { update: false, version: null } : null;
    }
    // Zipped packs identified by hash name their project. Others are matched
    // by name: files start with the pack's ("ComplementaryReimagined_r5.2.zip",
    // "§6Faithful 32x.zip"), so it must begin with its slug or title.
    // Datapacks live per save; the list here does not track them.
    if (kind === "datapack" || kind === "world") return null;
    if (Object.values(packInsights).some((info) => info.kind === kind && info.projectId === hit.project_id)) return { update: false, version: null };
    const list = kind === "shader" ? s.packs?.shaderpacks : s.packs?.resourcepacks;
    const words = [hit.slug, hit.title].map((w) => packKey(w ?? "")).filter((w) => w.length > 3);
    const found = list?.some((pack) => {
      const name = packKey(pack.name);
      return words.some((w) => name.startsWith(w));
    });
    return found ? { update: false, version: null } : null;
  };

  // The details pane is never empty: the first result is shown until one is picked.
  useEffect(() => {
    const first = s.search.hits[0];
    if (!first || s.search.loading) return;
    if (!s.search.hits.some((h) => h.project_id === s.versions.project)) s.loadModVersions(instance.id, first.project_id);
  }, [s.search.hits, s.search.loading]);

  const page = Math.floor(s.search.offset / PAGE_SIZE) + 1;
  const pages = Math.max(1, Math.ceil(s.search.total / PAGE_SIZE));
  const searched = s.search.key !== "";
  const project = s.search.hits.find((h) => h.project_id === s.versions.project);
  // The version of the picked mod in this world, if it is installed: its row
  // says so, newer ones update and older ones switch back.
  const current = kind === "mod" ? Object.values(s.insights.byFile).find((info) => info?.projectId === s.versions.project) : undefined;
  const currentIndex = current ? s.versions.versions.findIndex((v) => v.id === current.versionId) : -1;
  const installLabel = (index: number) =>
    !current
      ? t("安装", "Install")
      : currentIndex >= 0 && index < currentIndex
        ? t("更新到此版本", "Update to this")
        : t("换成此版本", "Switch to this");
  const [detail, setDetail] = useState<ModProject | null>(null);
  const shader = kind === "shader" ? shaderSupport(instance, s.mods) : null;

  const switchSource = (next: SearchSource) => {
    setDetail(null);
    if (next === source) return;
    setChinese(null);
    setNoChineseMatch(null);
    setChosen(null);
    if (next === "curseforge" && !s.curseforgeKey.trim()) {
      useStore.setState({ modSearch: { ...s.search, source: next, key: "", hits: [], total: 0 } });
      return;
    }
    if (blocked) useStore.setState({ modSearch: { ...s.search, source: next } });
    else s.searchMods(instance.id, query.trim(), 0, undefined, { source: next });
  };

  const switchKind = (next: ContentKind) => {
    setDetail(null);
    if (next === kind) return;
    setQuery("");
    setChinese(null);
    setNoChineseMatch(null);
    setChosen(null);
    if (vanilla && next === "mod") s.setBrowseKind(next);
    // Maps are only on CurseForge.
    else if (next === "world" && source !== "curseforge") {
      if (s.curseforgeKey.trim()) s.searchMods(instance.id, "", 0, next, { source: "curseforge" });
      else useStore.setState({ modSearch: { ...s.search, kind: next, source: "curseforge", key: "", hits: [], total: 0 } });
    } else s.searchMods(instance.id, "", 0, next);
  };

  /** Searches what is typed; `refine` also changes the filters or order. */
  const search = (offset: number, refine?: { filters?: FilterChoice; sort?: SearchSort }) => {
    setDetail(null);
    const text = query.trim();
    if (kind !== "mod" || !hasChinese(text)) {
      setChinese(null);
      setNoChineseMatch(null);
      setChosen(null);
      s.searchMods(instance.id, text, offset, undefined, refine);
      return;
    }
    const candidates = findChineseMods(text);
    if (candidates.length === 0) {
      setChinese(null);
      setNoChineseMatch(text);
      s.searchMods(instance.id, text, offset, undefined, refine);
      return;
    }
    const applied = chosen && candidates.some((mod) => mod.slug === chosen.slug) ? chosen : candidates[0];
    setChinese({ query: text, candidates, applied });
    setNoChineseMatch(null);
    s.searchMods(instance.id, applied.slug, offset, undefined, refine);
  };

  // One choice per group; picking the chosen one again clears it.
  const choose = (group: string, value: string | null) => {
    const filters = { ...s.search.filters };
    if (value && filters[group] !== value) filters[group] = value;
    else delete filters[group];
    search(0, { filters });
  };

  const pick = (result: ChineseResolution, mod: ModName) => {
    setQuery(result.query);
    setChosen(mod);
    setChinese({ ...result, applied: mod });
    setNoChineseMatch(null);
    s.searchMods(instance.id, mod.slug, 0);
  };

  return (
    <div className="manage-scroll">
      <div className="row">
        <Segmented value={kind} onChange={switchKind} options={KIND_OPTIONS} />
        <Segmented
          value={source}
          onChange={switchSource}
          options={[
            { value: "modrinth", label: "Modrinth" },
            { value: "curseforge", label: "CurseForge" },
          ]}
        />
        <span className="faint" style={{ fontSize: "var(--text-xs)" }}>
          {t("只显示适配 ", "Showing only results for ")}{instance.versionId}
          {kind === "mod" && !vanilla ? ` · ${loaderLabel(instance.loader)}` : ""}{t(" 的结果", "")}
        </span>
      </div>
      {blocked ? (
        <NeedsLoader />
      ) : needsKey ? (
        <Notice
          tone="warn"
          title={t("CurseForge 需要 API Key", "CurseForge needs an API key")}
          actions={
            <Button
              size="sm"
              onClick={() => {
                s.closeModal();
                s.openSettings("download");
              }}
            >
              {t("去设置", "Open settings")}
            </Button>
          }
        >
          {t("CurseForge 的接口要求每个应用自带密钥。在 console.curseforge.com 免费申请后，填到「设置 → 下载」里即可搜索和安装。", "CurseForge's API needs a key per app. Get one free at console.curseforge.com and enter it under Settings → Downloads to search and install.")}
        </Notice>
      ) : (
        <>
          <form
            className="row"
            onSubmit={(e) => {
              e.preventDefault();
              search(0);
            }}
          >
            <div style={{ flex: 1 }}>
              <Input
                icon="search"
                placeholder={KIND_PLACEHOLDER[kind]}
                value={query}
                onChange={(e) => {
                  setQuery(e.target.value);
                  setChosen(null);
                }}
              />
            </div>
            <Button type="submit" busy={s.search.loading}>
              {s.search.loading ? t("搜索中…", "Searching…") : t("搜索", "Search")}
            </Button>
          </form>
          {kind === "datapack" && (
            <div className="filter-row" role="group" aria-label={t("装进存档", "Install into save")}>
              <span className="filter-label">{t("装进存档", "Into save")}</span>
              <div className="filter-chips">
                {saves.length === 0 ? (
                  <span className="faint" style={{ fontSize: "var(--text-xs)" }}>
                    {worlds ? t("这个世界还没有存档。先进一次游戏创建存档，再来装数据包。", "This world has no saves yet. Play once to create one, then add datapacks.") : t("正在读取存档…", "Reading saves…")}
                  </span>
                ) : (
                  saves.slice(0, 12).map((w) => (
                    <button key={w.folder} type="button" className="filter-chip" aria-pressed={w.folder === target} onClick={() => setSave(w.folder)}>
                      {w.name || w.folder}
                    </button>
                  ))
                )}
              </div>
            </div>
          )}
          <div className="browse-filters">
            {!curseforge && FILTER_GROUPS[kind].map((group) => {
              // Long groups show their common tags on one row; the rest
              // (and the chosen one, wherever it is) behind "更多".
              const folds = group.options.length > FOLDED + 2;
              const open = !folds || unfolded[group.id];
              const shown = open ? group.options : group.options.filter((o, i) => i < FOLDED || o.value === s.search.filters[group.id]);
              return (
                <div key={group.id} className="filter-row" role="group" aria-label={group.label}>
                  <span className="filter-label">{group.label}</span>
                  <div className="filter-chips">
                    <button type="button" className="filter-chip" aria-pressed={!s.search.filters[group.id]} onClick={() => choose(group.id, null)}>
                      {t("全部", "All")}
                    </button>
                    {shown.map((o) => (
                      <button key={o.value} type="button" className="filter-chip" aria-pressed={s.search.filters[group.id] === o.value} onClick={() => choose(group.id, o.value)}>
                        {o.label}
                      </button>
                    ))}
                    {folds && (
                      <button
                        type="button"
                        className="filter-chip filter-more"
                        aria-expanded={open}
                        onClick={() => setUnfolded({ ...unfolded, [group.id]: !open })}
                      >
                        {open ? t("收起", "Less") : t(`更多 ${group.options.length - FOLDED}`, `${group.options.length - FOLDED} more`)}
                        <Icon name={open ? "chevronUp" : "chevronDown"} size={11} />
                      </button>
                    )}
                  </div>
                </div>
              );
            })}
            <div className="filter-row">
              <span className="filter-label">{t("排序", "Sort")}</span>
              <Segmented value={s.search.sort} onChange={(sort) => search(0, { sort })} options={SORTS} />
              {searched && !s.search.loading && <span className="filter-total faint">{t("共 ", "")}{formatCount(s.search.total)}{t(" 个结果", " results")}</span>}
            </div>
          </div>
          {shader && !shader.ok && (
            <Notice tone="warn" title={t("还不能加载光影", "Shaders can't load yet")}>
              <span className="row" style={{ gap: 10, flexWrap: "wrap" }}>
                {shader.text}
                {shader.need && (
                  <Button size="sm" variant="ghost" icon="search" onClick={() => switchToMod(shader.need!.slug)}>
                    {t("搜索 ", "Search ")}{shader.need.name}
                  </Button>
                )}
              </span>
            </Notice>
          )}
          {chinese && (
            <div className="muted" style={{ fontSize: "var(--text-sm)" }} aria-live="polite">
              {t("已按「", "Searched \"")}{chinese.applied.zh}{t("」搜索 ", "\" as ")}{chinese.applied.en}
            </div>
          )}
          {chinese && chinese.candidates.length > 1 && (
            <div className="row" style={{ flexWrap: "wrap", gap: 6 }}>
              <span className="faint" style={{ fontSize: "var(--text-xs)" }}>
                {t("匹配到多个模组：", "Several mods match: ")}
              </span>
              {chinese.candidates.map((mod) => (
                <Button key={mod.slug} size="sm" variant={mod.slug === chinese.applied.slug ? "primary" : "ghost"} onClick={() => pick(chinese, mod)}>
                  {mod.zh} · {mod.en}
                </Button>
              ))}
            </div>
          )}
          {noChineseMatch && (
            <Notice tone="warn" title={t("词典里没有这个中文名，试试英文名", "This Chinese name isn't in the dictionary; try the English name")}>
              {t("仍按「", "Still searching Modrinth for \"")}{noChineseMatch}{t("」在 Modrinth 中搜索。也可以直接输入英文名或 Modrinth 项目名。", "\". You can also type the English name or the Modrinth project name.")}
            </Notice>
          )}
          {s.search.error && (
            <Notice
              tone="danger"
              title={t(`没能连上 ${curseforge ? "CurseForge" : "Modrinth"}`, `Couldn't reach ${curseforge ? "CurseForge" : "Modrinth"}`)}
              actions={<Button size="sm" icon="refresh" onClick={() => search(0)}>{t("重试", "Try again")}</Button>}
            >
              {s.search.error}
            </Notice>
          )}
          <OperationLine />
          {!searched ? (
            <Empty icon="search" title={t(`从 ${curseforge ? "CurseForge" : "Modrinth"} 下载${KIND_NAME[kind]}`, `Download ${KIND_NAME[kind]} from ${curseforge ? "CurseForge" : "Modrinth"}`)}>
              {t("输入关键词后搜索；留空搜索可以浏览热门内容。", "Type keywords and search; search with nothing typed to browse what's popular.")}
            </Empty>
          ) : detail ? (
            <ProjectDetail project={detail} kind={kind} glyph={KIND_GLYPH[kind]} busy={busy || noSave} onInstall={install} onBack={() => setDetail(null)} />
          ) : (
            <div className="browse">
              {/* A new search keeps the old page dimmed until the answer arrives. */}
              <div className={`list browse-results${s.search.loading && s.search.hits.length > 0 ? " stale" : ""}`} aria-busy={s.search.loading}>
                {s.search.loading &&
                  s.search.hits.length === 0 &&
                  Array.from({ length: SKELETON_ROWS }, (_, i) => (
                    <div key={i} className="list-row browse-skeleton" aria-hidden>
                      <span className="skeleton skeleton-icon" />
                      <span className="col" style={{ flex: 1, gap: 7 }}>
                        <span className="skeleton" style={{ width: `${34 + ((i * 17) % 30)}%` }} />
                        <span className="skeleton" style={{ width: `${62 + ((i * 11) % 26)}%` }} />
                      </span>
                    </div>
                  ))}
                {s.search.hits.map((hit) => {
                  const named = kind === "mod" ? (findModName(hit.slug) ?? findModName(hit.project_id) ?? findModName(undefined, hit.title)) : undefined;
                  const tags = kind === "shader" ? shaderTags(hit.categories) : [];
                  const have = installed(hit);
                  return (
                    <button
                      key={hit.project_id}
                      type="button"
                      className="list-row clickable browse-hit"
                      aria-selected={hit.project_id === s.versions.project}
                      onClick={() => s.loadModVersions(instance.id, hit.project_id)}
                    >
                      <ModIcon src={hit.icon_url} seed={hit.project_id} size={38} glyph={KIND_GLYPH[kind]} />
                      <div className="col" style={{ flex: 1, gap: 2, textAlign: "left", minWidth: 0 }}>
                        <span className="browse-title truncate">
                          {hit.title}
                          {!isEnglish && named && named.zh !== hit.title && <span className="mod-zh"> · {named.zh}</span>}
                        </span>
                        <span className="muted truncate" style={{ fontSize: "var(--text-xs)" }}>
                          {(kind === "mod" && !isEnglish && zhDescription(named?.slug ?? hit.slug)) || hit.description}
                        </span>
                        {tags.length > 0 && (
                          <span className="browse-tags">
                            {tags.map((tag) => (
                              <span key={tag} className="browse-tag">
                                {tag}
                              </span>
                            ))}
                          </span>
                        )}
                      </div>
                      {have ? (
                        <span className={`browse-have${have.update ? " update" : ""}`} title={have.version ? t(`已安装 ${have.version}`, `Installed ${have.version}`) : undefined}>
                          {have.update ? t("可更新", "Update available") : t("已安装", "Installed")}
                        </span>
                      ) : (
                        <span className="mono faint" style={{ fontSize: "var(--text-xs)", whiteSpace: "nowrap" }}>
                          <Icon name="download" size={11} /> {formatCount(hit.downloads)}
                        </span>
                      )}
                      <Icon name="chevronRight" size={14} className="faint" />
                    </button>
                  );
                })}
                {s.search.hits.length === 0 && !s.search.loading && (
                  <div className="list-row faint" style={{ fontSize: "var(--text-sm)" }}>
                    {kind === "mod"
                      ? t(`没有找到兼容的模组；也可以换到 ${curseforge ? "Modrinth" : "CurseForge"} 找，或在「模组」页手动添加 .jar。`, `No compatible mods found; try ${curseforge ? "Modrinth" : "CurseForge"}, or add a .jar on the Mods tab.`)
                      : t(`没有找到适配 ${instance.versionId} 的${KIND_NAME[kind]}；也可以把 .zip 直接放进文件夹。`, `No ${KIND_NAME[kind]} found for ${instance.versionId}; you can also put .zip files straight into the folder.`)}
                  </div>
                )}
                {!(s.search.loading && s.search.hits.length === 0) && (
                  <div className="list-row pager">
                    <Button size="sm" variant="ghost" icon="chevronLeft" disabled={page <= 1 || s.search.loading} onClick={() => search(s.search.offset - PAGE_SIZE)}>
                      {t("上一页", "Previous")}
                    </Button>
                    <span className="mono faint" style={{ fontSize: "var(--text-xs)" }}>
                      {t(`${page} / ${pages} 页 · ${s.search.total} 个结果`, `Page ${page} of ${pages} · ${s.search.total} results`)}
                    </span>
                    <Button size="sm" variant="ghost" disabled={page >= pages || s.search.loading} onClick={() => search(s.search.offset + PAGE_SIZE)}>
                      {t("下一页 ", "Next ")}<Icon name="chevronRight" size={13} />
                    </Button>
                  </div>
                )}
              </div>
              <AnimatePresence mode="wait">
                {!s.versions.project && (
                  <motion.div key="hint" className="browse-versions browse-placeholder" initial={{ opacity: 0 }} animate={{ opacity: 1, transition: normal }} exit={{ opacity: 0 }}>
                    <PixelIcon name={kind === "mod" ? "block" : KIND_GLYPH[kind]} size={28} />
                    <span>{t("选择左侧任意一项，这里会列出适配 ", "Pick any result on the left to see its versions for ")}{instance.versionId}{t(" 的版本。", ".")}</span>
                  </motion.div>
                )}
                {s.versions.project && (
                  <motion.div key={s.versions.project} className="browse-versions card" initial={{ opacity: 0, x: 10 }} animate={{ opacity: 1, x: 0, transition: normal }} exit={{ opacity: 0 }}>
                    <div className="card-header browse-versions-head">
                      <ModIcon src={project?.icon_url} seed={s.versions.project} size={26} glyph={KIND_GLYPH[kind]} />
                      <span className="card-title truncate" style={{ flex: 1 }}>{project?.title ?? s.versions.project}</span>
                      {project && (
                        <Button size="sm" variant="ghost" icon="info" onClick={() => setDetail(project)}>
                          {t("介绍", "About")}
                        </Button>
                      )}
                    </div>
                    <div className="col" style={{ padding: 8, gap: 2 }}>
                      {s.versions.loading ? (
                        <div className="faint row" style={{ padding: 10 }}>
                          <span className="spinner" />{t(" 正在获取版本…", " Loading versions…")}
                        </div>
                      ) : s.versions.versions.length === 0 ? (
                        <div className="faint" style={{ padding: 10 }}>
                          {t("没有可安装的兼容版本。", "No compatible version to install.")}
                        </div>
                      ) : (
                        s.versions.versions.map((v, index) => (
                          <div key={v.id} className="picker-row flat">
                            <div className="col" style={{ flex: 1, minWidth: 0 }}>
                              <span className="row" style={{ gap: 6 }}>
                                <span className="mono truncate" style={{ fontWeight: 600 }}>
                                  {v.version_number}
                                </span>
                                {v.version_type && VERSION_TYPE[v.version_type] && <span className="version-flag">{VERSION_TYPE[v.version_type]}</span>}
                              </span>
                              <span className="faint truncate" style={{ fontSize: "var(--text-xs)" }}>
                                {v.loaders.join(" / ")} · {v.game_versions.slice(0, 3).join(", ")}
                              </span>
                            </div>
                            {current?.versionId === v.id ? (
                              <span className="browse-have" title={t("这个世界正在用的版本", "The version this world uses")}>
                                {t("当前", "Current")}
                              </span>
                            ) : (
                              <Button size="sm" icon={currentIndex >= 0 && index < currentIndex ? "refresh" : "download"} disabled={busy || s.fileBusy || noSave} onClick={() => install(v.id)}>
                                {installLabel(index)}
                              </Button>
                            )}
                          </div>
                        ))
                      )}
                      <span className="faint" style={{ fontSize: "var(--text-xs)", padding: "6px 10px" }}>
                        {kind === "mod"
                          ? t("安装时自动添加必需依赖", "Required dependencies are added automatically")
                          : kind === "datapack"
                            ? target
                              ? t(`下载到存档「${saves.find((w) => w.folder === target)?.name || target}」的 datapacks 文件夹，下次进入时生效`, `Downloads to the datapacks folder of "${saves.find((w) => w.folder === target)?.name || target}"; it applies next time you open it`)
                              : t("先选择要装进哪个存档", "Choose a save first")
                            : t(`下载到 ${kind === "shader" ? "shaderpacks" : "resourcepacks"} 文件夹`, `Downloads to the ${kind === "shader" ? "shaderpacks" : "resourcepacks"} folder`)}
                      </span>
                    </div>
                  </motion.div>
                )}
              </AnimatePresence>
            </div>
          )}
        </>
      )}
    </div>
  );

  function switchToMod(slug: string) {
    setQuery(slug);
    setChinese(null);
    setNoChineseMatch(null);
    setChosen(null);
    s.searchMods(instance.id, slug, 0, "mod");
  }
}

function DataTab({ instance, busy }: { instance: Instance; busy: boolean }) {
  const s = usePick((s) => ({
    fileOperation: s.fileOperation,
    chooseFiles: s.chooseFiles,
    open: s.open,
    installInstance: s.installInstance,
    openModal: s.openModal,
    filesChanged: s.filesChanged,
  }));
  const [pack, setPack] = useState<PackInfo | null>(null);
  // undefined while checking; null when the pack is current or can't be checked.
  const [newer, setNewer] = useState<ModVersion | null | undefined>(null);
  useEffect(() => {
    let live = true;
    setNewer(null);
    call<PackInfo | null>("modpack_info", { id: instance.id }).then(
      (info) => {
        if (!live) return;
        setPack(info);
        if (!info?.source) return;
        setNewer(undefined);
        call<ModVersion | null>("modpack_update", { id: instance.id }).then(
          (v) => live && setNewer(v),
          () => live && setNewer(null),
        );
      },
      () => live && setPack(null),
    );
    return () => {
      live = false;
    };
  }, [instance.id, s.filesChanged]);
  const chooseNewer = async () => {
    const [path] = await s.chooseFiles("pack");
    if (path) void s.fileOperation(instance.id, { kind: "updatePack", path });
  };
  const packRows: { icon: Parameters<typeof Icon>[0]["name"]; title: string; text: string; action: React.ReactNode }[] = [
    {
      icon: "external",
      title: t("导出为整合包", "Export as modpack"),
      text: t("打包成 Modrinth 的 .mrpack：Modrinth 上有的文件写成下载链接，其余模组和配置直接打包", "Makes a Modrinth .mrpack: files on Modrinth become download links, other mods and configs are packed in"),
      action: (
        <Button size="sm" disabled={busy} onClick={() => s.openModal({ kind: "exportPack", id: instance.id })}>
          {t("导出…", "Export…")}
        </Button>
      ),
    },
    {
      icon: "server",
      title: t("导出为服务端", "Export as server"),
      text: t("生成可以直接运行的服务器：官方服务端、服务器需要的模组和配置、启动脚本；只在客户端运行的模组会去掉", "Makes a server ready to run: the official server, the mods and configs it needs, start scripts; client-only mods are left out"),
      action: (
        <Button size="sm" disabled={busy} onClick={() => s.openModal({ kind: "exportServer", id: instance.id })}>
          {t("导出…", "Export…")}
        </Button>
      ),
    },
    {
      icon: "globe",
      title: t("部署到云服务器", "Deploy to a cloud server"),
      text: t("把这个世界连同存档和模组传到你租的 Linux 云服务器上一直运行；在这里开关、备份、看控制台，改了模组一键同步", "Uploads this world with its save and mods to a Linux cloud server you rent and keeps it running; start, stop, back up and use the console from here, and sync mods in one click"),
      action: (
        <Button size="sm" onClick={() => s.openModal({ kind: "remote", id: instance.id })}>
          {t("打开…", "Open…")}
        </Button>
      ),
    },
  ];
  if (pack) {
    const from = t(`来自「${pack.name}」${pack.version ? ` ${pack.version}` : ""}。`, `From "${pack.name}"${pack.version ? ` ${pack.version}` : ""}. `);
    const dropped = t("被新版去掉、你也没改过的模组会移到回收站", "mods the new version drops (and you didn't change) go to the recycle bin");
    packRows.unshift({
      icon: "download",
      title: newer ? t(`整合包有新版 ${newer.version_number}`, `Modpack ${newer.version_number} is out`) : t("更新整合包", "Update modpack"),
      text: !pack.source
        ? from + t(`选新版的 .mrpack 或 CurseForge .zip；${dropped}`, `Pick the new .mrpack or CurseForge .zip; ${dropped}`)
        : newer === undefined
          ? from + t("正在检查新版…", "Checking for a new version…")
          : newer
            ? from + t(`更新会下载新版并替换整合包文件；${dropped}`, `Updating downloads the new version and replaces the pack's files; ${dropped}`)
            : from + t("已是最新版本。也可以手动选择 .mrpack 或 CurseForge .zip", "This is the newest version. You can also pick an .mrpack or CurseForge .zip yourself"),
      action: (
        <>
          <Button size="sm" variant={newer ? "ghost" : undefined} disabled={busy} onClick={() => void chooseNewer()}>
            {t("选择文件…", "Choose file…")}
          </Button>
          {newer && (
            <Button size="sm" variant="go" icon="download" disabled={busy} title={newer.name} onClick={() => void s.fileOperation(instance.id, { kind: "updateModpackOnline", version: newer.id })}>
              {t("更新", "Update")}
            </Button>
          )}
        </>
      ),
    });
  }
  const rows: { icon: Parameters<typeof Icon>[0]["name"]; title: string; text: string; action: React.ReactNode }[] = [
    ...packRows,
    {
      icon: "archive",
      title: t("备份存档与配置", "Back up saves and settings"),
      text: t("把整个游戏目录打包成 ZIP，保存在启动器的备份目录", "Zips the whole game folder into the launcher's backups folder"),
      action: (
        <>
          <Button size="sm" variant="ghost" icon="folder" onClick={() => void s.open({ kind: "backupsDir", id: instance.id })}>
            {t("备份目录", "Backups folder")}
          </Button>
          <Button size="sm" disabled={busy} onClick={() => void s.fileOperation(instance.id, { kind: "backup" })}>
            {t("立即备份", "Back up now")}
          </Button>
        </>
      ),
    },
    {
      icon: "refresh",
      title: t("从备份恢复", "Restore from backup"),
      text: t("恢复前会自动备份当前数据；只能恢复同版本、同加载器的备份", "Current data is backed up first; only backups of the same version and loader can be restored"),
      action: (
        <Button
          size="sm"
          disabled={busy}
          onClick={async () => {
            const [path] = await s.chooseFiles("restore");
            if (path) void s.fileOperation(instance.id, { kind: "restoreBackup", path });
          }}
        >
          {t("选择备份…", "Choose backup…")}
        </Button>
      ),
    },
    {
      icon: "copy",
      title: t("复制世界", "Duplicate world"),
      text: t("连同存档、模组和设置复制一份新世界", "Copies saves, mods and settings into a new world"),
      action: (
        <Button size="sm" disabled={busy} onClick={() => void s.fileOperation(instance.id, { kind: "duplicate" })}>
          {t("复制", "Duplicate")}
        </Button>
      ),
    },
    {
      icon: "shield",
      title: t("校验并修复", "Verify and repair"),
      text: t("重新核对并下载缺失或损坏的游戏文件，不会改动存档", "Checks game files again and downloads missing or broken ones; saves are untouched"),
      action: (
        <Button size="sm" disabled={busy} onClick={() => void s.installInstance(instance.id)}>
          {t("开始校验", "Verify")}
        </Button>
      ),
    },
    {
      icon: "folder",
      title: t("游戏目录", "Game folder"),
      text: t("存档、截图、资源包和配置文件都在这里", "Saves, screenshots, resource packs and config files live here"),
      action: (
        <Button size="sm" onClick={() => void s.open({ kind: "gameDir", id: instance.id })}>
          {t("打开", "Open")}
        </Button>
      ),
    },
    {
      icon: "trash",
      title: t("移入回收站", "Move to recycle bin"),
      text: t("不会删除任何文件，可以在世界页的回收站中恢复", "Nothing is deleted; restore it from the recycle bin on the Worlds page"),
      action: (
        <Button size="sm" variant="danger-ghost" disabled={busy} onClick={() => s.openModal({ kind: "deleteInstance", instance })}>
          {t("移入回收站", "Move to recycle bin")}
        </Button>
      ),
    },
  ];
  return (
    <div className="manage-scroll">
      <Worlds instance={instance} />
      <div className="section-label">{t("世界数据", "World data")}</div>
      <div className="list">
        {rows.map((row) => (
          <div key={row.title} className="list-row" style={{ minHeight: 58 }}>
            <Icon name={row.icon} size={16} className="faint" />
            <div className="col" style={{ flex: 1, gap: 2 }}>
              <span style={{ fontWeight: 600 }}>{row.title}</span>
              <span className="muted" style={{ fontSize: "var(--text-xs)" }}>
                {row.text}
              </span>
            </div>
            <div className="row">{row.action}</div>
          </div>
        ))}
      </div>
    </div>
  );
}

/** The instance's singleplayer worlds, newest first, as small tiles. */
function Worlds({ instance }: { instance: Instance }) {
  const s = usePick((s) => ({
    open: s.open,
    launch: s.launchInstance,
    closeModal: s.closeModal,
    openModal: s.openModal,
    chooseFiles: s.chooseFiles,
    fileOperation: s.fileOperation,
    fileBusy: s.fileBusy,
    account: s.settings.activeAccountId,
    busy: s.runningIds.includes(instance.id) || s.launchPending != null || s.install?.instanceId === instance.id,
  }));
  const list = useWorlds(instance);
  const worlds = list?.worlds ?? null;
  const total = worlds?.reduce((sum, world) => sum + world.bytes, 0) ?? 0;
  return (
    <section className="saves">
      <div className="section-label">
        {t("存档", "Saves")}
        {worlds && worlds.length > 0 && (
          <span className="mono faint">
            {worlds.length}{t(" 个 · ", " · ")}{formatBytes(total)}
          </span>
        )}
        <span className="spacer" />
        <Button
          size="sm"
          variant="ghost"
          icon="archive"
          disabled={s.fileBusy || s.busy}
          title={t("从 .zip 导入存档；也能导入别处的存档文件夹", "Import a save from a .zip, or a save folder from elsewhere")}
          onClick={async () => {
            const [path] = await s.chooseFiles("saveZip");
            if (path) void s.fileOperation(instance.id, { kind: "importSave", path });
          }}
        >
          {t("导入 .zip", "Import .zip")}
        </Button>
        <Button
          size="sm"
          variant="ghost"
          icon="folder"
          disabled={s.fileBusy || s.busy}
          title={t("复制一个存档文件夹进来（里面有 level.dat）", "Copy in a save folder (one with level.dat)")}
          onClick={async () => {
            const [path] = await s.chooseFiles("saveFolder");
            if (path) void s.fileOperation(instance.id, { kind: "importSave", path });
          }}
        >
          {t("导入文件夹", "Import folder")}
        </Button>
      </div>
      {worlds == null ? (
        <div className="saves-empty muted">
          <span className="spinner" />{t(" 正在读取存档…", " Loading saves…")}
        </div>
      ) : worlds.length === 0 ? (
        <div className="saves-empty muted">
          <BrandMark size={28} />
          {t("还没有存档。进入游戏创建一个世界后，它会出现在这里。", "No saves yet. Create one in game and it appears here.")}
        </div>
      ) : (
        <div className="world-tiles">
          {worlds.map((world, index) => (
            <motion.div
              key={world.folder}
              className="world-tile clickable"
              role="button"
              tabIndex={0}
              title={t("管理这个存档：改名、备份、导出、数据包", "Manage this save: rename, back up, export, datapacks")}
              onClick={() => s.openModal({ kind: "save", id: instance.id, folder: world.folder })}
              onKeyDown={(e) => {
                if (e.key === "Enter" && e.target === e.currentTarget) s.openModal({ kind: "save", id: instance.id, folder: world.folder });
              }}
              initial={{ opacity: 0, y: 6 }}
              animate={{ opacity: 1, y: 0, transition: { ...normal, delay: Math.min(index, 8) * 0.03 } }}
            >
              <span className="world-tile-icon">{world.icon ? <img src={world.icon} alt="" /> : <Identicon seed={`${instance.id}:${world.folder}`} size={40} />}</span>
              <span className="col" style={{ minWidth: 0, gap: 2 }}>
                <span className="world-tile-name truncate">{world.name ?? world.folder}</span>
                <span className="world-tile-meta mono">
                  {formatBytes(world.bytes)} · {world.modifiedMs ? formatRelative(world.modifiedMs).replace(t("尚未游玩", "Never played"), "—") : "—"}
                </span>
              </span>
              <span className="world-tile-actions" onClick={(e) => e.stopPropagation()}>
                <Button size="sm" variant="ghost" icon="folder" aria-label={t("打开存档文件夹", "Open the save folder")} title={t("打开存档文件夹", "Open the save folder")} onClick={() => void s.open({ kind: "world", id: instance.id, folder: world.folder })} />
                <Button
                  size="sm"
                  variant="go"
                  icon="play"
                  disabled={!instance.installed || !s.account || s.busy || !list?.quickPlay}
                  title={
                    !instance.installed
                      ? t("先安装这个世界", "Install this world first")
                      : !list?.quickPlay
                        ? t("直接进入存档需要 Minecraft 1.20 或更高版本", "Opening a save directly needs Minecraft 1.20 or later")
                        : !s.account
                          ? t("先添加账号", "Add an account first")
                          : t("开始游戏并直接进入这个存档", "Start the game straight into this save")
                  }
                  onClick={() => {
                    s.closeModal();
                    void s.launch(instance.id, world.folder);
                  }}
                >
                  {t("进入存档", "Open save")}
                </Button>
              </span>
            </motion.div>
          ))}
        </div>
      )}
    </section>
  );
}

/** A pack name to compare: colour codes, case and punctuation dropped. */
function packKey(name: string): string {
  return name
    .replace(/§./g, "")
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, "");
}
