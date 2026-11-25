import { useEffect, useRef, useState } from "react";

import { call } from "../../api/backend";
import type { Cleanup, StorageUsage } from "../../api/types";
import { Button } from "../../components/ui";
import { formatBytes } from "../../lib/format";
import { t } from "../../lib/i18n";
import { usePick } from "../../store";
import { Section } from "./shared";

/** What the launcher keeps on disk, and what can go: everything cleaned
    here is downloaded or rebuilt again when needed. */
export function StorageSection() {
  const s = usePick((s) => ({ fileBusy: s.fileBusy, filesChanged: s.filesChanged, fileOperation: s.fileOperation, openModal: s.openModal, open: s.open, openRecycle: s.openRecycle }));
  const [usage, setUsage] = useState<StorageUsage | null>(null);
  const [measuring, setMeasuring] = useState(false);
  const mountedRef = useRef(true);
  const measuringRef = useRef(false);
  const hasUsageRef = useRef(false);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);
  const measure = () => {
    if (measuringRef.current) return;
    measuringRef.current = true;
    setMeasuring(true);
    call<StorageUsage>("storage_usage")
      .then((result) => {
        if (!mountedRef.current) return;
        hasUsageRef.current = true;
        setUsage(result);
      })
      .catch(() => {
        if (!mountedRef.current) return;
        hasUsageRef.current = false;
        setUsage(null);
      })
      .finally(() => {
        measuringRef.current = false;
        if (mountedRef.current) setMeasuring(false);
      });
  };
  // Measuring walks every file, so it waits to be asked; after a cleanup it
  // measures again by itself.
  useEffect(() => {
    if (hasUsageRef.current) measure();
  }, [s.filesChanged]);
  const clean = (target: Cleanup) => void s.fileOperation(null, { kind: "clean", target });
  const rows: { label: string; bytes: number; hint: string; action?: React.ReactNode }[] = usage
    ? [
        { label: t("世界", "Worlds"), bytes: usage.worlds, hint: t("存档、模组、配置和截图", "Saves, mods, configs and screenshots") },
        { label: t("游戏资源文件", "Game assets"), bytes: usage.assets, hint: t("声音、语言和材质；所有世界共用", "Sounds, languages and textures; shared by all worlds") },
        { label: t("运行库", "Libraries"), bytes: usage.libraries, hint: t("游戏和加载器的库；所有世界共用", "Game and loader libraries; shared by all worlds") },
        {
          label: t("版本文件", "Version files"),
          bytes: usage.versions,
          hint: usage.unusedVersions.length ? t(`${usage.unusedVersions.length} 个版本已经没有世界在用：${usage.unusedVersions.slice(0, 4).join("、")}${usage.unusedVersions.length > 4 ? " …" : ""}`, `${usage.unusedVersions.length} versions no world uses: ${usage.unusedVersions.slice(0, 4).join(", ")}${usage.unusedVersions.length > 4 ? " …" : ""}`) : t("都有世界在用", "All in use"),
          action: usage.unusedVersions.length > 0 && (
            <Button size="sm" disabled={s.fileBusy} onClick={() => clean({ kind: "unusedVersions" })}>
              {t("清理 ", "Free ")}{formatBytes(usage.unusedVersionsBytes)}
            </Button>
          ),
        },
        {
          label: t("回收站", "Recycle bin"),
          bytes: usage.recycle,
          hint: t("删除的世界、模组、存档和截图", "Deleted worlds, mods, saves and screenshots"),
          action: (
            <Button size="sm" disabled={usage.recycle === 0} onClick={() => void s.openRecycle()}>
              {t("查看", "View")}
            </Button>
          ),
        },
        {
          label: t("备份", "Backups"),
          bytes: usage.backups,
          hint: t("世界与存档备份", "World and save backups"),
          action: (
            <Button size="sm" icon="folder" onClick={() => void s.open({ kind: "dataDir" })}>
              {t("打开", "Open")}
            </Button>
          ),
        },
        {
          label: t("截图缩略图", "Screenshot thumbnails"),
          bytes: usage.thumbnails,
          hint: t("需要时会重新生成", "Made again when needed"),
          action: usage.thumbnails > 0 && (
            <Button size="sm" disabled={s.fileBusy} onClick={() => clean({ kind: "thumbnails" })}>
              {t("清理", "Clean")}
            </Button>
          ),
        },
        {
          label: t("启动器日志", "Launcher logs"),
          bytes: usage.logs,
          hint: t("清理一周以前的日志", "Removes logs older than a week"),
          action: (
            <Button size="sm" disabled={s.fileBusy} onClick={() => clean({ kind: "oldLogs" })}>
              {t("清理", "Clean")}
            </Button>
          ),
        },
      ]
    : [];
  const total = usage ? usage.worlds + usage.assets + usage.libraries + usage.versions + usage.recycle + usage.backups + usage.thumbnails + usage.logs + usage.java.reduce((sum, j) => sum + j.bytes, 0) : 0;
  return (
    <Section id="storage" icon="disk" title={t("存储", "Storage")} text={t("清理的内容需要时会重新下载或生成", "Anything cleaned here is downloaded or rebuilt when needed")}>
      {!usage ? (
        <div className="row">
          <span className="muted" style={{ flex: 1 }}>
            {t("查看启动器占用的磁盘空间", "Check disk space used by the launcher")}
          </span>
          <Button icon="disk" busy={measuring} onClick={measure}>
            {t("统计占用", "Measure")}
          </Button>
        </div>
      ) : (
        <>
          <div className="storage-total">
            <span className="fact-number">{formatBytes(total)}</span>
            <span className="muted">{t("共占用", "in total")}</span>
            <span className="spacer" />
            <Button size="sm" variant="ghost" icon="refresh" busy={measuring} onClick={measure}>
              {t("重新统计", "Measure again")}
            </Button>
          </div>
          <div className="storage-bar" aria-hidden>
            {rows.map((row) => (
              <span key={row.label} style={{ flexGrow: Math.max(row.bytes, 1) }} title={`${row.label} ${formatBytes(row.bytes)}`} />
            ))}
          </div>
          <div className="list">
            {rows.map((row) => (
              <div key={row.label} className="list-row storage-row">
                <div className="col fill gap-2">
                  <span style={{ fontWeight: 600 }}>{row.label}</span>
                  <span className="faint truncate" style={{ fontSize: "var(--text-xs)" }}>
                    {row.hint}
                  </span>
                </div>
                <span className="mono">{formatBytes(row.bytes)}</span>
                {row.action}
              </div>
            ))}
            {usage.java.map((java) => (
              <div key={java.path} className="list-row storage-row">
                <div className="col fill gap-2">
                  <span style={{ fontWeight: 600 }}>Java {java.major}</span>
                  <span className="faint truncate" style={{ fontSize: "var(--text-xs)" }} title={java.path}>
                    {java.unused ? t("暂无世界使用，可安全删除", "Unused by any world; safe to delete") : t("有世界正在用", "In use")}
                  </span>
                </div>
                <span className="mono">{formatBytes(java.bytes)}</span>
                {java.unused && (
                  <Button size="sm" disabled={s.fileBusy} onClick={() => clean({ kind: "java", path: java.path })}>
                    {t("删除", "Delete")}
                  </Button>
                )}
              </div>
            ))}
          </div>
        </>
      )}
    </Section>
  );
}
