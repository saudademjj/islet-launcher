import { useEffect, useMemo, useRef, useState } from "react";

import type { Instance, VersionSummary } from "../../api/types";
import { BiomePicker } from "../../components/BiomePicker";
import { Icon } from "../../components/Icon";
import { JavaPicker } from "../../components/JavaPicker";
import { LaunchOptions, launchProblem } from "../../components/LaunchOptions";
import { LoaderPicker, useLoaderSelectionValid } from "../../components/LoaderPicker";
import { Button, Field, Input, Segmented } from "../../components/ui";
import { formatReleaseDate, updateName, versionTypeLabel } from "../../lib/format";
import { useFormGuard } from "../../lib/formGuard";
import { t } from "../../lib/i18n";
import { useAutoMemory, usePick, useStore } from "../../store";

type JavaMode = "auto" | "inherit" | "custom";

export function GeneralTab({ instance, busy }: { instance: Instance; busy: boolean }) {
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
  const prevVersionRef = useRef(instance.versionId);
  useEffect(() => {
    if (prevVersionRef.current !== instance.versionId) {
      prevVersionRef.current = instance.versionId;
      setLoader(instance.loader);
      setLoaderVersion(instance.loaderVersion);
    }
  }, [instance.versionId, instance.loader, instance.loaderVersion]);
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
              ? t("再点一次确认。更换前会自动备份存档，旧模组可能不兼容新版本。", "Click again to confirm. Saves are backed up first; mods may not match the new version.")
              : t(`当前 ${instance.versionId}，更换后将重新安装并匹配加载器。`, `Currently ${instance.versionId}. Switching reinstalls and updates the loader.`)
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
          <Field label={t("游玩账号", "Play as")} hint={t("跟随：用账号页正在使用的账号", "Follow: the one selected on the Accounts page")}>
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
        <Field label={t("世界外观", "World look")} hint={t("仅用于启动器背景展示", "Backdrop shown inside the launcher")}>
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
            hint={t("切换后需重新安装，存档和模组会保留", "Switching reinstalls the world; saves and mods are kept")}
          />
        </Field>
        <div className="launch-options">
          <button type="button" className="launch-options-toggle" aria-expanded={javaOpen} onClick={() => setJavaOpen(!javaOpen)}>
            <Icon name={javaOpen ? "chevronDown" : "chevronRight"} />
            Java
            {!javaOpen && <span className="chip">{mode === "auto" ? t("自动匹配", "Automatic") : mode === "inherit" ? t("继承全局", "Use global") : t("指定", "Custom")}</span>}
          </button>
          {javaOpen && (
            <Field label={t("使用哪个 Java", "Which Java")} hint={t("自动匹配版本所需的 Java，缺少时自动下载", "Automatically picks the required Java version and downloads if missing")}>
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
          {t("修改后点击保存生效", "Click Save to apply changes")}
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
