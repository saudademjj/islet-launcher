import { AnimatePresence, motion } from "motion/react";
import { useEffect, useRef, useState } from "react";

import { call, toError } from "../api/backend";
import type { ModProject, ModVersion, SearchResults } from "../api/types";
import { Icon } from "../components/Icon";
import { ModIcon } from "../components/ModIcon";
import { Modal } from "../components/overlay";
import { ProjectDetail } from "../components/ProjectDetail";
import { Button, Input, Notice, Segmented } from "../components/ui";
import { MODPACK_GROUPS, SORTS, modpackChoice, type FilterChoice, type SearchSort } from "../lib/contentFilters";
import { formatBytes, formatCount } from "../lib/format";
import { t } from "../lib/i18n";
import { normal } from "../lib/motion";
import { usePick, useStore } from "../store";

/** Both sources return this many hits per page. */
const PAGE_SIZE = 20;
const SKELETON_ROWS = 6;
const VERSION_TYPE: Record<string, string> = { beta: t("测试版", "Beta"), alpha: t("预览版", "Alpha") };

/** Modpacks from Modrinth or CurseForge, each installed as a new world.
 *  Laid out like a world's own download tab: results on the left, the
 *  chosen pack's versions on the right. */
/** Size of the pack file itself (.mrpack or .zip), not of the mods it lists. */
const packSize = (version: ModVersion) => (version.files.find((file) => file.primary) ?? version.files[0])?.size ?? 0;

