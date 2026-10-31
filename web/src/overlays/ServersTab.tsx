import { AnimatePresence, motion } from "motion/react";
import { useState } from "react";

import { toError } from "../api/backend";
import type { Instance, ServerEdit } from "../api/types";
import { JoinButton, ServerCard } from "../components/Server";
import { Button, Empty, Input } from "../components/ui";
import { formatCount } from "../lib/format";
import { normal } from "../lib/motion";
import { useServerList, useServers } from "../lib/servers";
import { usePick } from "../store";
import { t } from "../lib/i18n";
import { Icon } from "../components/Icon";

type Draft = { index: number | null; name: string; address: string };

/** The instance's multiplayer list: the same servers.dat the game edits,
    with live status and joining straight from the launcher. */
export function ServersTab({ instance, busy }: { instance: Instance; busy: boolean }) {
  const s = usePick((s) => ({ toast: s.toast, closeModal: s.closeModal }));
  const list = useServerList(instance);
  const pings = useServers((st) => st.pings);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [confirm, setConfirm] = useState<number | null>(null);
  const [saving, setSaving] = useState(false);

  const online = list?.reduce((sum, server) => sum + (pings[server.address]?.status?.online ?? 0), 0) ?? 0;
  const reachable = list?.filter((server) => pings[server.address]?.status).length ?? 0;
  const refreshing = list?.some((server) => pings[server.address]?.loading) ?? false;

  const apply = async (change: ServerEdit, success?: string) => {
    setSaving(true);
    try {
      await useServers.getState().edit(instance.id, change);
      if (success) s.toast(success, "success");
      return true;
    } catch (error) {
      s.toast(toError(error).message, "error");
      return false;
    } finally {
      setSaving(false);
    }
  };

  const submit = async () => {
    if (!draft) return;
    const name = draft.name.trim();
    const address = draft.address.trim();
    if (!address || /\s/.test(address)) return s.toast(t("请填写服务器地址，例如 mc.example.com 或 1.2.3.4:25565", "Enter a server address, like mc.example.com or 1.2.3.4:25565"), "error");
    const target = draft.index == null ? null : list?.find((server) => server.index === draft.index);
    const ok =
      draft.index == null
        ? await apply({ kind: "add", name, address }, t("已添加服务器", "Server added"))
        : target
          ? await apply({ kind: "update", index: target.index, expected: target.address, name, address }, t("服务器已保存", "Server saved"))
          : false;
    if (ok) {
      setDraft(null);
      useServers.getState().ping([address], true);
    }
  };

  const form = (key: string) => (
    <ServerForm key={key} draft={draft!} saving={saving} onChange={setDraft} onSubmit={() => void submit()} onCancel={() => setDraft(null)} />
  );

  return (
    <div className="manage-scroll">
      <div className="pane-head">
        <div className="col gap-4">
          <div className="section-label">
            {t("多人游戏", "Multiplayer")}
            {list && list.length > 0 && (
              <span className="mono faint">
                {list.length}{t(" 个服务器", " servers")}{reachable > 0 && t(` · ${formatCount(online)} 人在线`, ` · ${formatCount(online)} online`)}
              </span>
            )}
          </div>
          <span className="pane-hint">{t("和游戏里「多人游戏」是同一份列表。1.20 及以上直接进入服务器，更早的版本启动后自动连接。", "The same list as Multiplayer in game. 1.20 and later join directly; older versions connect after starting.")}</span>
        </div>
        <div className="spacer" />
        {list && list.length > 0 && (
          <Button size="sm" variant="ghost" icon="refresh" busy={refreshing} onClick={() => useServers.getState().ping(list.map((server) => server.address), true)}>
            {t("刷新状态", "Refresh status")}
          </Button>
        )}
        <Button size="sm" icon="plus" disabled={busy || draft != null} onClick={() => setDraft({ index: null, name: "", address: "" })}>
          {t("添加服务器", "Add server")}
        </Button>
      </div>

      <AnimatePresence initial={false}>
        {draft?.index == null && draft && (
          <motion.div key="new" initial={{ opacity: 0, height: 0 }} animate={{ opacity: 1, height: "auto", transition: normal }} exit={{ opacity: 0, height: 0, transition: { duration: 0.12 } }} style={{ overflow: "hidden", flex: "none" }}>
            {form("new")}
          </motion.div>
        )}
      </AnimatePresence>

      {list == null ? (
        <div className="saves-empty muted">
          <span className="spinner" />{t(" 正在读取服务器列表…", " Loading the server list…")}
        </div>
      ) : list.length === 0 ? (
        !draft && (
          <Empty
            art={
              <span className="empty-pixel">
                <Icon name="signal" size={48} />
              </span>
            }
            title={t("还没有服务器", "No servers yet")}
            actions={
              <Button variant="primary" icon="plus" disabled={busy} onClick={() => setDraft({ index: null, name: "", address: "" })}>
                {t("添加服务器", "Add server")}
              </Button>
            }
          >
            {t("填上朋友或社区服务器的地址，就能在这里看到在线人数和延迟，并一键加入。", "Add a friend's or community server to see players and ping here and join in one click.")}
          </Empty>
        )
      ) : (
        <div className="server-list">
          {list.map((server, i) =>
            draft?.index === server.index ? (
              form(`edit-${server.index}`)
            ) : (
              <motion.div key={`${server.index}:${server.address}`} layout="position" initial={{ opacity: 0, y: 6 }} animate={{ opacity: 1, y: 0, transition: { ...normal, delay: Math.min(i, 8) * 0.03 } }}>
                <ServerCard
                  instance={instance}
                  server={server}
                  actions={
                    <div className="server-actions">
                      {confirm === server.index ? (
                        <>
                          <Button size="sm" variant="hero" onClick={() => setConfirm(null)}>
                            {t("取消", "Cancel")}
                          </Button>
                          <Button
                            size="sm"
                            variant="danger"
                            icon="trash"
                            busy={saving}
                            onClick={async () => {
                              if (await apply({ kind: "remove", index: server.index, expected: server.address }, t("已从列表删除", "Removed from the list"))) setConfirm(null);
                            }}
                          >
                            {t("删除", "Delete")}
                          </Button>
                        </>
                      ) : (
                        <>
                          <Button size="sm" variant="hero" icon="chevronUp" aria-label={t("上移", "Move up")} title={t("在列表中上移", "Move up in the list")} disabled={busy || i === 0 || saving} onClick={() => void apply({ kind: "raise", index: server.index, expected: server.address })} />
                          <Button size="sm" variant="hero" icon="edit" aria-label={t("编辑", "Edit")} title={t("编辑名称和地址", "Edit name and address")} disabled={busy} onClick={() => setDraft({ index: server.index, name: server.name, address: server.address })} />
                          <Button size="sm" variant="hero" icon="trash" aria-label={t("删除", "Delete")} title={t("从列表删除", "Remove from list")} disabled={busy} onClick={() => setConfirm(server.index)} />
                          <JoinButton instance={instance} server={server} onJoin={s.closeModal} />
                        </>
                      )}
                    </div>
                  }
                />
              </motion.div>
            ),
          )}
        </div>
      )}
    </div>
  );
}

