import { useEffect, useState } from "react";

import { call } from "../api/backend";
import type { Datapack, SaveBackup } from "../api/types";
import { Icon } from "../components/Icon";
import { Modal } from "../components/overlay";
import { PixelIcon } from "../components/Pixel";
import { Button, Identicon, Input } from "../components/ui";
import { formatBytes, formatRelative } from "../lib/format";
import { useWorlds } from "../lib/worlds";
import { usePick, useStore } from "../store";
import { isEnglish, t } from "../lib/i18n";

/** One save of a world, the way the game's "Edit World" screen offers it:
    rename, back up and restore, export, its datapacks, and removal. */
export function SaveDialog({ id, folder }: { id: string; folder: string }) {
  const s = usePick((s) => ({
    instance: s.instances.find((i) => i.id === id) ?? null,
    fileBusy: s.fileBusy,
    gameBusy: s.runningIds.includes(id) || s.launchPending === id || s.install?.instanceId === id,
    account: s.settings.activeAccountId,
    filesChanged: s.filesChanged,
    fileOperation: s.fileOperation,
    chooseFiles: s.chooseFiles,
    chooseSavePath: s.chooseSavePath,
    openModal: s.openModal,
    open: s.open,
    launch: s.launchInstance,
    openManage: s.openManage,
  }));
  const list = useWorlds(s.instance);
  const world = list?.worlds.find((w) => w.folder === folder) ?? null;
  const shown = world?.name ?? folder;
  const [name, setName] = useState(shown);
  const [renamed, setRenamed] = useState<string | null>(null);
  const [packs, setPacks] = useState<Datapack[] | null>(null);
  const [backups, setBackups] = useState<SaveBackup[] | null>(null);
  const [confirm, setConfirm] = useState<string | null>(null);
  const busy = s.fileBusy || s.gameBusy;
  const back = () => s.openModal({ kind: "manage", id, tab: "data" });

  useEffect(() => setName(shown), [shown]);

  // A rename can move the folder too: follow the save to its new one.
  useEffect(() => {
    if (!list || world || !renamed) return;
    const moved = list.worlds.find((w) => w.name === renamed);
    if (moved) s.openModal({ kind: "save", id, folder: moved.folder });
  }, [list, world, renamed]);

  useEffect(() => {
    if (!world) return;
    let live = true;
    call<Datapack[]>("list_datapacks", { id, save: folder })
      .then((value) => live && setPacks(value))
      .catch(() => live && setPacks([]));
    call<SaveBackup[]>("save_backups", { id, save: folder })
      .then((value) => live && setBackups(value))
      .catch(() => live && setBackups([]));
    return () => {
      live = false;
    };
  }, [id, folder, Boolean(world), s.filesChanged]);

  if (!s.instance) return null;
  const run = (operation: Parameters<typeof s.fileOperation>[1], quiet?: boolean) => void s.fileOperation(id, operation, quiet);

  return (
    <Modal
      title={shown}
      subtitle={world ? t(`${s.instance.name} · ${formatBytes(world.bytes)} · ${world.modifiedMs ? `上次游玩 ${formatRelative(world.modifiedMs)}` : "尚未游玩"}`, `${s.instance.name} · ${formatBytes(world.bytes)} · ${world.modifiedMs ? `last played ${formatRelative(world.modifiedMs)}` : "never played"}`) : list ? t("这个存档已经不在了", "This save is gone") : t("正在读取…", "Loading…")}
      icon={world?.icon ? <img className="save-icon" src={world.icon} alt="" /> : <Identicon seed={`${id}:${folder}`} size={36} />}
      onClose={back}
      footer={
        <>
          <Button variant="ghost" icon="chevronLeft" onClick={back} style={{ marginRight: "auto" }}>
            {t("返回存档列表", "Back to saves")}
          </Button>
          <Button
            variant="go"
            icon="play"
            disabled={!world || !s.instance.installed || !s.account || busy || !list?.quickPlay}
            title={!list?.quickPlay ? t("直接进入存档需要 Minecraft 1.20 或更高版本", "Opening a save directly needs Minecraft 1.20 or later") : t("开始游戏并直接进入这个存档", "Start the game straight into this save")}
            onClick={() => {
              useStore.getState().closeModal();
              void s.launch(id, folder);
            }}
          >
            {t("进入存档", "Open save")}
          </Button>
        </>
      }
    >
      {world && (
        <div className="save-dialog">
          <form
            className="row"
            onSubmit={(e) => {
              e.preventDefault();
              const next = name.trim();
              if (!next || next === shown) return;
              setRenamed(next);
              run({ kind: "renameSave", folder, name: next }, true);
            }}
          >
            <div style={{ flex: 1 }}>
              <Input value={name} maxLength={64} disabled={busy} aria-label={t("存档名", "Save name")} onChange={(e) => setName(e.target.value)} />
            </div>
            <Button type="submit" disabled={busy || !name.trim() || name.trim() === shown}>
              {t("改名", "Rename")}
            </Button>
          </form>
          <span className="field-hint">{t("改的是游戏里显示的名字；文件夹「", "Changes the name shown in game; the folder \"")}{folder}{t("」会尽量一起改。", "\" is renamed too when possible.")}</span>

          <div className="save-actions">
            <Button size="sm" icon="archive" disabled={busy} onClick={() => run({ kind: "backupSave", folder })}>
              {t("备份", "Back up")}
            </Button>
            <Button
              size="sm"
              icon="external"
              disabled={busy}
              onClick={async () => {
                const destination = await s.chooseSavePath(t("导出存档", "Export save"), `${shown}.zip`, "zip");
                if (destination) run({ kind: "exportSave", folder, destination });
              }}
            >
              {t("导出 .zip", "Export .zip")}
            </Button>
            <Button size="sm" icon="folder" onClick={() => void s.open({ kind: "world", id, folder })}>
              {t("打开文件夹", "Open folder")}
            </Button>
            <span className="spacer" />
            <Button
              size="sm"
              variant={confirm === "remove" ? "danger" : "danger-ghost"}
              icon="trash"
              disabled={busy}
              onBlur={() => setConfirm(null)}
              onClick={() => {
                if (confirm !== "remove") return setConfirm("remove");
                run({ kind: "recycle", item: "save", name: folder }, true);
                back();
              }}
            >
              {confirm === "remove" ? t("确认移到回收站", "Confirm move to recycle bin") : t("移到回收站", "Move to recycle bin")}
            </Button>
          </div>

          <section className="save-section">
            <div className="section-label">
              {t("数据包", "Datapacks")}
              {packs && packs.length > 0 && <span className="mono faint">{packs.length}{t(" 个", "")}</span>}
              <span className="spacer" />
              <Button size="sm" variant="ghost" icon="search" onClick={() => s.openManage(id, "browse", "", "datapack")}>
                {t("在线下载", "Find online")}
              </Button>
              <Button
                size="sm"
                variant="ghost"
                icon="plus"
                disabled={busy}
                onClick={async () => {
                  const paths = await s.chooseFiles("datapacks");
                  if (paths.length) run({ kind: "addDatapacks", save: folder, paths });
                }}
              >
                {t("添加数据包", "Add datapacks")}
              </Button>
            </div>
            {packs == null ? (
              <div className="saves-empty muted">
                <span className="spinner" />{t(" 正在读取…", " Loading…")}
              </div>
            ) : packs.length === 0 ? (
              <div className="saves-empty muted">{t("这个存档没有数据包。添加的 .zip 会在下次进入存档时生效。", "This save has no datapacks. Added .zip files apply next time you open the save.")}</div>
            ) : (
              <div className="list">
                {packs.map((pack) => (
                  <div key={pack.name} className="list-row">
                    <PixelIcon name="pack" size={20} />
                    <div className="col" style={{ flex: 1, minWidth: 0, gap: 2 }}>
                      <span className="truncate" style={{ fontWeight: 600 }}>
                        {pack.name.replace(/\.zip$/i, "")}
                      </span>
                      <span className="faint truncate" style={{ fontSize: "var(--text-xs)" }}>
                        {pack.description ?? (pack.folder ? t("文件夹", "Folder") : "ZIP")} · {formatBytes(pack.bytes)}
                      </span>
                    </div>
                    <Button
                      size="sm"
                      variant="ghost"
                      icon="trash"
                      disabled={busy}
                      aria-label={t(`把 ${pack.name} 移到回收站`, `Move ${pack.name} to the recycle bin`)}
                      title={t("移到回收站", "Move to recycle bin")}
                      onClick={() => run({ kind: "recycle", item: "datapack", name: pack.name, save: folder }, true)}
                    />
                  </div>
                ))}
              </div>
            )}
          </section>

          <section className="save-section">
            <div className="section-label">
              {t("备份", "Backups")}
              {backups && backups.length > 0 && <span className="mono faint">{backups.length}{t(" 个", " more")}</span>}
            </div>
            {backups == null ? null : backups.length === 0 ? (
              <div className="saves-empty muted">{t("还没有备份。点上面的「备份」把这个存档打包保存一份；打开「设置 → 游戏退出后自动备份存档」后，每次玩完会自动备份。", "No backups yet. Use Back up above to keep a copy of this save, or turn on Settings → Back up saves after playing.")}</div>
            ) : (
              <div className="list">
                {backups.map((backup) => (
                  <div key={backup.path} className="list-row">
                    <Icon name="archive" size={15} className="faint" />
                    <div className="col" style={{ flex: 1, minWidth: 0, gap: 2 }}>
                      <span style={{ fontWeight: 600 }}>{new Date(backup.createdMs).toLocaleString(isEnglish ? "en-US" : "zh-CN", { hour12: false })}</span>
                      <span className="faint" style={{ fontSize: "var(--text-xs)" }}>
                        <span className="mono">{formatBytes(backup.bytes)}</span>
                        {backup.auto ? t(" · 玩完自动备份", " · after playing") : t(" · 手动备份", " · by hand")}
                      </span>
                    </div>
                    <Button
                      size="sm"
                      variant={confirm === backup.path ? "primary" : "default"}
                      icon="refresh"
                      disabled={busy}
                      onBlur={() => setConfirm(null)}
                      onClick={() => {
                        if (confirm !== backup.path) return setConfirm(backup.path);
                        setConfirm(null);
                        run({ kind: "restoreSave", folder, backup: backup.path });
                      }}
                    >
                      {confirm === backup.path ? t("确认恢复", "Confirm restore") : t("恢复", "Restore")}
                    </Button>
                  </div>
                ))}
              </div>
            )}
            <span className="field-hint">{t("恢复前，存档现在的样子会先移到回收站，不会丢。", "Before restoring, the save as it is now moves to the recycle bin, so nothing is lost.")}</span>
          </section>
        </div>
      )}
    </Modal>
  );
}
