import { AnimatePresence, motion } from "motion/react";
import { useState } from "react";

import type { Instance } from "../api/types";
import { Icon } from "../components/Icon";
import { BrandMark } from "../components/Pixel";
import { WorldCard } from "../components/WorldCard";
import { Button, Empty, Input, Kbd, Segmented } from "../components/ui";
import { item, stagger } from "../lib/motion";
import { formatPlayTime } from "../lib/format";
import { MOD_KEY } from "../lib/platform";
import { readChoice, saveChoice } from "../lib/prefs";
import { Toolbar } from "../layout/Shell";
import { instanceState, usePick, useStore } from "../store";
import { ImportPackButton } from "./HomePage";
import { worldGroups } from "../overlays/GroupDialog";
import { isEnglish, t } from "../lib/i18n";
import { useP2p } from "../lib/p2p";

const ORDER_KEY = "mc-launcher.worlds-order";
const ORDERS = ["recent", "time", "name"] as const;
const VIEW_KEY = "mc-launcher.worlds-view";
const VIEWS = ["grid", "list"] as const;

export function InstancesPage() {
  const s = usePick((s) => ({
    instances: s.instances,
    activeId: s.settings.activeInstanceId,
    fileBusy: s.fileBusy,
    openCreate: s.openCreate,
    openRecycle: s.openRecycle,
  }));
  // A shelf narrows the page to the starred worlds, one group, or the rest.
  type Shelf = { kind: "all" | "favorites" | "ungrouped" } | { kind: "group"; name: string };
  const [picked, setShelf] = useState<Shelf>({ kind: "all" });
  const groups = worldGroups(s.instances);
  const starred = s.instances.filter((i) => i.favorite).length;
  // A shelf that emptied out (last star removed, group renamed) falls back to all.
  const shelf: Shelf =
    (picked.kind === "group" && !groups.includes(picked.name)) || (picked.kind === "favorites" && starred === 0) || (picked.kind === "ungrouped" && groups.length === 0) ? { kind: "all" } : picked;
  const onShelf = (i: Instance) => {
    if (shelf.kind === "group") return i.group === shelf.name;
    return shelf.kind === "all" || (shelf.kind === "favorites" ? i.favorite : !i.group);
  };
  const [query, setQuery] = useState("");
  type Kind = "all" | "vanilla" | "modded";
  const [pickedKind, setKind] = useState<Kind>("all");
  // How the worlds are sorted and shown is remembered between visits.
  const [order, pickOrder] = useState(() => readChoice(ORDER_KEY, ORDERS, "recent"));
  const setOrder = (value: (typeof ORDERS)[number]) => {
    pickOrder(value);
    saveChoice(ORDER_KEY, value);
  };
  const [view, pickView] = useState(() => readChoice(VIEW_KEY, VIEWS, "grid"));
  const setView = (value: (typeof VIEWS)[number]) => {
    pickView(value);
    saveChoice(VIEW_KEY, value);
  };
  const q = query.trim().toLowerCase();
  const vanilla = s.instances.filter((i) => i.loader === "vanilla").length;
  const modded = s.instances.length - vanilla;
  // Vanilla / modded only splits anything when there are both.
  const splits = vanilla > 0 && modded > 0;
  const kind: Kind = splits ? pickedKind : "all";
  const toggleKind = (value: Kind) => setKind(kind === value ? "all" : value);
  const shown = s.instances.filter(
    (i) =>
      onShelf(i) &&
      (kind === "all" || (kind === "vanilla") === (i.loader === "vanilla")) &&
      (!q || i.name.toLowerCase().includes(q) || i.versionId.toLowerCase().includes(q)),
  );
  // Sorting is a view on the list; the backend keeps its own order.
  if (order === "time") shown.sort((a, b) => b.playSeconds - a.playSeconds);
  else if (order === "name") shown.sort((a, b) => a.name.localeCompare(b.name, isEnglish ? "en" : "zh-CN"));
  else shown.sort((a, b) => (b.lastLaunchedAt ?? b.createdAt) - (a.lastLaunchedAt ?? a.createdAt));

  return (
    <>
      <Toolbar title={t("世界", "Worlds")} meta={t(`${s.instances.length} 个世界 · 点击卡片设为当前世界`, `${s.instances.length} worlds · click a card to make it current`)}>
        <Button variant="ghost" icon="trash" onClick={() => void s.openRecycle()}>
          {t("回收站", "Recycle bin")}
        </Button>
        <Button icon="download" onClick={() => useStore.getState().openModal({ kind: "modpacks" })}>{t("浏览整合包", "Browse modpacks")}</Button>
        <ImportPackButton disabled={s.fileBusy} label={t("导入", "Import")} />
        <Button variant="primary" icon="plus" onClick={s.openCreate}>
          {t("新建 ", "New ")}<Kbd>{MOD_KEY}N</Kbd>
        </Button>
      </Toolbar>
      <div className="content">
        <motion.div className="page" variants={stagger} initial="initial" animate="animate">
          {s.instances.length === 0 ? (
            <Empty
              art={<BrandMark size={56} />}
              title={t("还没有世界", "No worlds yet")}
              actions={
                <>
                  <ImportPackButton />
                  <Button variant="primary" icon="plus" onClick={s.openCreate}>
                    {t("新建世界", "New world")}
                  </Button>
                </>
              }
            >
              {t("世界是一套独立的游戏配置，拥有自己的版本、存档、模组和内存设置。", "A world is its own game setup, with its own version, saves, mods and memory.")}
            </Empty>
          ) : (
            <>
              {(s.instances.length > 1 || starred > 0 || groups.length > 0) && (
                // One row: which worlds (shelf, then kind) on the left, how
                // they're shown on the right. Only one chip says "all".
                <motion.div className="world-filters" variants={item}>
                  <div className="filter-chips" role="group" aria-label={t("分组", "Groups")}>
                    <button
                      type="button"
                      className="filter-chip"
                      aria-pressed={shelf.kind === "all" && kind === "all"}
                      onClick={() => {
                        setShelf({ kind: "all" });
                        setKind("all");
                      }}
                    >
                      {t("全部", "All")} <span className="faint">{s.instances.length}</span>
                    </button>
                    {starred > 0 && (
                      <button type="button" className="filter-chip" aria-pressed={shelf.kind === "favorites"} onClick={() => setShelf({ kind: "favorites" })}>
                        <Icon name="star" size={11} /> {t("收藏", "Starred")} <span className="faint">{starred}</span>
                      </button>
                    )}
                    {groups.map((name) => (
                      <button key={name} type="button" className="filter-chip" aria-pressed={shelf.kind === "group" && shelf.name === name} onClick={() => setShelf({ kind: "group", name })}>
                        {name} <span className="faint">{s.instances.filter((i) => i.group === name).length}</span>
                      </button>
                    ))}
                    {groups.length > 0 && s.instances.some((i) => !i.group) && (
                      <button type="button" className="filter-chip" aria-pressed={shelf.kind === "ungrouped"} onClick={() => setShelf({ kind: "ungrouped" })}>
                        {t("未分组", "Ungrouped")}
                      </button>
                    )}
                  </div>
                  {splits && (
                    <div className="filter-chips world-kinds" role="group" aria-label={t("类型", "Kind")}>
                      <button type="button" className="filter-chip" aria-pressed={kind === "vanilla"} onClick={() => toggleKind("vanilla")}>
                        {t("原版", "Vanilla")} <span className="faint">{vanilla}</span>
                      </button>
                      <button type="button" className="filter-chip" aria-pressed={kind === "modded"} onClick={() => toggleKind("modded")}>
                        {t("模组", "Modded")} <span className="faint">{modded}</span>
                      </button>
                    </div>
                  )}
                  {s.instances.length > 1 && (
                    <div className="world-view">
                      <Segmented
                        value={order}
                        onChange={setOrder}
                        options={[
                          { value: "recent", label: t("最近游玩", "Recent") },
                          { value: "time", label: t("游玩时长", "Play time") },
                          { value: "name", label: t("名称", "Name") },
                        ]}
                      />
                      {s.instances.length > 3 && (
                        <div style={{ width: 220 }}>
                          <Input icon="search" placeholder={t("搜索名称或版本", "Search name or version")} value={query} onChange={(e) => setQuery(e.target.value)} />
                        </div>
                      )}
                      <div className="view-toggle" role="group" aria-label={t("显示方式", "View")}>
                        {VIEWS.map((v) => (
                          <button
                            key={v}
                            type="button"
                            aria-pressed={view === v}
                            aria-label={v === "grid" ? t("卡片", "Cards") : t("列表", "List")}
                            title={v === "grid" ? t("卡片", "Cards") : t("列表", "List")}
                            onClick={() => setView(v)}
                          >
                            <Icon name={v === "grid" ? "grid" : "rows"} size={14} />
                          </button>
                        ))}
                      </div>
                    </div>
                  )}
                </motion.div>
              )}
              <motion.div className={`worlds${view === "list" ? " as-list" : ""}`} variants={item}>
                <AnimatePresence initial={false}>
                  {shown.map((instance) => (
                    <InstanceCard key={instance.id} instance={instance} active={instance.id === s.activeId} />
                  ))}
                </AnimatePresence>
                {!q && kind === "all" && shelf.kind === "all" && (
                  <motion.button layout="position" type="button" className="world-new" onClick={s.openCreate}>
                    <span className="world-new-icon">
                      <Icon name="plus" size={20} />
                    </span>
                    <span className="world-new-title">{t("新建世界", "New world")}</span>
                    <span className="faint">{t("选择版本和加载器", "Choose version and loader")}</span>
                  </motion.button>
                )}
              </motion.div>
              {shown.length === 0 && <div className="faint" style={{ padding: "24px 4px" }}>{t("没有符合条件的世界", "No worlds match")}</div>}
              <PlayStats instances={s.instances} />
            </>
          )}
        </motion.div>
      </div>
    </>
  );
}

