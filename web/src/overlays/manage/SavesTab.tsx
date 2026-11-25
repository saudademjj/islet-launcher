import { motion } from "motion/react";

import type { Instance } from "../../api/types";
import { BrandMark } from "../../components/Pixel";
import { Button, Identicon } from "../../components/ui";
import { formatBytes, formatRelative } from "../../lib/format";
import { t } from "../../lib/i18n";
import { normal } from "../../lib/motion";
import { useWorlds } from "../../lib/worlds";
import { usePick } from "../../store";

/** Pure singleplayer saves view; world-level backup/export/repair live under Tools. */
export function SavesTab({ instance, onOpenTools }: { instance: Instance; onOpenTools: () => void }) {
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
    <div className="manage-scroll">
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
            title={t("从 .zip 导入存档", "Import a save from .zip")}
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
            title={t("导入本地存档文件夹", "Import a local save folder")}
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
            {t("还没有存档", "No saves yet")}
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
                <span className="col gap-2">
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
      <div className="row muted" style={{ fontSize: "var(--text-xs)" }}>
        <span>{t("需要备份整个世界、导出或校验修复？", "Need a full world backup, export, or repair?")}</span>
        <Button size="sm" variant="ghost" icon="chevronRight" onClick={onOpenTools}>
          {t("打开工具", "Open Tools")}
        </Button>
      </div>
    </div>
  );
}
