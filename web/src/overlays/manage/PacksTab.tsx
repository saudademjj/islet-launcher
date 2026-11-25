import { motion } from "motion/react";
import { useState } from "react";

import { call, toError } from "../../api/backend";
import type { ContentKind, Instance, PackFile } from "../../api/types";
import { Icon } from "../../components/Icon";
import { ModIcon } from "../../components/ModIcon";
import { Button, Input, Segmented } from "../../components/ui";
import { usePackInsights } from "../../lib/contentIcons";
import { formatBytes } from "../../lib/format";
import { t } from "../../lib/i18n";
import { normal } from "../../lib/motion";
import { matchItem, queryWords } from "../../lib/search";
import { usePick, useStore } from "../../store";
import { OperationLine, shaderSupport } from "./shared";

type PackFilter = "all" | "active" | "inactive";

export function PacksTab({ instance, browse }: { instance: Instance; browse: (kind: ContentKind, query?: string) => void }) {
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
  const [reordering, setReordering] = useState(false);
  const setOrder = async (next: string[]) => {
    if (reordering) return;
    setReordering(true);
    try {
      await call("set_resource_packs", { id: instance.id, packs: next });
      await useStore.getState().reloadPacks();
    } catch (error) {
      useStore.getState().toast(toError(error).message, "error");
    } finally {
      setReordering(false);
    }
  };
  const move = (name: string, by: number) => {
    const at = order.indexOf(name);
    if (at < 0) return;
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
      hint: t("越靠上优先级越高，下次启动生效", "Higher packs take priority; applies on next launch"),
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
                <Icon name={section.glyph} />
              </span>
              <div className="col gap-2 fill">
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
                  title={instance.loader === "vanilla" ? t("为原版直接安装 OptiFine", "Install OptiFine for vanilla") : instance.loader === "forge" ? t("安装 OptiFine 加载光影", "Install OptiFine for shaders") : t("安装 OptiFine 与 OptiFabric（与 Sodium / Iris 不兼容）", "Install OptiFine + OptiFabric (incompatible with Sodium/Iris)")}
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
              <div className="saves-empty muted">{t("还没有", "No ")}{section.title}{t("，可在线下载或手动添加。", " yet. Download online or add local files.")}</div>
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
                      <div className="col gap-2 fill">
                        <span className="pack-name truncate">{info?.title ?? pack.name.replace(/\.zip$/i, "")}</span>
                        <span className="pack-meta mono truncate">
                          {info ? `${pack.name} · ` : pack.folder ? t("文件夹 · ", "Folder · ") : "ZIP · "}
                          {formatBytes(pack.bytes)}
                        </span>
                      </div>
                      {pack.active != null && (
                        <span className="pack-active" title={section.kind === "shader" ? t("游戏内正在使用的光影", "Active shader pack") : t("已启用，序号越小优先级越高", "Enabled; lower numbers take priority")}>
                          {section.kind === "shader" ? t("使用中", "In use") : t(`启用 #${pack.active}`, `On #${pack.active}`)}
                        </span>
                      )}
                      {section.kind === "resourcepack" && (
                        <span className="pack-order">
                          {pack.active != null ? (
                            <>
                              <Button size="sm" variant="ghost" icon="chevronUp" aria-label={t("上移", "Move up")} title={t("上移：覆盖更多", "Move up: overrides more")} disabled={s.busy || reordering || pack.active === 1} onClick={() => move(pack.name, -1)} />
                              <Button size="sm" variant="ghost" icon="chevronDown" aria-label={t("下移", "Move down")} title={t("下移", "Move down")} disabled={s.busy || reordering || pack.active === order.length} onClick={() => move(pack.name, 1)} />
                              <Button size="sm" variant="ghost" disabled={s.busy || reordering} onClick={() => void setOrder(order.filter((n) => n !== pack.name))}>
                                {t("禁用", "Disable")}
                              </Button>
                            </>
                          ) : (
                            <Button size="sm" variant="go" disabled={s.busy || reordering} onClick={() => void setOrder([pack.name, ...order])}>
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
                        aria-label={t(`把 ${pack.name} 移入回收站`, `Move ${pack.name} to the recycle bin`)}
                        title={t("移入回收站", "Move to recycle bin")}
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
