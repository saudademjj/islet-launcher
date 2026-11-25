import { Modal } from "../components/overlay";
import { Button, Notice } from "../components/ui";
import type { ServerModCheck } from "../api/types";
import { usePick, useStore } from "../store";
import { t } from "../lib/i18n";

/** Before joining a server made with Islet: the mods it runs that this
    world lacks, fetched in one go so the server doesn't turn the player away. */
export function ServerModsDialog({ id, address, check, offline }: { id: string; address: string; check: ServerModCheck; offline: boolean }) {
  const s = usePick((s) => ({
    fileBusy: s.fileBusy,
    closeModal: s.closeModal,
    fileOperation: s.fileOperation,
    launchInstance: s.launchInstance,
  }));
  const join = () => {
    s.closeModal();
    void s.launchInstance(id, undefined, address, offline, true);
  };
  const fix = () => {
    if (s.fileBusy) return;
    s.closeModal();
    useStore.setState({ joinAfterFix: { id, address, offline } });
    void s.fileOperation(id, { kind: "fixServerMods", address });
  };
  const shown = check.missing.slice(0, 12);
  return (
    <Modal
      title={t("服务器的模组和你的不一样", "The server runs different mods")}
      subtitle={check.server || address}
      onClose={s.closeModal}
      footer={
        <>
          <Button variant="ghost" onClick={s.closeModal}>
            {t("取消", "Cancel")}
          </Button>
          <Button onClick={join}>{t("仍然进入", "Join anyway")}</Button>
          {check.fixable && (
            <Button variant="primary" icon="download" disabled={s.fileBusy} onClick={fix}>
              {t("补齐模组后进入", "Get the mods and join")}
            </Button>
          )}
        </>
      }
    >
      <div className="col gap-16">
        <span className="field-hint">
          {t(
            `你缺少服务器的 ${check.missing.length} 个模组${check.outdated.length ? `，另有 ${check.outdated.length} 个版本过旧` : ""}。`,
            `You're missing ${check.missing.length} server mod(s)${check.outdated.length ? `, with ${check.outdated.length} outdated` : ""}.`,
          )}
        </span>
        <div className="list" style={{ maxHeight: 240, overflowY: "auto" }}>
          {shown.map((m) => (
            <div key={m.sha1} className="list-row">
              <span className="mono truncate" style={{ flex: 1 }}>
                {m.file}
              </span>
              <span className={`field-hint${m.url ? "" : " bad"}`}>{m.url ? t("可下载", "Downloadable") : t("需要整合包", "Needs the pack")}</span>
            </div>
          ))}
          {check.missing.length > shown.length && <div className="list-row field-hint">{t(`还有 ${check.missing.length - shown.length} 个…`, `${check.missing.length - shown.length} more…`)}</div>}
        </div>
        {check.outdated.length > 0 && (
          <span className="field-hint">
            {t(`补齐时，旧版本（${check.outdated.join("、")}）会移入回收站。`, `Getting them moves the old versions (${check.outdated.join(", ")}) to the recycle bin.`)}
          </span>
        )}
        {!check.fixable && (
          <Notice tone="warn" title={t("有的模组没法自动下载", "Some mods can't be downloaded")}>
            {t("请向服主获取最新整合包，在「管理 → 工具」中更新。", "Ask the host for the latest modpack and update it under Manage → Tools.")}
          </Notice>
        )}
      </div>
    </Modal>
  );
}
