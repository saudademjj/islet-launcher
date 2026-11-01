import { useEffect, useMemo, useState } from "react";

import type { VersionSummary } from "../api/types";
import { LoaderPicker, useLoaderSelectionValid } from "../components/LoaderPicker";
import { Modal, VirtualList } from "../components/overlay";
import { SceneHeader } from "../components/SceneHeader";
import { BIOME_IDS, BIOMES, biomeFor, useBiomeChoices, type BiomeId } from "../lib/biome";
import { Button, Field, Identicon, Input, Segmented, Tabs } from "../components/ui";
import { formatReleaseDate, loaderLabel, updateName, versionTypeLabel } from "../lib/format";
import { usePick, useStore } from "../store";
import { t } from "../lib/i18n";

const newSeed = () => `new:${Math.random().toString(36).slice(2, 10)}`;

/** Versions most modpacks and mods target, with why a player might pick them. */
const POPULAR: [string, string][] = [
  ["1.21.1", t("新版模组最多", "Most new mods")],
  ["1.20.1", t("整合包最多", "Most modpacks")],
  ["1.16.5", t("老牌整合包", "Classic modpacks")],
  ["1.12.2", t("经典模组版本", "Classic mod version")],
];

type Filter = "release" | "snapshot" | "all";

export function CreateInstance() {
  const s = usePick((s) => ({
    versions: s.versions,
    latest: s.latest,
    loaded: s.versionsLoaded,
    loading: s.versionsLoading,
    fromCache: s.versionsFromCache,
    hasInstances: s.instances.length > 0,
    loadVersions: s.loadVersions,
    closeModal: s.closeModal,
    mutate: s.mutate,
    preset: s.modal?.kind === "create" ? s.modal.version : undefined,
  }));
  // The name follows the chosen version ("1.20.1 Fabric") until it is typed
  // over, so a new world never shares the game's default save name.
  const [typedName, setTypedName] = useState<string | null>(null);
  const [filter, setFilter] = useState<Filter>("release");
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<VersionSummary | null>(null);
  const [loader, setLoader] = useState("vanilla");
  const [loaderVersion, setLoaderVersion] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const loaderValid = useLoaderSelectionValid(loader, loaderVersion);
  const autoName = !s.hasInstances ? t("我的世界", "My World") : selected ? `${selected.id}${loader === "vanilla" ? "" : ` ${loaderLabel(loader)}`}` : "";
  const name = typedName ?? autoName;
  const setName = (value: string) => setTypedName(value);
  // The new world is previewed from a seed of its own; "换一个" rolls again.
  const [seed, setSeed] = useState(newSeed);
  const [biomeChoice, setBiomeChoice] = useState<BiomeId | "auto">("auto");
  const biome = biomeChoice === "auto" ? biomeFor(seed) : BIOMES[biomeChoice];

  // Default to the version asked for, else the latest release, once the list arrives.
  useEffect(() => {
    if (selected || !s.loaded) return;
    const asked = s.preset ? s.versions.find((v) => v.id === s.preset) : undefined;
    if (asked && asked.versionType !== "release") setFilter("all");
    const latest = asked ?? s.versions.find((v) => v.id === s.latest?.release) ?? s.versions.find((v) => v.versionType === "release") ?? s.versions[0];
    if (latest) setSelected(latest);
  }, [s.loaded, s.versions, s.latest, s.preset, selected]);

  const quick = useMemo(() => {
    const byId = new Map(s.versions.map((v) => [v.id, v]));
    const latest = s.latest?.release ? byId.get(s.latest.release) : undefined;
    const picks: { version: VersionSummary; note: string }[] = latest ? [{ version: latest, note: t("最新正式版", "Latest release") }] : [];
    for (const [id, note] of POPULAR) {
      const version = byId.get(id);
      if (version && version.id !== latest?.id) picks.push({ version, note });
    }
    return picks.slice(0, 4);
  }, [s.versions, s.latest]);

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return s.versions.filter(
      (v) => (filter === "all" || v.versionType === filter) && (!q || v.id.toLowerCase().includes(q) || (updateName(v.id) ?? "").includes(q)),
    );
  }, [s.versions, filter, query]);

  const create = async () => {
    if (!selected) return;
    setCreating(true);
    const ok = await s.mutate("create_instance", {
      name: name.trim(),
      versionId: selected.id,
      versionType: selected.versionType,
      loader,
      loaderVersion,
    });
    setCreating(false);
    if (ok) {
      // The new world keeps the biome it was previewed with.
      const id = useStore.getState().settings.activeInstanceId;
      if (id) useBiomeChoices.getState().choose(id, biome.id);
      useStore.setState({ modal: null, page: "home" });
      useStore.getState().toast(t(`已创建「${name.trim() || selected.id}」，首次开始游戏时安装`, `Created "${name.trim() || selected.id}"; it installs the first time you play`), "success");
    }
  };

  return (
    <Modal
      wide
      onClose={s.closeModal}
      bodyClass="create-body"
      scene={
        <SceneHeader
          className="create-head"
          seed={seed}
          biome={biome}
          eyebrow={t(`新建世界 · 封面：${biome.name}`, `New world · cover: ${biome.name}`)}
          title={name.trim() || t("未命名", "Untitled")}
          meta={
            selected && (
              <>
                <span className="mono">{selected.id}</span>
                <span className="dot" />
                {loaderLabel(loader)}
              </>
            )
          }
          aside={<Identicon key={seed} seed={seed} size={52} hero build />}
          onClose={s.closeModal}
          strip={
            <div className="create-strip">
              {/* Only the launcher's picture of the world: the game makes its
                  own terrain when a save is created in it. */}
              <span className="create-strip-label">{t("封面", "Cover")}</span>
              <Tabs
                value={biomeChoice}
                onChange={setBiomeChoice}
                tabs={[{ value: "auto" as const, label: t("随机", "Random") }, ...BIOME_IDS.map((id) => ({ value: id, label: BIOMES[id].name }))]}
              />
              <button type="button" className="create-reroll" onClick={() => setSeed(newSeed())} title={t("换一张封面", "Another cover")}>
                {t("换一个", "Reroll")}
              </button>
            </div>
          }
        />
      }
      footer={
        <>
          <span className="faint create-note">
            {s.fromCache && t("版本列表来自本地缓存 · ", "Cached version list · ")}
            {t("封面只是启动器里的背景，不决定游戏地形；存档在开始游戏后到游戏里新建", "The cover is only the launcher's backdrop, not the game's terrain; make a save in the game after pressing Play")}
          </span>
          <Button variant="ghost" onClick={s.closeModal}>
            {t("取消", "Cancel")}
          </Button>
          <Button variant="primary" icon="plus" disabled={!selected || !loaderValid} busy={creating} onClick={() => void create()}>
            {t("创建世界", "Create a world")}
          </Button>
        </>
      }
    >
      <div className="create-form col">
        <Field label={t("名称", "Name")}>
          <Input value={name} placeholder={autoName || t("例如：生存 1.21", "e.g. Survival 1.21")} onChange={(e) => setName(e.target.value)} autoFocus />
        </Field>
        <Field label={t("加载器", "Loader")}>
          <LoaderPicker loader={loader} version={loaderVersion} minecraft={selected?.id ?? null} onLoader={setLoader} onVersion={setLoaderVersion} />
        </Field>
        {quick.length > 0 && (
          <Field label={t("常用版本", "Popular versions")}>
            <div className="quick-versions">
              {quick.map(({ version, note }) => (
                <button
                  key={version.id}
                  type="button"
                  className="quick-version"
                  aria-pressed={selected?.id === version.id}
                  onClick={() => {
                    setFilter("release");
                    setQuery("");
                    setSelected(version);
                  }}
                >
                  <span className="mono quick-id">{version.id}</span>
                  <span className="quick-note">
                    {updateName(version.id) ? `${updateName(version.id)} · ${note}` : note}
                  </span>
                </button>
              ))}
            </div>
          </Field>
        )}
      </div>

      <div className="create-versions col">
        <div className="row">
          <Segmented
            value={filter}
            options={[
              { value: "release", label: t("正式版", "Release") },
              { value: "snapshot", label: t("快照", "Snapshot") },
              { value: "all", label: t("全部", "All") },
            ]}
            onChange={setFilter}
          />
          <div style={{ flex: 1 }}>
            <Input icon="search" placeholder={t("版本号或更新名，如 1.20 / 足迹", "Version or update name, e.g. 1.20 / Trails")} value={query} onChange={(e) => setQuery(e.target.value)} />
          </div>
          <Button variant="ghost" icon="refresh" busy={s.loading} aria-label={t("重新加载", "Reload")} title={t("重新加载", "Reload")} onClick={() => s.loadVersions(true)} />
        </div>
        <div className="version-list">
          {s.loading && !s.loaded ? (
            <div className="list-placeholder">
              <span className="spinner" />{t(" 正在获取版本列表…", " Loading versions…")}
            </div>
          ) : !s.loaded ? (
            <div className="list-placeholder">
              {t("版本列表加载失败，请检查网络后重试", "Couldn't load versions; check the network and try again")}
              <Button size="sm" onClick={() => s.loadVersions(true)}>
                {t("重新加载", "Reload")}
              </Button>
            </div>
          ) : shown.length === 0 ? (
            <div className="list-placeholder">{t("没有匹配的版本", "No matching versions")}</div>
          ) : (
            <VirtualList
              items={shown}
              rowHeight={38}
              style={{ height: "100%" }}
              scrollToIndex={selected ? shown.findIndex((v) => v.id === selected.id) : null}
              render={(v) => {
                const chosen = selected?.id === v.id;
                const latest = v.id === s.latest?.release || v.id === s.latest?.snapshot;
                return (
                  <button type="button" className="version-row" aria-selected={chosen} onClick={() => setSelected(v)}>
                    <span className="mono" style={{ fontWeight: 650, minWidth: 92 }}>
                      {v.id}
                    </span>
                    {updateName(v.id) && v.versionType === "release" ? (
                      <span className="version-update">{updateName(v.id)}</span>
                    ) : (
                      <span className="muted" style={{ fontSize: "var(--text-xs)" }}>
                        {versionTypeLabel(v.versionType)}
                      </span>
                    )}
                    {latest && <span className="badge-latest">{t("最新", "Latest")}</span>}
                    <span className="spacer" />
                    <span className="mono faint" style={{ fontSize: "var(--text-xs)" }}>
                      {formatReleaseDate(v.releaseTime)}
                    </span>
                  </button>
                );
              }}
            />
          )}
        </div>
      </div>
    </Modal>
  );
}
