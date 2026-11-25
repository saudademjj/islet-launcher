import { AnimatePresence, motion } from "motion/react";
import { useEffect, useState } from "react";

import type { ContentKind, Instance, ModProject } from "../../api/types";
import { Icon, type IconName } from "../../components/Icon";
import { ModIcon } from "../../components/ModIcon";
import { ProjectDetail } from "../../components/ProjectDetail";
import { Button, Empty, Input, Notice, Segmented } from "../../components/ui";
import { FILTER_GROUPS, SORTS, type FilterChoice, type SearchSort } from "../../lib/contentFilters";
import { usePackInsights } from "../../lib/contentIcons";
import { formatCount, loaderLabel } from "../../lib/format";
import { isEnglish, t } from "../../lib/i18n";
import { findChineseMods, findModName, hasChinese, slugOfFile, zhDescription, type ModName } from "../../lib/modNames";
import { normal } from "../../lib/motion";
import { useWorlds } from "../../lib/worlds";
import { usePick, useStore, type SearchSource } from "../../store";
import { isPluginServer, NeedsLoader, OperationLine, packKey, shaderSupport } from "./shared";

const PAGE_SIZE = 10;
/** Tags a long filter group shows before "更多". */
const FOLDED = 10;
/** Placeholder rows while the first page of results loads. */
const SKELETON_ROWS = 6;

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
const KIND_GLYPH = { mod: "mod", resourcepack: "pack", shader: "sun", datapack: "map", world: "block" } as const satisfies Record<ContentKind, IconName>;

/** The shader loaders a shader pack supports, as readable tags. */
function shaderTags(categories: string[] | undefined): string[] {
  const names: Record<string, string> = { iris: "Iris", optifine: "OptiFine", canvas: "Canvas", vanilla: t("原版核心光影", "Vanilla core shaders") };
  return (categories ?? []).flatMap((c) => (names[c] ? [names[c]] : []));
}

const VERSION_TYPE: Record<string, string> = { beta: t("测试版", "Beta"), alpha: t("预览版", "Alpha") };

