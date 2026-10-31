import { useState } from "react";

import { Modal } from "../components/overlay";
import { Button, Field, Input } from "../components/ui";
import { t } from "../lib/i18n";
import { usePick } from "../store";

/** Every group in use, in name order. */
export function worldGroups(instances: { group: string | null }[]): string[] {
  return [...new Set(instances.map((i) => i.group).filter((g): g is string => !!g))].sort((a, b) => a.localeCompare(b));
}

/** Puts one world in a group: pick one in use, type a new one, or none. */
export function GroupDialog({ id }: { id: string }) {
  const s = usePick((s) => ({ instances: s.instances, close: s.closeModal, organize: s.organize }));
  const instance = s.instances.find((i) => i.id === id);
  const [group, setGroup] = useState(instance?.group ?? "");
  const [busy, setBusy] = useState(false);
  if (!instance) return null;
  const groups = worldGroups(s.instances);
  const name = group.trim();
  const save = async () => {
    setBusy(true);
    const ok = await s.organize(id, instance.favorite, name || null);
    setBusy(false);
    if (ok) s.close();
  };
  return (
    <Modal
      title={t("分组", "Group")}
      subtitle={instance.name}
      onClose={s.close}
      footer={
        <>
          <Button variant="ghost" onClick={s.close}>
            {t("取消", "Cancel")}
          </Button>
          <Button variant="primary" busy={busy} disabled={name === (instance.group ?? "")} onClick={() => void save()}>
            {t("保存", "Save")}
          </Button>
        </>
      }
    >
      <div className="col gap-16">
        {groups.length > 0 && (
          <div className="filter-chips" role="group" aria-label={t("已有分组", "Groups in use")}>
            <button type="button" className="filter-chip" aria-pressed={!name} onClick={() => setGroup("")}>
              {t("不分组", "No group")}
            </button>
            {groups.map((g) => (
              <button key={g} type="button" className="filter-chip" aria-pressed={name === g} onClick={() => setGroup(g)}>
                {g}
              </button>
            ))}
          </div>
        )}
        <Field label={t("分组名", "Group name")} hint={t("输入新名字即可新建；留空表示不分组。", "Type a new name to start a group; leave empty for none.")}>
          <Input value={group} maxLength={24} placeholder={t("例如 生存、整合包、测试", "e.g. Survival, Modpacks, Testing")} onChange={(e) => setGroup(e.target.value)} onKeyDown={(e) => e.key === "Enter" && name !== (instance.group ?? "") && void save()} />
        </Field>
      </div>
    </Modal>
  );
}
