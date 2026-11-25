import { useEffect, useState, type ReactNode } from "react";

import { call } from "../../api/backend";
import type { Instance, ModVersion, PackInfo } from "../../api/types";
import { Icon, type IconName } from "../../components/Icon";
import { Button } from "../../components/ui";
import { t } from "../../lib/i18n";
import { usePick } from "../../store";

interface ToolRow {
  icon: IconName;
  title: string;
  text?: string;
  action: ReactNode;
}

export function ToolsTab({ instance, busy }: { instance: Instance; busy: boolean }) {
  const s = usePick((s) => ({
    fileOperation: s.fileOperation,
    chooseFiles: s.chooseFiles,
    open: s.open,
    installInstance: s.installInstance,
    openModal: s.openModal,
    filesChanged: s.filesChanged,
  }));
  const [pack, setPack] = useState<PackInfo | null>(null);
  // undefined while checking; null when the pack is current or can't be checked.
  const [newer, setNewer] = useState<ModVersion | null | undefined>(null);
  useEffect(() => {
    let live = true;
    setNewer(null);
    call<PackInfo | null>("modpack_info", { id: instance.id }).then(
      (info) => {
        if (!live) return;
        setPack(info);
        if (!info?.source) return;
        setNewer(undefined);
        call<ModVersion | null>("modpack_update", { id: instance.id }).then(
          (v) => live && setNewer(v),
          () => live && setNewer(null),
        );
      },
      () => live && setPack(null),
    );
    return () => {
      live = false;
    };
  }, [instance.id, s.filesChanged]);

  const chooseNewer = async () => {
    const [path] = await s.chooseFiles("pack");
    if (path) void s.fileOperation(instance.id, { kind: "updatePack", path });
  };

  const shareRows: ToolRow[] = [
    {
      icon: "external",
      title: t("导出为整合包", "Export as modpack"),
      text: t("导出为 .mrpack 或 CurseForge 整合包，便于分享", "Export as a .mrpack or CurseForge pack to share"),
      action: (
        <Button size="sm" disabled={busy} onClick={() => s.openModal({ kind: "exportPack", id: instance.id })}>
          {t("导出…", "Export…")}
        </Button>
      ),
    },
    {
      icon: "server",
      title: t("导出为服务端", "Export as server"),
      text: t("生成可直接运行的服务端与启动脚本，自动排除客户端模组", "Generate a ready-to-run server and start scripts, excluding client-only mods"),
      action: (
        <Button size="sm" disabled={busy} onClick={() => s.openModal({ kind: "exportServer", id: instance.id })}>
          {t("导出…", "Export…")}
        </Button>
      ),
    },
    {
      icon: "globe",
      title: t("部署到云服务器", "Deploy to a cloud server"),
      text: t("上传存档与模组到 Linux 云服务器长期运行", "Upload saves and mods to a Linux server"),
      action: (
        <Button size="sm" onClick={() => s.openModal({ kind: "remote", id: instance.id })}>
          {t("打开…", "Open…")}
        </Button>
      ),
    },
  ];

  if (pack) {
    const from = t(`当前：${pack.name}${pack.version ? ` ${pack.version}` : ""} · `, `Current: ${pack.name}${pack.version ? ` ${pack.version}` : ""} · `);
    shareRows.unshift({
      icon: "download",
      title: newer ? t(`整合包有新版 ${newer.version_number}`, `Modpack ${newer.version_number} is out`) : t("更新整合包", "Update modpack"),
      text: !pack.source
        ? from + t("选择新版整合包文件以更新", "Pick a newer modpack file to update")
        : newer === undefined
          ? from + t("正在检查更新…", "Checking for updates…")
          : newer
            ? from + t("可更新到新版，移除的旧模组将移入回收站", "Update available; removed mods go to the recycle bin")
            : from + t("已是最新版本，也可手动选择整合包文件", "Up to date; or choose a modpack file manually"),
      action: (
        <>
          <Button size="sm" variant={newer ? "ghost" : undefined} disabled={busy} onClick={() => void chooseNewer()}>
            {t("选择文件…", "Choose file…")}
          </Button>
          {newer && (
            <Button size="sm" variant="go" icon="download" disabled={busy} title={newer.name} onClick={() => void s.fileOperation(instance.id, { kind: "updateModpackOnline", version: newer.id })}>
              {t("更新", "Update")}
            </Button>
          )}
        </>
      ),
    });
  }

  const backupRows: ToolRow[] = [
    {
      icon: "archive",
      title: t("备份存档与配置", "Back up saves and settings"),
      text: t("将整个世界打包为备份文件", "Pack the entire world into a backup file"),
      action: (
        <>
          <Button size="sm" variant="ghost" icon="folder" onClick={() => void s.open({ kind: "backupsDir", id: instance.id })}>
            {t("备份目录", "Backups folder")}
          </Button>
          <Button size="sm" disabled={busy} onClick={() => void s.fileOperation(instance.id, { kind: "backup" })}>
            {t("立即备份", "Back up now")}
          </Button>
        </>
      ),
    },
    {
      icon: "refresh",
      title: t("从备份恢复", "Restore from backup"),
      text: t("恢复前自动备份当前数据，仅限同版本和加载器", "Backs up current data first; same version and loader only"),
      action: (
        <Button
          size="sm"
          disabled={busy}
          onClick={async () => {
            const [path] = await s.chooseFiles("restore");
            if (path) void s.fileOperation(instance.id, { kind: "restoreBackup", path });
          }}
        >
          {t("选择备份…", "Choose backup…")}
        </Button>
      ),
    },
    {
      icon: "copy",
      title: t("复制世界", "Duplicate world"),
      text: t("包括存档、模组和设置", "Saves, mods and settings included"),
      action: (
        <Button size="sm" disabled={busy} onClick={() => void s.fileOperation(instance.id, { kind: "duplicate" })}>
          {t("复制", "Duplicate")}
        </Button>
      ),
    },
    {
      icon: "shield",
      title: t("校验并修复", "Verify and repair"),
      text: t("重新下载缺失或损坏的游戏文件，不影响存档", "Redownload missing or damaged game files without touching saves"),
      action: (
        <Button size="sm" disabled={busy} onClick={() => void s.installInstance(instance.id)}>
          {t("开始校验", "Verify")}
        </Button>
      ),
    },
  ];

  const folderRows: ToolRow[] = [
    {
      icon: "folder",
      title: t("游戏目录", "Game folder"),
      action: (
        <Button size="sm" onClick={() => void s.open({ kind: "gameDir", id: instance.id })}>
          {t("打开", "Open")}
        </Button>
      ),
    },
    {
      icon: "trash",
      title: t("移入回收站", "Move to recycle bin"),
      text: t("文件不会立即删除，可在回收站恢复", "Nothing is deleted immediately; restore from the recycle bin"),
      action: (
        <Button size="sm" variant="danger-ghost" disabled={busy} onClick={() => s.openModal({ kind: "deleteInstance", instance })}>
          {t("移入回收站", "Move to recycle bin")}
        </Button>
      ),
    },
  ];

  const groups: { title: string; rows: ToolRow[] }[] = [
    { title: t("分享与开服", "Share & server"), rows: shareRows },
    { title: t("备份与修复", "Backup & repair"), rows: backupRows },
    { title: t("目录与移除", "Folder & remove"), rows: folderRows },
  ];

  return (
    <div className="manage-scroll">
      {groups.map((group) => (
        <section key={group.title} className="col gap-8">
          <div className="section-label">{group.title}</div>
          <div className="list">
            {group.rows.map((row) => (
              <div key={row.title} className="list-row" style={{ minHeight: 58 }}>
                <Icon name={row.icon} className="faint" />
                <div className="col fill gap-2">
                  <span style={{ fontWeight: 600 }}>{row.title}</span>
                  {row.text && (
                    <span className="muted" style={{ fontSize: "var(--text-xs)" }}>
                      {row.text}
                    </span>
                  )}
                </div>
                <div className="row">{row.action}</div>
              </div>
            ))}
          </div>
        </section>
      ))}
    </div>
  );
}