export function ModpackBrowser() {
  const s = usePick((s) => ({ close: s.closeModal, busy: s.fileBusy, task: s.operationTask, install: s.fileOperation, key: s.settings.curseforgeKey }));
  const [source, setSource] = useState<"modrinth" | "curseforge">("modrinth");
  const [query, setQuery] = useState("");
  const [game, setGame] = useState("");
  const [sort, setSort] = useState<SearchSort>("downloads");
  const [choice, setChoice] = useState<FilterChoice>({});
  const [offset, setOffset] = useState(0);
  const [result, setResult] = useState<SearchResults | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [chosen, setChosen] = useState<ModProject | null>(null);
  const [detail, setDetail] = useState(false);
  const [versions, setVersions] = useState<ModVersion[]>([]);
  const [versionLoading, setVersionLoading] = useState(false);
  const request = useRef(0);
  // Bumped by "Try again" to repeat the same request.
  const [attempt, setAttempt] = useState(0);
  const needsKey = source === "curseforge" && !s.key;
  const loader = choice.loader || "";

  useEffect(() => {
    if (needsKey) {
      setResult(null);
      return;
    }
    const id = ++request.current;
    setLoading(true);
    setError("");
    const timer = setTimeout(() => {
      const { loader = null, category = null } = modpackChoice(source, choice);
      call<SearchResults>("search_modpacks", { query, source, offset, sort, version: game.trim() || null, loader, category })
        .then(
          (r) => id === request.current && setResult(r),
          (e) => id === request.current && setError(toError(e).message),
        )
        .finally(() => id === request.current && setLoading(false));
    }, 300);
    return () => {
      clearTimeout(timer);
      ++request.current;
    };
  }, [query, source, offset, sort, game, choice, needsKey, attempt]);

  useEffect(() => {
    if (!chosen) return;
    let live = true;
    setVersions([]);
    setVersionLoading(true);
    call<ModVersion[]>("modpack_versions", { project: chosen.project_id })
      .then(
        (v) => live && setVersions(v.filter((version) => (!game.trim() || version.game_versions.includes(game.trim())) && (!loader || version.loaders.length === 0 || version.loaders.includes(loader)))),
        (e) => live && setError(toError(e).message),
      )
      .finally(() => live && setVersionLoading(false));
    return () => {
      live = false;
    };
  }, [chosen, game, loader, attempt]);

  const restart = () => {
    setOffset(0);
    setChosen(null);
    setDetail(false);
  };
  const install = (version: string) => {
    if (version && !s.busy) void s.install(null, { kind: "installModpack", version });
  };
  const hits = result?.hits ?? [];
  const page = Math.floor(offset / PAGE_SIZE) + 1;
  const pages = Math.max(1, Math.ceil((result?.total_hits ?? 0) / PAGE_SIZE));

  return (
    <Modal wide title={t("浏览整合包", "Browse modpacks")} onClose={s.close}>
      <div className="col gap-12">
        <div className="row">
          <div style={{ flex: 1 }}>
            <Input icon="search" placeholder={t("搜索整合包，留空看热门", "Search modpacks, or leave empty for popular ones")} value={query} onChange={(e) => { setQuery(e.target.value); restart(); }} />
          </div>
          <div style={{ width: 168 }}>
            <Input placeholder={t("游戏版本，如 1.20.1", "Version, e.g. 1.20.1")} value={game} onChange={(e) => { setGame(e.target.value); restart(); }} />
          </div>
        </div>
        <div className="browse-filters">
          <div className="filter-row">
            <span className="filter-label">{t("来源", "Source")}</span>
            <Segmented value={source} onChange={(v) => { setSource(v); setChoice(modpackChoice(v, choice)); restart(); }} options={[{ value: "modrinth" as const, label: "Modrinth" }, { value: "curseforge" as const, label: "CurseForge" }]} />
          </div>
          {MODPACK_GROUPS.map((group) => (
            <div key={group.id} className="filter-row" role="group" aria-label={group.label}>
              <span className="filter-label">{group.label}</span>
              <div className="filter-chips">
                <button type="button" className="filter-chip" aria-pressed={!choice[group.id]} onClick={() => { setChoice({ ...choice, [group.id]: "" }); restart(); }}>
                  {t("全部", "All")}
                </button>
                {group.options
                  .filter((o) => !o.only || o.only === source)
                  .map((o) => (
                    <button key={o.value} type="button" className="filter-chip" aria-pressed={choice[group.id] === o.value} onClick={() => { setChoice({ ...choice, [group.id]: o.value }); restart(); }}>
                      {o.label}
                    </button>
                  ))}
              </div>
            </div>
          ))}
          <div className="filter-row">
            <span className="filter-label">{t("排序", "Sort")}</span>
            <Segmented value={sort} onChange={(v) => { setSort(v); restart(); }} options={SORTS} />
            {result && !loading && <span className="filter-total faint">{t("共 ", "")}{formatCount(result.total_hits)}{t(" 个结果", " results")}</span>}
          </div>
        </div>
        {needsKey && (
          <Notice title={t("CurseForge 需要 API Key", "CurseForge needs an API key")}>
            {t("在「设置 → 下载」里填写 CurseForge API Key 后即可搜索；Modrinth 不需要。", "Add a CurseForge API key under Settings → Downloads to search it; Modrinth needs none.")}
          </Notice>
        )}
        {error && (
          <Notice
            tone="danger"
            title={t(`没能连上 ${source === "curseforge" ? "CurseForge" : "Modrinth"}`, `Couldn't reach ${source === "curseforge" ? "CurseForge" : "Modrinth"}`)}
            actions={<Button size="sm" icon="refresh" onClick={() => setAttempt((n) => n + 1)}>{t("重试", "Try again")}</Button>}
          >
            {error}
          </Notice>
        )}
        {s.busy && s.task && (
          <div className="row muted" style={{ fontSize: "var(--text-sm)" }}>
            <span className="spinner" />{t(" 正在下载整合包并创建世界…", " Downloading the modpack and creating the world…")}
            <div className="spacer" />
            <Button size="sm" variant="danger-ghost" onClick={() => void useStore.getState().run("cancel_task", { taskId: s.task })}>
              {t("取消", "Cancel")}
            </Button>
          </div>
        )}
        {detail && chosen ? (
          <ProjectDetail project={chosen} kind="mod" glyph="pack" busy={s.busy} onInstall={install} onBack={() => setDetail(false)} pick={{ loading: versionLoading, newest: versions[0] ?? null }} />
        ) : needsKey ? null : (
          <div className="browse">
            {/* A new search keeps the old page dimmed until the answer arrives. */}
            <div className={`list browse-results${loading && hits.length > 0 ? " stale" : ""}`} aria-busy={loading}>
              {loading &&
                hits.length === 0 &&
                Array.from({ length: SKELETON_ROWS }, (_, i) => (
                  <div key={i} className="list-row browse-skeleton" aria-hidden>
                    <span className="skeleton skeleton-icon" />
                    <span className="col fill gap-8">
                      <span className="skeleton" style={{ width: `${34 + ((i * 17) % 30)}%` }} />
                      <span className="skeleton" style={{ width: `${62 + ((i * 11) % 26)}%` }} />
                    </span>
                  </div>
                ))}
              {hits.map((hit) => (
                <button key={hit.project_id} type="button" className="list-row clickable browse-hit" aria-selected={hit.project_id === chosen?.project_id} onClick={() => setChosen(hit)}>
                  <ModIcon src={hit.icon_url} seed={hit.project_id} size={40} glyph="pack" />
                  <div className="col fill gap-2" style={{ textAlign: "left" }}>
                    <span className="browse-title truncate">{hit.title}</span>
                    <span className="muted truncate" style={{ fontSize: "var(--text-xs)" }}>
                      {hit.description}
                    </span>
                  </div>
                  <span className="mono faint" style={{ fontSize: "var(--text-xs)", whiteSpace: "nowrap" }}>
                    <Icon name="download" /> {formatCount(hit.downloads)}
                  </span>
                  <Icon name="chevronRight" className="faint" />
                </button>
              ))}
              {result && hits.length === 0 && !loading && (
                <div className="list-row faint" style={{ fontSize: "var(--text-sm)" }}>
                  {game.trim()
                    ? t(`没有找到适配 ${game.trim()} 的整合包；换个关键词或清空版本试试。`, `No modpacks found for ${game.trim()}; try other words or clear the version.`)
                    : t("没有匹配的整合包；换个关键词试试。", "No matching modpacks; try other words.")}
                </div>
              )}
              {result && !(loading && hits.length === 0) && (
                <div className="list-row pager">
                  <Button size="sm" variant="ghost" icon="chevronLeft" disabled={page <= 1 || loading} onClick={() => setOffset(offset - PAGE_SIZE)}>
                    {t("上一页", "Previous")}
                  </Button>
                  <span className="mono faint" style={{ fontSize: "var(--text-xs)" }}>
                    {t(`${page} / ${pages} 页`, `Page ${page} of ${pages}`)}
                  </span>
                  <Button size="sm" variant="ghost" disabled={page >= pages || loading} onClick={() => setOffset(offset + PAGE_SIZE)}>
                    {t("下一页 ", "Next ")}<Icon name="chevronRight" />
                  </Button>
                </div>
              )}
            </div>
            <AnimatePresence mode="wait">
              {!chosen ? (
                <motion.div key="hint" className="browse-versions browse-placeholder" initial={{ opacity: 0 }} animate={{ opacity: 1, transition: normal }} exit={{ opacity: 0 }}>
                  <Icon name="pack" size={32} />
                  <span>{t("选择左侧任意整合包，这里会列出它的版本。安装会创建一个新世界。", "Pick any modpack on the left to see its versions. Installing creates a new world.")}</span>
                </motion.div>
              ) : (
                <motion.div key={chosen.project_id} className="browse-versions card" initial={{ opacity: 0, x: 10 }} animate={{ opacity: 1, x: 0, transition: normal }} exit={{ opacity: 0 }}>
                  <div className="card-header browse-versions-head">
                    <ModIcon src={chosen.icon_url} seed={chosen.project_id} size={24} glyph="pack" />
                    <span className="card-title truncate" style={{ flex: 1 }}>{chosen.title}</span>
                    <Button size="sm" variant="ghost" icon="info" onClick={() => setDetail(true)}>
                      {t("介绍", "About")}
                    </Button>
                  </div>
                  <div className="col gap-2" style={{ padding: 8 }}>
                    {versionLoading ? (
                      <div className="faint row" style={{ padding: 10 }}>
                        <span className="spinner" />{t(" 正在获取版本…", " Loading versions…")}
                      </div>
                    ) : versions.length === 0 ? (
                      <div className="faint" style={{ padding: 10 }}>
                        {game.trim() ? t(`没有适配 ${game.trim()} 的版本。`, `No version for ${game.trim()}.`) : t("没有可安装的版本。", "No version to install.")}
                      </div>
                    ) : (
                      versions.map((v) => (
                        <div key={v.id} className="picker-row flat">
                          <div className="col" style={{ flex: 1, minWidth: 0 }}>
                            <span className="row gap-6">
                              <span className="mono truncate" style={{ fontWeight: 600 }}>
                                {v.version_number}
                              </span>
                              {v.version_type && VERSION_TYPE[v.version_type] && <span className="version-flag">{VERSION_TYPE[v.version_type]}</span>}
                            </span>
                            <span className="faint truncate" style={{ fontSize: "var(--text-xs)" }}>
                              {v.loaders.join(" / ")} · {v.game_versions.slice(0, 3).join(", ")}
                              {packSize(v) > 0 && ` · ${t("整合包文件", "pack file")} ${formatBytes(packSize(v))}`}
                            </span>
                          </div>
                          <Button size="sm" icon="download" disabled={s.busy} onClick={() => install(v.id)}>
                            {t("安装", "Install")}
                          </Button>
                        </div>
                      ))
                    )}
                    <span className="faint" style={{ fontSize: "var(--text-xs)", padding: "6px 10px" }}>
                      {t("下载后导入为一个新世界，原有世界不受影响。整合包文件只含清单和配置，模组在安装时另外下载，进度里会显示总大小。", "Downloads and imports as a new world; your other worlds stay as they are. The pack file holds the list and configs; mods download during the install, which shows the total size.")}
                    </span>
                  </div>
                </motion.div>
              )}
            </AnimatePresence>
          </div>
        )}
      </div>
    </Modal>
  );
}
