import { useEffect, useState } from "react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { listen } from "@tauri-apps/api/event";
import { call, inTauri, toError } from "../api/backend";
import type { Snapshot } from "../api/types";
import { usePick, useStore } from "../store";
import { t } from "../lib/i18n";
import { Modal } from "./overlay";
import { Button } from "./ui";
import { Icon } from "./Icon";

type Pending = { paths: string[]; account?: string };
export function FileDrop() {
  const [pending, setPending] = useState<Pending | null>(null);
  const [over, setOver] = useState(false);
  const [target, setTarget] = useState("");
  const [busy, setBusy] = useState(false);
  const s = usePick(s => ({ instances: s.instances, accounts: s.accounts, activeAccount: s.settings.activeAccountId, fileBusy: s.fileBusy }));
  useEffect(() => {
    if (!inTauri) return;
    let disposed = false;
    const cleanups: (() => void)[] = [];
    const register = (p: Promise<() => void>) => p.then(f => disposed ? f() : cleanups.push(f));
    const receive = (paths: string[], account?: string) => {
      if (!paths.length || disposed) return;
      setTarget(useStore.getState().settings.activeInstanceId ?? "");
      setPending(previous => ({ paths: [...new Set([...(previous?.paths ?? []), ...paths])], account }));
    };
    const drain = () => call<string[]>("take_open_files").then(receive, () => {});
    void register(listen("open-files-ready", () => { void drain(); }));
    void drain();
    void register(getCurrentWebviewWindow().onDragDropEvent(e => {
      setOver(e.payload.type === "enter" || e.payload.type === "over");
      if (e.payload.type === "drop") {
        const { x, y } = e.payload.position;
        const el = document.elementFromPoint(x / devicePixelRatio, y / devicePixelRatio);
        const account = el?.closest<HTMLElement>("[data-drop-account]")?.dataset.dropAccount;
        receive(e.payload.paths, account);
      }
    }));
    return () => { disposed = true; cleanups.forEach(f => f()); };
  }, []);
  if (!pending) return over ? (
    <div className="drop-hint">
      <div className="drop-hint-card">
        <Icon name="drop" size={32} />
        <span className="drop-hint-title">{t("松手添加", "Drop to add")}</span>
        <span className="drop-hint-kinds">{t("模组 .jar · 整合包 .mrpack / .zip · 皮肤 .png", "Mods .jar · modpacks .mrpack / .zip · skins .png")}</span>
      </div>
    </div>
  ) : null;
  const suffix = (p: string) => p.split(".").pop()?.toLowerCase();
  const jars = pending.paths.every(p => suffix(p) === "jar" || suffix(p) === "litemod");
  const skin = pending.paths.length === 1 && suffix(pending.paths[0]) === "png";
  const pack = pending.paths.length === 1 && ["zip", "mrpack"].includes(suffix(pending.paths[0]) ?? "");
  const account = pending.account ?? s.activeAccount;
  const owner = s.accounts.find(a => a.id === account);
  const names = pending.paths.map(p => p.split(/[\\/]/).pop());
  const execute = async () => {
    const state = useStore.getState();
    setBusy(true);
    try {
      if (skin) state.applySnapshot(await call<Snapshot>("change_skin", { id: account, path: pending.paths[0], slim: owner?.skinSlim ?? false }));
      else if (jars) await state.fileOperation(target, { kind: "addMods", paths: pending.paths });
      else if (pack) await state.fileOperation(null, { kind: "importPack", path: pending.paths[0] });
      setPending(null);
    } catch (e) { state.toast(toError(e).message, "error"); }
    finally { setBusy(false); }
  };
  return <Modal title={t("添加拖入的文件", "Add dropped files")} onClose={() => !busy && setPending(null)} footer={<Button variant="primary" busy={busy} disabled={s.fileBusy || !(jars ? !!target : skin ? !!account : pack)} onClick={() => void execute()}>{t("添加", "Add")}</Button>}>
    <div className="col gap-16">
      <div className="list">
        {names.slice(0, 6).map((name, i) => <div key={i} className="list-row mono truncate" style={{ fontSize: "var(--text-sm)" }}>{name}</div>)}
        {names.length > 6 && <div className="list-row faint" style={{ fontSize: "var(--text-xs)" }}>{t(`还有 ${names.length - 6} 个文件`, `${names.length - 6} more files`)}</div>}
      </div>
      {jars && (
        <div className="filter-row" role="group" aria-label={t("安装到世界", "Install into world")}>
          <span className="filter-label">{t("装进世界", "Into world")}</span>
          <div className="filter-chips">
            {s.instances.length === 0
              ? <span className="faint" style={{ fontSize: "var(--text-xs)" }}>{t("还没有世界，先创建一个再添加模组。", "No worlds yet; create one before adding mods.")}</span>
              : s.instances.map(i => (
                <button key={i.id} type="button" className="filter-chip" aria-pressed={i.id === target} onClick={() => setTarget(i.id)}>
                  {i.name}
                </button>
              ))}
          </div>
        </div>
      )}
      {skin && <span className="muted">{owner ? t(`设为「${owner.username}」的皮肤。`, `Set as ${owner.username}'s skin.`) : t("请先添加或选择一个账号。", "Add or choose an account first.")}</span>}
      {pack && <span className="muted">{t("导入为独立的新世界，原有世界不受影响。", "Imports as a separate new world; your other worlds stay as they are.")}</span>}
      {!jars && !skin && !pack && <span className="muted">{t("一次拖入一批模组，或者一个整合包 / PNG 皮肤。", "Drop a batch of mods, or one modpack / PNG skin at a time.")}</span>}
    </div>
  </Modal>;
}
