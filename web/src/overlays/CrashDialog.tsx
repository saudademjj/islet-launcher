import { useEffect, useState } from "react";

import type { CrashAnalysis, CrashFix } from "../api/types";
import { Modal } from "../components/overlay";
import { ShareLogButton } from "../components/ShareLog";
import { SceneHeader } from "../components/SceneHeader";
import { Button, Chip, Identicon, Notice } from "../components/ui";
import { usePick } from "../store";
import { t } from "../lib/i18n";

type FixState = "busy" | "done" | "opened";

/**
 * Shown when the game exits abnormally and the user did not press stop.
 * Diagnostics come from the Rust analyzer; each card offers the matching
 * one-click repair. Nothing is marked handled until the async work reports
 * success.
 */
export function CrashDialog({
  instanceId,
  code,
  analysis,
}: {
  instanceId: string;
  code: number | null;
  analysis: CrashAnalysis;
}) {
  const s = usePick((s) => ({
    instance: s.instances.find((item) => item.id === instanceId) ?? null,
    install: s.install,
    installOutcome: s.installOutcome,
    closeModal: s.closeModal,
    copy: s.copy,
    openLogs: s.openLogs,
    openManage: s.openManage,
    installInstance: s.installInstance,
    installJavaForInstance: s.installJavaForInstance,
    setInstanceMemory: s.setInstanceMemory,
    launchInstance: s.launchInstance,
    // Busy with this world elsewhere: started again, or still downloading.
    occupied: s.runningIds.includes(instanceId) || s.launchPending === instanceId || s.install?.instanceId === instanceId,
  }));
  const [states, setStates] = useState<Record<number, FixState | undefined>>({});
  const [repairing, setRepairing] = useState<number | null>(null);

  useEffect(() => {
    if (repairing == null) return;
    const outcome = s.installOutcome;
    if (!outcome || outcome.instanceId !== instanceId) return;
    setStates((previous) => ({ ...previous, [repairing]: outcome.ok ? "done" : undefined }));
    setRepairing(null);
  }, [s.installOutcome, instanceId, repairing]);

  const setState = (index: number, state: FixState | undefined) =>
    setStates((previous) => ({ ...previous, [index]: state }));

  const runFix = async (index: number, fix: CrashFix) => {
    if (fix.kind === "useJava") {
      setState(index, "busy");
      const ok = await s.installJavaForInstance(instanceId, fix.major);
      setState(index, ok ? "done" : undefined);
    } else if (fix.kind === "raiseMemory") {
      setState(index, "busy");
      const ok = await s.setInstanceMemory(instanceId, fix.mb);
      setState(index, ok ? "done" : undefined);
    } else if (fix.kind === "installMod") {
      // Opening the download page is not a completed repair.
      setState(index, "opened");
      s.openManage(instanceId, "browse", fix.query);
    } else if (fix.kind === "openMods") {
      setState(index, "opened");
      s.openManage(instanceId, "mods");
    } else if (fix.kind === "repair") {
      // installInstance clears the previous outcome before returning; only
      // then may the dialog start watching for the new result.
      const started = await s.installInstance(instanceId);
      if (!started) {
        setState(index, undefined);
        return;
      }
      setState(index, "busy");
      setRepairing(index);
    }
  };

  // Once something is fixed, trying again is the next step.
  const fixed = Object.values(states).includes("done");
  const working = Object.values(states).includes("busy");
  const exit = code != null ? t(`退出码 ${code}`, `Exit code ${code}`) : t("退出码未知", "Exit code unknown");
  const name = s.instance?.name ?? t("世界", "Worlds");
  const report = [
    t(`游戏意外退出：${name}`, `Game crashed: ${name}`),
    s.instance ? t(`版本：${s.instance.versionId} · ${exit}`, `Version: ${s.instance.versionId} · ${exit}`) : exit,
    ...analysis.diagnoses.flatMap((diagnosis) => [t(`【${diagnosis.title}】`, `[${diagnosis.title}]`), diagnosis.detail]),
    analysis.excerpt ? t(`\n---- 关键日志 ----\n${analysis.excerpt}`, `\n---- Key log lines ----\n${analysis.excerpt}`) : "",
  ]
    .filter(Boolean)
    .join("\n");

  return (
    <Modal
      scene={
        <SceneHeader
          seed={instanceId}
          tone="danger"
          eyebrow={t("游戏意外退出", "The game crashed")}
          title={name}
          meta={s.instance ? <span className="mono">{s.instance.versionId}</span> : undefined}
          aside={<Identicon seed={instanceId} size={46} hero />}
          onClose={s.closeModal}
          strip={
            <div className="crash-strip">
              <span className="mono">{exit}</span>
              <span className="dot" />
              {analysis.diagnoses.length > 0 ? t(`找到 ${analysis.diagnoses.length} 个可能的原因`, `${analysis.diagnoses.length} likely causes found`) : t("没有识别出原因", "No cause recognized")}
            </div>
          }
        />
      }
      onClose={s.closeModal}
      footer={
        <>
          <span className="crash-report-note">{t("诊断只使用本次启动的日志", "Uses only this launch's log")}</span>
          <Button size="sm" icon="copy" onClick={() => void s.copy(report, t("诊断报告", "Diagnosis report"))}>
            {t("复制诊断报告", "Copy report")}
          </Button>
          <ShareLogButton instanceId={instanceId} />
          <Button
            size="sm"
            variant="ghost"
            icon="log"
            onClick={() => {
              s.closeModal();
              void s.openLogs(instanceId);
            }}
          >
            {t("查看日志", "Show log")}
          </Button>
          <Button size="sm" variant={fixed ? "ghost" : "default"} onClick={s.closeModal}>
            {t("关闭", "Close")}
          </Button>
          {fixed && (
            <Button
              size="sm"
              variant="primary"
              icon="play"
              disabled={working || s.occupied}
              onClick={() => {
                s.closeModal();
                void s.launchInstance(instanceId);
              }}
            >
              {t("重新开始游戏", "Play again")}
            </Button>
          )}
        </>
      }
    >
      {analysis.diagnoses.length === 0 ? (
        <Notice tone="warn" title={t("没能识别原因", "Couldn't tell the cause")}>
          {t("这次启动的日志里没有匹配到已知的崩溃特征。可以查看完整日志，或把诊断报告附在问题反馈中。", "Nothing in this launch's log matches a known crash. Look at the full log, or attach the report when asking for help.")}
        </Notice>
      ) : (
        <div className="crash-list">
          {analysis.diagnoses.map((diagnosis, index) => {
            const fix = diagnosis.fix;
            const state = states[index];
            return (
              <div key={`${diagnosis.kind}-${index}`} className={`crash-card${state === "done" ? " done" : ""}`}>
                <span className="crash-card-index mono">{String(index + 1).padStart(2, "0")}</span>
                <div className="crash-card-body">
                  <span className="crash-card-title">{diagnosis.title}</span>
                  <span className="crash-card-detail">{diagnosis.detail}</span>
                </div>
                <div className="crash-actions">
                  {!fix || fix.kind === "none" ? (
                    <Chip>{t("需手动处理", "Needs a manual fix")}</Chip>
                  ) : state === "done" ? (
                    <Button size="sm" variant="ghost" icon="check" disabled>
                      {t("已处理", "Done")}
                    </Button>
                  ) : state === "opened" ? (
                    <Button size="sm" variant="ghost" icon="folder" disabled>
                      {t("已打开", "Opened")}
                    </Button>
                  ) : state === "busy" ? (
                    <Button size="sm" busy>
                      {t("处理中…", "Working…")}
                    </Button>
                  ) : (
                    <Button size="sm" onClick={() => void runFix(index, fix)}>
                      {fixLabel(fix)}
                    </Button>
                  )}
                </div>
              </div>
            );
          })}
          {analysis.excerpt && (
            <details className="crash-excerpt">
              <summary>{t("关键日志", "Key log lines")}</summary>
              <pre className="mono selectable">{analysis.excerpt}</pre>
            </details>
          )}
          {repairing != null && s.install ? (
            <div className="row muted crash-progress">
              <span className="spinner" />{t(" 正在校验并修复游戏文件，请稍候…", " Verifying and repairing game files…")}
            </div>
          ) : null}
        </div>
      )}
    </Modal>
  );
}

function fixLabel(fix: CrashFix): string {
  switch (fix.kind) {
    case "useJava":
      return t(`准备 Java ${fix.major}`, `Get Java ${fix.major}`);
    case "raiseMemory":
      return t(`设为 ${fix.mb} MB`, `Set to ${fix.mb} MB`);
    case "installMod":
      return t(`下载 ${fix.query}`, `Download ${fix.query}`);
    case "openMods":
      return t("管理模组", "Manage mods");
    case "repair":
      return t("校验并修复", "Verify and repair");
    default:
      return t("处理", "Fix");
  }
}
