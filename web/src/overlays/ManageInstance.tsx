import { AnimatePresence, motion } from "motion/react";
import { useEffect, useRef } from "react";

import type { ContentKind } from "../api/types";
import { Modal } from "../components/overlay";
import { Sign } from "../components/Pixel";
import { SceneHeader } from "../components/SceneHeader";
import { Button, Identicon, Tabs } from "../components/ui";
import { formatPlayTime, formatRelative, loaderLabel } from "../lib/format";
import { t } from "../lib/i18n";
import { normal } from "../lib/motion";
import { SMALL_PX } from "../lib/pixelScale";
import { useShots } from "../lib/screenshots";
import { useServers } from "../lib/servers";
import { findInstance, usePick, useStore, type ManageTab } from "../store";
import { BrowseTab } from "./manage/BrowseTab";
import { GeneralTab } from "./manage/GeneralTab";
import { ModsTab } from "./manage/ModsTab";
import { PacksTab } from "./manage/PacksTab";
import { SavesTab } from "./manage/SavesTab";
import { isPluginServer } from "./manage/shared";
import { ToolsTab } from "./manage/ToolsTab";
import { OptionsTab } from "./OptionsTab";
import { ScreenshotsTab } from "./Screenshots";
import { ServersTab } from "./ServersTab";

export function ManageInstance({ id, tab }: { id: string; tab: ManageTab }) {
  const s = usePick((s) => ({
    instance: findInstance(s, id),
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
  // A server project only keeps its mods or plugins here.
  const project = !!s.instance.server;
  const plugins = isPluginServer(s.instance.loader);
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
          eyebrow={project ? t("开服项目", "Server project") : t("管理世界", "Manage world")}
          title={s.instance.name}
          meta={
            <>
              <span className="mono">{s.instance.versionId}</span>
              <span className="dot" />
              {loaderLabel(s.instance.loader)}
              {!project && <span className="dot" />}
              {!project && formatRelative(s.instance.lastLaunchedAt)}
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
              tabs={project ? [
                { value: "mods", label: <>{plugins ? t("插件", "Plugins") : t("模组", "Mods")}{s.modCount > 0 && <span className="tab-count">{s.modCount}</span>}</> },
                { value: "browse", label: t("下载", "Download") },
              ] : [
                { value: "general", label: t("常规", "General") },
                { value: "options", label: t("游戏选项", "Options") },
                { value: "mods", group: true, label: <>{t("模组", "Mods")}{s.modCount > 0 && <span className="tab-count">{s.modCount}</span>}</> },
                { value: "packs", label: t("资源包与光影", "Packs") },
                { value: "browse", label: t("下载", "Download") },
                { value: "saves", group: true, label: t("存档", "Saves") },
                { value: "servers", label: <>{t("多人游戏", "Servers")}{serverCount > 0 && <span className="tab-count">{serverCount}</span>}</> },
                { value: "shots", label: <>{t("截图", "Screenshots")}{shotCount > 0 && <span className="tab-count">{shotCount}</span>}</> },
                { value: "tools", group: true, label: t("工具", "Tools") },
              ]}
            />
          }
        />
      }
    >
      {s.busy && (
        <div className="manage-busy">
          <Sign tone="warn" unit={SMALL_PX} />
          {t("游戏运行或任务进行中，暂不可修改世界", "Cannot edit this world while running or busy")}
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
          {tab === "saves" && <SavesTab instance={s.instance} onOpenTools={() => setTab("tools")} />}
          {tab === "tools" && <ToolsTab instance={s.instance} busy={s.busy} />}
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
