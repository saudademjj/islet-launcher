import { motion } from "motion/react";
import type { ReactNode } from "react";

import type { Instance } from "../api/types";
import { formatPlayTime, formatRelative, loaderLabel } from "../lib/format";
import { useWorldStats } from "../lib/worldStats";
import { useStore, type InstanceState } from "../store";
import { useContextMenu } from "./ContextMenu";
import { biomeStyle, useBiome, useCover } from "../lib/biome";
import { SceneCover } from "./SceneCover";
import { Landscape, PX } from "./Pixel";
import { Identicon, InstanceStatus } from "./ui";
import { t } from "../lib/i18n";

/** An instance as a small world: its landscape on top, facts below. */
export function WorldCard({
  instance,
  state,
  active,
  onSelect,
  actions,
}: {
  instance: Instance;
  state: InstanceState;
  active?: boolean;
  onSelect?: () => void;
  actions?: ReactNode;
}) {
  const biome = useBiome(instance.id);
  const cover = useCover(instance.id);
  const stats = useWorldStats(instance);
  const busy = state === "running" || state === "starting" || state === "installing";
  // Everything a world can do, one right-click away.
  const onContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    const store = useStore.getState();
    useContextMenu.getState().open(e.clientX, e.clientY, [
      {
        label: state === "running" ? t("停止游戏", "Stop game") : state === "queued" ? t("取消排队", "Leave queue") : state === "notInstalled" ? t("安装并开始", "Install and play") : t("开始游戏", "Play"),
        icon: state === "running" ? "stop" : state === "queued" ? "close" : "play",
        disabled: state === "starting" || state === "installing",
        run: () => {
          store.setActiveInstance(instance.id);
          store.primaryAction(instance.id);
        },
      },
      { label: t("设为当前世界", "Make current"), icon: "home", disabled: active, run: () => store.setActiveInstance(instance.id) },
      "-",
      { label: t("管理", "Manage"), icon: "settings", run: () => store.openManage(instance.id) },
      { label: t("存档", "Saves"), icon: "box", run: () => store.openManage(instance.id, "data") },
      { label: t("打开游戏目录", "Open game folder"), icon: "folder", run: () => void store.open({ kind: "gameDir", id: instance.id }) },
      { label: t("查看日志", "Show log"), icon: "log", run: () => void store.openLogs(instance.id) },
      {
        label: t("创建桌面快捷方式", "Create desktop shortcut"),
        icon: "external",
        run: () =>
          void store.run<string>("create_shortcut", { id: instance.id }).then((path) => {
            if (path) store.toast(t("已在桌面创建快捷方式，双击直接进入这个世界", "Shortcut created on the Desktop; double-click it to jump into this world"), "success");
          }),
      },
      "-",
      { label: t("复制世界", "Duplicate world"), icon: "copy", disabled: busy || !instance.installed, run: () => void store.fileOperation(instance.id, { kind: "duplicate" }) },
      { label: t("移到回收站", "Move to recycle bin"), icon: "trash", danger: true, disabled: busy, run: () => store.openModal({ kind: "deleteInstance", instance }) },
    ]);
  };
  return (
    <motion.div
      layout="position"
      className={`world${active ? " active" : ""}${onSelect ? " clickable" : ""}`}
      onClick={onSelect}
      onContextMenu={onContextMenu}
      initial={{ opacity: 0, y: 6 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, scale: 0.97, transition: { duration: 0.14 } }}
      whileTap={onSelect ? { scale: 0.985 } : undefined}
    >
      <div className="world-scene" style={biomeStyle(biome)}>
        <Landscape seed={instance.id} unit={PX} cols={34} ground={[1, 2]} sky={5} treesFrom={6} far={[3, 5]} biome={biome} />
        <SceneCover id={instance.id} />
        <span className="world-biome">{cover ? t("截图", "Screenshots") : biome.name}</span>
        {active && <span className="world-badge">{t("当前", "Current")}</span>}
      </div>
      <div className="world-body">
        <span className="world-avatar">
          <Identicon seed={instance.id} size={38} hero />
        </span>
        <div className="row">
          <span className="world-name truncate">{instance.name}</span>
          <div className="spacer" />
          <InstanceStatus state={state} />
        </div>
        {stats && (
          <div className="world-stats">
            <span>{stats.saves}{t(" 个存档", " saves")}</span>
            {stats.mods != null && <span>{stats.mods}{t(" 个模组", " mods")}</span>}
          </div>
        )}
        <div className="world-meta">
          <span className="mono">{instance.versionId}</span>
          <span className="dot" />
          {loaderLabel(instance.loader)}
          <span className="dot" />
          <span className="truncate">
            {formatRelative(instance.lastLaunchedAt)}
            {instance.playSeconds >= 60 && t(` · 已玩 ${formatPlayTime(instance.playSeconds)}`, ` · played ${formatPlayTime(instance.playSeconds)}`)}
          </span>
        </div>
        {actions && (
          <div className="world-actions" onClick={(e) => e.stopPropagation()}>
            {actions}
          </div>
        )}
      </div>
    </motion.div>
  );
}