function ServerForm({ draft, saving, onChange, onSubmit, onCancel }: { draft: Draft; saving: boolean; onChange: (d: Draft) => void; onSubmit: () => void; onCancel: () => void }) {
  return (
    <form
      className="server-form"
      onSubmit={(e) => {
        e.preventDefault();
        onSubmit();
      }}
    >
      <label className="col gap-6" style={{ flex: "1 1 160px" }}>
        <span className="field-label">{t("名称", "Name")}</span>
        <Input autoFocus={draft.index == null} value={draft.name} placeholder={t("Minecraft 服务器", "Minecraft Server")} maxLength={64} onChange={(e) => onChange({ ...draft, name: e.target.value })} />
      </label>
      <label className="col gap-6" style={{ flex: "2 1 240px" }}>
        <span className="field-label">{t("地址", "Address")}</span>
        <Input mono autoFocus={draft.index != null} value={draft.address} placeholder={t("mc.example.com 或 1.2.3.4:25565", "mc.example.com or 1.2.3.4:25565")} spellCheck={false} onChange={(e) => onChange({ ...draft, address: e.target.value })} />
      </label>
      <div className="row server-form-actions">
        <Button variant="ghost" onClick={onCancel}>
          {t("取消", "Cancel")}
        </Button>
        <Button type="submit" variant="primary" icon="check" busy={saving} disabled={!draft.address.trim()}>
          {draft.index == null ? t("添加", "Add") : t("保存", "Save")}
        </Button>
      </div>
    </form>
  );
}