export function BrowseTab({ instance, busy }: { instance: Instance; busy: boolean }) {
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
    curseforgeConfigured: Boolean(s.settings.curseforgeKey.trim() || s.appInfo.curseforgeConfigured),
    openSettings: s.openSettings,
    closeModal: s.closeModal,
  }));
  const [query, setQuery] = useState(s.modQuery);
  const source = s.search.source;
  const curseforge = source === "curseforge";
  const needsKey = curseforge && !s.curseforgeConfigured;
  const packInsights = usePackInsights(instance.id, s.packs);
  const [chinese, setChinese] = useState<ChineseResolution | null>(null);
  const [noChineseMatch, setNoChineseMatch] = useState<string | null>(null);
  const [chosen, setChosen] = useState<ModName | null>(null);
  const [unfolded, setUnfolded] = useState<Record<string, boolean>>({});
  const kind = s.search.kind;
  const kindName = isPluginServer(instance.loader) && kind === "mod" ? t("插件", "Plugins") : KIND_NAME[kind];
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
    if (next === "curseforge" && !s.curseforgeConfigured) {
      useStore.setState({ modSearch: { ...s.search, source: next, key: "", hits: [], total: 0 }, modVersions: { key: "", project: null, versions: [], loading: false } });
      return;
    }
    if (blocked) useStore.setState({ modSearch: { ...s.search, source: next }, modVersions: { key: "", project: null, versions: [], loading: false } });
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
      if (s.curseforgeConfigured) s.searchMods(instance.id, "", 0, next, { source: "curseforge" });
      else useStore.setState({ modSearch: { ...s.search, kind: next, source: "curseforge", key: "", hits: [], total: 0 }, modVersions: { key: "", project: null, versions: [], loading: false } });
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
      <div className="row wrap">
        {/* A server project only takes mods or plugins, and plugins only come from Modrinth. */}
        {!instance.server && <Segmented value={kind} onChange={switchKind} options={KIND_OPTIONS} />}
        {!isPluginServer(instance.loader) && (
          <Segmented
            value={source}
            onChange={switchSource}
            options={[
              { value: "modrinth", label: "Modrinth" },
              { value: "curseforge", label: "CurseForge" },
            ]}
          />
        )}
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
          {t("在 console.curseforge.com 免费申请 Key 后，填到「设置 → 下载」即可使用。", "Get a free key at console.curseforge.com and add it in Settings → Download.")}
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
                placeholder={isPluginServer(instance.loader) ? t("搜索插件，如 EssentialsX、LuckPerms", "Search plugins, like EssentialsX or LuckPerms") : KIND_PLACEHOLDER[kind]}
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
                    {worlds ? t("暂无存档，请先进入游戏创建一个存档。", "No saves yet; create one in game first.") : t("正在读取存档…", "Reading saves…")}
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
                        <Icon name={open ? "chevronUp" : "chevronDown"} />
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
              <span className="row gap-12 wrap">
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
            <div className="row wrap gap-6">
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
            <Notice tone="warn" title={t("未匹配到常用中文名", "No Chinese alias found")}>
              {t(`已按「${noChineseMatch}」直接搜索，也可改用英文名。`, `Searching directly for "${noChineseMatch}"; try the English name if needed.`)}
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
            <Empty icon="search" title={t(`从 ${curseforge ? "CurseForge" : "Modrinth"} 下载${kindName}`, `Download ${kindName} from ${curseforge ? "CurseForge" : "Modrinth"}`)}>
              {t("输入关键词搜索，或留空浏览热门内容。", "Type keywords or search empty to browse popular items.")}
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
                      <span className="col fill gap-8">
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
                      <ModIcon src={hit.icon_url} seed={hit.project_id} size={40} glyph={KIND_GLYPH[kind]} />
                      <div className="col fill gap-2" style={{ textAlign: "left" }}>
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
                          <Icon name="download" /> {formatCount(hit.downloads)}
                        </span>
                      )}
                      <Icon name="chevronRight" className="faint" />
                    </button>
                  );
                })}
                {s.search.hits.length === 0 && !s.search.loading && (
                  <div className="list-row faint" style={{ fontSize: "var(--text-sm)" }}>
                    {kind === "mod"
                      ? t(`没有找到兼容模组，可换用 ${curseforge ? "Modrinth" : "CurseForge"} 或手动添加 .jar。`, `No compatible mods found; try ${curseforge ? "Modrinth" : "CurseForge"} or add a .jar.`)
                      : t(`没有找到适配 ${instance.versionId} 的${KIND_NAME[kind]}。`, `No ${KIND_NAME[kind]} found for ${instance.versionId}.`)}
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
                      {t("下一页 ", "Next ")}<Icon name="chevronRight" />
                    </Button>
                  </div>
                )}
              </div>
              <AnimatePresence mode="wait">
                {!s.versions.project && (
                  <motion.div key="hint" className="browse-versions browse-placeholder" initial={{ opacity: 0 }} animate={{ opacity: 1, transition: normal }} exit={{ opacity: 0 }}>
                    <Icon name={kind === "mod" ? "block" : KIND_GLYPH[kind]} size={32} />
                    <span>{t("选择左侧项目以查看适配 ", "Select a result to view versions for ")}{instance.versionId}{t(" 的版本", "")}</span>
                  </motion.div>
                )}
                {s.versions.project && (
                  <motion.div key={s.versions.project} className="browse-versions card" initial={{ opacity: 0, x: 10 }} animate={{ opacity: 1, x: 0, transition: normal }} exit={{ opacity: 0 }}>
                    <div className="card-header browse-versions-head">
                      <ModIcon src={project?.icon_url} seed={s.versions.project} size={24} glyph={KIND_GLYPH[kind]} />
                      <span className="card-title truncate" style={{ flex: 1 }}>{project?.title ?? s.versions.project}</span>
                      {project && (
                        <Button size="sm" variant="ghost" icon="info" onClick={() => setDetail(project)}>
                          {t("介绍", "About")}
                        </Button>
                      )}
                    </div>
                    <div className="col gap-2" style={{ padding: 8 }}>
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
                              <span className="row gap-6">
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
                              ? t(`将安装到存档「${saves.find((w) => w.folder === target)?.name || target}」，下次进入生效`, `Installs to save "${saves.find((w) => w.folder === target)?.name || target}"; applies next load`)
                              : t("先选择要装进哪个存档", "Choose a save first")
                            : kind === "world"
                              ? t("安装后下次启动游戏即可游玩", "Ready to play on next launch")
                              : t(`将安装到${kind === "shader" ? "光影包" : "资源包"}列表`, `Installs to ${kind === "shader" ? "shader packs" : "resource packs"}`)}
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
