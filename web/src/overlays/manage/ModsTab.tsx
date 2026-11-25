import { useDeferredValue, useEffect, useMemo, useState } from "react";

import { call } from "../../api/backend";
import type { ContentKind, Instance, LastModChange, LocalMod, ModInsight, ModMeta } from "../../api/types";
import { Icon } from "../../components/Icon";
import { ModIcon } from "../../components/ModIcon";
import { VirtualList } from "../../components/overlay";
import { BrandMark, Sign } from "../../components/Pixel";
import { Button, Empty, Input, Segmented, Switch } from "../../components/ui";
import { useModIcons } from "../../lib/contentIcons";
import { formatBytes, formatRelative, loaderLabel } from "../../lib/format";
import { isEnglish, t } from "../../lib/i18n";
import { findModName, zhDescription, type ModName } from "../../lib/modNames";
import { SMALL_PX } from "../../lib/pixelScale";
import { matchItem, queryWords } from "../../lib/search";
import { usePick } from "../../store";
import { isPluginServer, NeedsLoader, OperationLine } from "./shared";

type ModFilter = "all" | "updates" | "disabled" | "broken";

/** Collapsed row height (55px row + 1px bottom divider). */
const MOD_ROW_HEIGHT = 56;

/** A readable name for a mod file Modrinth does not know. */
function fileTitle(name: string): string {
  return name.replace(/\.disabled$/, "").replace(/\.jar$/i, "");
}

type ModRow = { mod: LocalMod; info?: ModInsight; meta?: ModMeta; named?: ModName; title: string };

/** Mod names sort the way they read, numbers by value. */
const titleOrder = new Intl.Collator(isEnglish ? "en" : "zh-Hans-CN", { numeric: true, sensitivity: "base" });

/** How long the latest install or update stays offered for undoing. */
const UNDO_DAYS = 7;