/** Where the time went: total play time and each world's share. */
function PlayStats({ instances }: { instances: Instance[] }) {
  const played = instances.filter((i) => i.playSeconds >= 60).sort((a, b) => b.playSeconds - a.playSeconds);
  if (played.length === 0) return null;
  const total = played.reduce((sum, i) => sum + i.playSeconds, 0);
  const top = played[0].playSeconds;
  return (
    <motion.section variants={item} className="play-stats">
      <div className="section-label">
        {t("游玩统计", "Play time")}
        <span className="mono faint">
          {t("共 ", "")}{formatPlayTime(total)} · {played.length}{t(" 个世界", " worlds")}
        </span>
      </div>
      <div className="col" style={{ gap: 8 }}>
        {played.slice(0, 6).map((instance) => (
          <div key={instance.id} className="play-row">
            <span className="truncate play-name">{instance.name}</span>
            <span className="play-bar" aria-hidden>
              <span style={{ width: `${Math.max(2, (instance.playSeconds / top) * 100)}%` }} />
            </span>
            <span className="mono faint play-time">{formatPlayTime(instance.playSeconds)}</span>
          </div>
        ))}
      </div>
    </motion.section>
  );
}

function InstanceCard({ instance, active }: { instance: Instance; active: boolean }) {
  const s = usePick((s) => ({
    organize: s.organize,
    install: s.install,
    installQueue: s.installQueue,
    running: s.running,
    runningIds: s.runningIds,
    launchPending: s.launchPending,
    setActiveInstance: s.setActiveInstance,
    primaryAction: s.primaryAction,
    openLogs: s.openLogs,
    openManage: s.openManage,
    stopping: s.stopping.includes(instance.id),
  }));
  const state = instanceState(instance, s);
  const primary =
    state === "running"
      ? s.stopping
        ? { icon: "stop" as const, label: t("关闭中 · 强制", "Closing · force"), variant: "default" as const }
        : { icon: "stop" as const, label: t("停止", "Stop"), variant: "danger" as const }
      : state === "ready"
        ? { icon: "play" as const, label: t("开始", "Play"), variant: active ? ("primary" as const) : ("default" as const) }
        : state === "notInstalled"
          ? { icon: "download" as const, label: t("安装", "Install"), variant: "default" as const }
          : state === "queued"
            ? { icon: "close" as const, label: t("取消排队", "Leave queue"), variant: "default" as const }
            : null;

  return (
    <WorldCard
      instance={instance}
      state={state}
      active={active}
      onSelect={active ? undefined : () => s.setActiveInstance(instance.id)}
      actions={
        <>
          {primary ? (
            <Button size="sm" variant={primary.variant} icon={primary.icon} onClick={() => s.primaryAction(instance.id)}>
              {primary.label}
            </Button>
          ) : (
            <Button size="sm" busy>
              {state === "starting" ? t("启动中", "Starting") : t("安装中", "Installing")}
            </Button>
          )}
          <Button
            size="sm"
            variant="ghost"
            title={t("开一个 P2P 房间，让好友加入这个世界", "Open a P2P room for friends to join this world")}
            onClick={() => {
              if (!active) s.setActiveInstance(instance.id);
              useP2p.getState().focus("host");
            }}
          >
            {t("联机", "Multiplayer")}
          </Button>
          <div className="spacer" />
          <Button
            size="sm"
            variant="ghost"
            icon="star"
            className="world-star"
            aria-pressed={instance.favorite}
            aria-label={instance.favorite ? t("取消收藏", "Unstar") : t("收藏", "Star")}
            title={instance.favorite ? t("取消收藏", "Unstar") : t("收藏", "Star")}
            onClick={() => void s.organize(instance.id, !instance.favorite, instance.group)}
          />
          <Button
            size="sm"
            variant="ghost"
            icon="tag"
            aria-label={t("分组", "Group")}
            title={instance.group ? t(`分组：${instance.group}`, `Group: ${instance.group}`) : t("分组", "Group")}
            onClick={() => useStore.getState().openModal({ kind: "group", id: instance.id })}
          />
          <Button size="sm" variant="ghost" icon="log" aria-label={t("日志", "Log")} title={t("日志", "Log")} onClick={() => void s.openLogs(instance.id)} />
          <Button size="sm" variant="ghost" icon="settings" aria-label={t("管理", "Manage")} title={t("管理", "Manage")} onClick={() => s.openManage(instance.id)} />
        </>
      }
    />
  );
}