export function ModsTab({ instance, busy, browse }: { instance: Instance; busy: boolean; browse: (kind: ContentKind, query?: string) => void }) {
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
  const deferredQuery = useDeferredValue(query);
  // Selection mode: several mods on, off, updated or removed at once.
  const [picked, setPicked] = useState<Set<string> | null>(null);
  // The row opened to its details, and a disable or removal that would
  // leave other mods without what they need, waiting for a yes.
  const [opened, setOpened] = useState<string | null>(null);
  const [warning, setWarning] = useState<{ names: string[]; action: "disable" | "remove"; run: () => void } | null>(null);
  const [metas, setMetas] = useState<Record<string, ModMeta>>({});
  const [lastChange, setLastChange] = useState<LastModChange | null>(null);
  const vanilla = instance.loader === "vanilla";
  const plugins = isPluginServer(instance.loader);
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

  // Precompute sorted rows, dependency graph and preflight issues once per
  // mod/metadata change in O(N), so searching or multi-selecting stays instant
  // even on 500+ mod packs.
  const analysis = useMemo(() => {
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
    const enabledById = new Map<string, ModRow[]>();
    for (const row of rows) {
      if (row.meta) {
        const ids = [row.meta.id, ...row.meta.provides];
        for (const id of ids) {
          if (!id) continue;
          let list = suppliers.get(id);
          if (!list) {
            list = [];
            suppliers.set(id, list);
          }
          list.push(row);
        }
        if (row.mod.enabled && row.meta.id) {
          let dup = enabledById.get(row.meta.id);
          if (!dup) {
            dup = [];
            enabledById.set(row.meta.id, dup);
          }
          dup.push(row);
        }
      }
    }
    const hasConnector = ["connector", "connectormod"].some((id) => (suppliers.get(id) ?? []).some((r) => r.mod.enabled));
    const allowedMap: Record<string, string[]> = {
      fabric: ["fabric"],
      legacyfabric: ["fabric", "legacyfabric"],
      quilt: ["quilt", "fabric"],
      forge: hasConnector ? ["forge", "fabric"] : ["forge"],
      cleanroom: ["forge", "cleanroom"],
      neoforge: instance.versionId === "1.20.1"
        ? (hasConnector ? ["neoforge", "forge", "fabric"] : ["neoforge", "forge"])
        : (hasConnector ? ["neoforge", "fabric"] : ["neoforge"]),
      liteloader: ["liteloader"],
    };
    const validLoaders = allowedMap[instance.loader];
    const loaderFits = (jarLoaders?: string[]) =>
      plugins || !jarLoaders || jarLoaders.length === 0 || !validLoaders || jarLoaders.some((l) => validLoaders.includes(l));

    type NeedEntry = { dep: string; row?: ModRow; state: "on" | "off" | "missing" };
    type RowAudit = { needs: NeedEntry[]; missing: NeedEntry[]; dups: ModRow[]; badLoader: boolean; hasIssue: boolean };
    const audits = new Map<string, RowAudit>();
    const updates: ModRow[] = [];
    let disabled = 0;
    let brokenCount = 0;
    for (const row of rows) {
      if (row.info?.update) updates.push(row);
      if (!row.mod.enabled) disabled++;
      // One entry per file: Sodium needs two Fabric API modules, not two Fabric APIs.
      const seenRows = new Set<ModRow>();
      const rowNeeds: NeedEntry[] = [];
      for (const dep of row.meta?.depends ?? []) {
        const by = suppliers.get(dep) ?? [];
        const on = by.find((other) => other.mod.enabled);
        const provider = on ?? by[0];
        if (provider) {
          if (seenRows.has(provider)) continue;
          seenRows.add(provider);
        }
        rowNeeds.push({ dep, row: provider, state: on ? "on" : by.length > 0 ? "off" : "missing" });
      }
      const missing = row.mod.enabled ? rowNeeds.filter((need) => need.state !== "on") : [];
      const sameId = row.mod.enabled && row.meta?.id ? (enabledById.get(row.meta.id) ?? []) : [];
      const dups = sameId.length > 1 ? sameId.filter((other) => other.mod.name !== row.mod.name) : [];
      const badLoader = row.mod.enabled && !loaderFits(row.meta?.loaders);
      const hasIssue = missing.length > 0 || dups.length > 0 || badLoader;
      if (hasIssue) brokenCount++;
      audits.set(row.mod.name, { needs: rowNeeds, missing, dups, badLoader, hasIssue });
    }
    return { rows, suppliers, audits, updates, disabled, brokenCount };
  }, [s.mods, s.insights.byFile, metas, instance.loader, instance.versionId, plugins]);

  const { rows, suppliers, audits, updates, disabled, brokenCount } = analysis;
  const shown = useMemo(() => {
    const words = queryWords(deferredQuery);
    return rows.filter((row) => {
      const hasIssue = audits.get(row.mod.name)?.hasIssue ?? false;
      return (
        (filter === "all" || (filter === "updates" ? !!row.info?.update : filter === "broken" ? hasIssue : !row.mod.enabled)) &&
        (words.length === 0 || !!matchItem(row.title, [row.mod.name, row.named?.zh ?? "", row.meta?.id ?? ""], words))
      );
    });
  }, [rows, audits, filter, deferredQuery]);

  // At most one row in the virtualized list is expanded at a time (either its
  // detail panel, an inline single-mod warning, or both on the same row).
  const expandedName = !picked
    ? (warning && warning.names.length === 1 ? warning.names[0] : opened)
    : null;
  const expandedIndex = useMemo(() => {
    if (!expandedName) return null;
    const idx = shown.findIndex((row) => row.mod.name === expandedName);
    return idx >= 0 ? idx : null;
  }, [shown, expandedName]);

  if (vanilla)
    return (
      <div className="manage-scroll">
        <NeedsLoader />
      </div>
    );

  const auditOf = (row: ModRow) => audits.get(row.mod.name) ?? { needs: [], missing: [], dups: [], badLoader: false, hasIssue: false };
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
    if (needing(names).length > 0) {
      if (names.length === 1 && opened && opened !== names[0]) setOpened(null);
      setWarning({ names, action, run });
    } else {
      run();
    }
  };

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
    t("暂时无法连接 Modrinth，无法检查更新", "Couldn't reach Modrinth; update checks unavailable")
  ) : updates.length > 0 ? (
    <span className="good">{updates.length}{t(" 个可以更新", " can be updated")}</span>
  ) : s.insights.checked && s.mods.length > 0 ? (
    t("都是最新版本", "All up to date")
  ) : (
    t("禁用后不会删除文件，可随时重新启用", "Disabled mods aren't deleted and can be re-enabled anytime")
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
    <div className="mods-pane">
      <div className="mods-header">
        <div className="mods-head">
          <div className="mods-head-info">
            <span className="mods-count mono">{s.mods.length}</span>
            <div className="col gap-2 min-0">
              <span className="mods-title">
                {plugins ? t("个插件 · ", " plugins · ") : t("个模组 · ", " mods · ")}{s.mods.length - disabled}{t(" 个启用", " enabled")}
              </span>
              <span className="mods-state">{state}</span>
            </div>
          </div>
          <div className="mods-head-actions">
            <Button size="sm" variant="ghost" icon="refresh" busy={s.insights.loading} disabled={s.mods.length === 0} onClick={() => void s.inspectMods()}>
              {t("检查更新", "Check for updates")}
            </Button>
            {updates.length > 0 && (
              <Button size="sm" variant="primary" icon="download" disabled={busy} onClick={() => void s.updateMods(instance.id, updates.map((row) => row.info!.update!.versionId))}>
                {t("全部更新", "Update all")}
              </Button>
            )}
            {!plugins && (
              <Button size="sm" variant="ghost" icon="edit" disabled={vanilla} onClick={() => s.openModal({ kind: "configs", id: instance.id })}>
                {t("配置文件", "Configs")}
              </Button>
            )}
            <Button size="sm" variant="ghost" icon="folder" aria-label={plugins ? t("打开插件文件夹", "Open the plugins folder") : t("打开模组文件夹", "Open the mods folder")} title={plugins ? t("打开插件文件夹", "Open the plugins folder") : t("打开模组文件夹", "Open the mods folder")} onClick={() => void s.open({ kind: "contentDir", id: instance.id, content: "mod" })} />
            {s.mods.length > 0 && (
              <Button size="sm" icon="search" onClick={() => browse("mod")}>
                {plugins ? t("下载插件", "Find plugins") : t("下载模组", "Find mods")}
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
        </div>
        <OperationLine />
        {s.mods.length > 0 && (
          <div className="row wrap">
            <Segmented
              value={filter}
              onChange={(next) => {
                setWarning(null);
                setFilter(next);
              }}
              options={[
                { value: "all", label: t(`全部 ${s.mods.length}`, `All ${s.mods.length}`) },
                { value: "updates", label: t(`可更新 ${updates.length}`, `Updates ${updates.length}`) },
                { value: "disabled", label: t(`已禁用 ${disabled}`, `Disabled ${disabled}`) },
                ...(brokenCount > 0 || filter === "broken" ? [{ value: "broken" as const, label: t(`问题 ${brokenCount}`, `Issues ${brokenCount}`) }] : []),
              ]}
            />
            <div className="spacer" />
            {s.mods.length > 6 && (
              <div className="mods-search">
                <Input icon="search" placeholder={plugins ? t("筛选插件", "Filter plugins") : t("筛选模组，可用拼音", "Filter mods")} value={query} onChange={(e) => setQuery(e.target.value)} />
              </div>
            )}
            <Button
              size="sm"
              variant={picked ? "default" : "ghost"}
              icon={picked ? "close" : "check"}
              onClick={() => {
                setWarning(null);
                setPicked(picked ? null : new Set());
              }}
            >
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
              {t("移入回收站", "Move to recycle bin")}
            </Button>
          </div>
        )}
        {picked && warningBar(chosen.map((row) => row.mod.name))}
      </div>
      <div className="mods-body">
        {recent && (
          <div className="mods-undo">
            <Icon name="archive" />
            <span className="truncate">
              {recent.replaced > 0 ? t(`上次更新了 ${recent.added} 个模组`, `Last update: ${recent.added} mods`) : t(`上次安装了 ${recent.added} 个模组`, `Last install: ${recent.added} mods`)}
              <span className="faint"> · {formatRelative(recent.createdMs)}</span>
            </span>
            <div className="spacer" />
            <Button size="sm" variant="ghost" disabled={busy} title={t("恢复到上次更新前的模组", "Restore mods from before the last update")} onClick={() => void s.fileOperation(instance.id, { kind: "undoModUpdate" })}>
              {t("撤销这一次", "Undo it")}
            </Button>
          </div>
        )}
        {s.mods.length === 0 ? (
          <Empty
            art={<BrandMark size={44} />}
            title={plugins ? t("还没有插件", "No plugins yet") : t("还没有模组", "No mods yet")}
            actions={
              <Button size="sm" variant="primary" icon="search" onClick={() => browse("mod")}>
                {plugins ? t("去下载插件", "Find plugins") : t("去下载模组", "Find mods")}
              </Button>
            }
          >
            {plugins
              ? t("从 Modrinth 下载时，前置插件会一起装上", "Plugins they need install along with them from Modrinth")
              : t("从 Modrinth 下载时，前置模组会一起装上", "Mods they need install along with them from Modrinth")}
          </Empty>
        ) : shown.length === 0 ? (
          <div className="saves-empty muted">{plugins ? t("没有符合条件的插件。", "No plugins match.") : t("没有符合条件的模组。", "No mods match.")}</div>
        ) : (
          <div className="list mod-list mod-list-virtual">
            <VirtualList
              items={shown}
              rowHeight={MOD_ROW_HEIGHT}
              expandedIndex={expandedIndex}
              getKey={(row) => row.mod.name}
              resetScrollKey={`${filter}\0${deferredQuery}`}
              className="mod-virtual"
              render={(row, index) => {
                const { mod, info, named, title } = row;
                const isExpanded = !picked && expandedName === mod.name;
                const open = isExpanded && opened === mod.name;
                const { needs: rowNeeds, missing, dups, badLoader } = auditOf(row);
                return (
                  <div className={`mod-item${index === shown.length - 1 ? " last" : ""}`}>
                    <div
                      className={`list-row mod-row${mod.enabled ? "" : " off"}${picked ? " picking clickable" : ""}${open ? " open" : ""}`}
                      aria-selected={picked ? picked.has(mod.name) : undefined}
                      onClick={picked ? () => pick(mod.name) : undefined}
                    >
                      {picked && <span className={`pick-box${picked.has(mod.name) ? " on" : ""}`} aria-hidden>{picked.has(mod.name) && <Icon name="check" />}</span>}
                      <button
                        type="button"
                        className="mod-main"
                        aria-expanded={picked ? undefined : open}
                        tabIndex={picked ? -1 : undefined}
                        onClick={
                          picked
                            ? undefined
                            : () => {
                                if (warning && warning.names.length === 1 && warning.names[0] !== mod.name) setWarning(null);
                                setOpened(open ? null : mod.name);
                              }
                        }
                      >
                        {/* The jar's own icon shows at once and offline; Modrinth's when the jar has none. */}
                        <ModIcon src={icons[mod.name] ?? info?.iconUrl} seed={info?.projectId ?? mod.name} size={32} glyph="mod" />
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
                      {badLoader && (
                        <span className="mod-broken" title={t(`这是 ${(row.meta?.loaders ?? []).map(loaderLabel).join(" / ")} 模组，当前世界加载器为 ${loaderLabel(instance.loader)}`, `Built for ${(row.meta?.loaders ?? []).map(loaderLabel).join(" / ")}, while this world uses ${loaderLabel(instance.loader)}`)}>
                          {t("加载器不符", "Wrong loader")}
                        </span>
                      )}
                      {dups.length > 0 && (
                        <span className="mod-broken" title={t(`与 ${dups.map((d) => d.mod.name).join("、")} 模组 ID 相同`, `Same mod ID as ${dups.map((d) => d.mod.name).join(", ")}`)}>
                          {t("重复模组", "Duplicate")}
                        </span>
                      )}
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
                          <Icon name="download" />
                          {t("更新", "Update")}
                        </button>
                      ) : !info && s.insights.checked && !s.insights.error ? (
                        <span className="mod-local" title={t("未识别该文件，暂不支持检查更新", "Unrecognized file; update checks unavailable")}>
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
                            aria-label={t(`把 ${title} 移入回收站`, `Move ${title} to the recycle bin`)}
                            title={t("移入回收站", "Move to recycle bin")}
                            onClick={() => guard([mod.name], "remove", () => void s.fileOperation(instance.id, { kind: "recycle", item: "mod", name: mod.name }, true))}
                          />
                        </>
                      )}
                    </div>
                    {isExpanded && warningBar([mod.name])}
                    {open && (
                      <ModDetail
                        row={row}
                        loader={instance.loader}
                        badLoader={badLoader}
                        duplicates={dups}
                        needs={rowNeeds}
                        neededBy={rows.filter((other) => other !== row && row.meta && (other.meta?.depends ?? []).some((dep) => (suppliers.get(dep) ?? []).includes(row)))}
                        onFind={(dep) => browse("mod", dep)}
                        onOpenUrl={(url) => void s.openUrl(url)}
                      />
                    )}
                  </div>
                );
              }}
            />
          </div>
        )}
      </div>
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
function ModDetail({ row, loader, badLoader, duplicates, needs, neededBy, onFind, onOpenUrl }: {
  row: ModRow;
  loader: string;
  badLoader: boolean;
  duplicates: ModRow[];
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
      {badLoader && (
        <div className="mod-deps">
          <span className="mod-deps-label">{t("加载器", "Loader")}</span>
          <span className="mod-dep missing">
            {t(
              `模组适用于 ${(meta?.loaders ?? []).map(loaderLabel).join(" / ")}，与当前 ${loaderLabel(loader)} 不符`,
              `Built for ${(meta?.loaders ?? []).map(loaderLabel).join(" / ")}, incompatible with ${loaderLabel(loader)}`,
            )}
          </span>
        </div>
      )}
      {duplicates.length > 0 && (
        <div className="mod-deps">
          <span className="mod-deps-label">{t("重复", "Duplicate")}</span>
          {duplicates.map((other) => (
            <span key={other.mod.name} className="mod-dep missing">
              {other.mod.name}
            </span>
          ))}
        </div>
      )}
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
