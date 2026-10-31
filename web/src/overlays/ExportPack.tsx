import { useState } from "react";

import type { ExportOptions } from "../api/types";
import { Modal } from "../components/overlay";
import { Button, Field, Input, Segmented, Switch } from "../components/ui";
import { usePick } from "../store";
import { t } from "../lib/i18n";

/** A world as a Modrinth or CurseForge pack: name it, choose what rides
 *  along, save it. */
export function ExportPack({ id }: { id: string }) {
  const s = usePick((s) => ({
    instance: s.instances.find((i) => i.id === id) ?? null,
    fileBusy: s.fileBusy,
    openModal: s.openModal,
    chooseSavePath: s.chooseSavePath,
    fileOperation: s.fileOperation,
    curseforgeKey: s.settings.curseforgeKey,
  }));
  const [options, setOptions] = useState<ExportOptions>({
    name: s.instance?.name ?? "",
    version: "1.0.0",
    summary: "",
    format: "mrpack",
    options: false,
    servers: false,
    saves: false,
  });
  if (!s.instance) return null;
  const set = (patch: Partial<ExportOptions>) => setOptions({ ...options, ...patch });
  const back = () => s.openModal({ kind: "manage", id, tab: "data" });
  const cf = options.format === "curseforge";
  const extension = cf ? "zip" : "mrpack";
  const notInstalled = s.instance.loader !== "vanilla" && !s.instance.loaderVersion;
  const toggles: { key: "options" | "servers" | "saves"; label: string; hint: string }[] = [
    { key: "options", label: t("游戏设置", "Game settings"), hint: t("options.txt：按键、画质、语言等", "options.txt: keys, graphics, language and more") },
    { key: "servers", label: t("服务器列表", "Server list"), hint: "servers.dat" },
    { key: "saves", label: t("存档", "Saves"), hint: t("整包会变大；分享地图时才需要", "Makes the pack bigger; only for sharing maps") },
  ];
  return (
    <Modal
      title={t("导出为整合包", "Export as modpack")}
      subtitle={`${s.instance.name} · ${s.instance.versionId}`}
      onClose={back}
      footer={
        <>
          <Button variant="ghost" onClick={back}>
            {t("取消", "Cancel")}
          </Button>
          <Button
            variant="primary"
            icon="external"
            disabled={s.fileBusy || notInstalled || !options.name.trim()}
            onClick={async () => {
              const base = `${options.name.trim()}-${options.version.trim() || "1.0.0"}`.replace(/[\\/:*?"<>|]/g, "_");
              const destination = await s.chooseSavePath(t("导出整合包", "Export modpack"), `${base}.${extension}`, extension);
              if (!destination) return;
              void s.fileOperation(id, { kind: "exportPack", destination, options });
              back();
            }}
          >
            {t(`导出 .${extension}`, `Export .${extension}`)}
          </Button>
        </>
      }
    >
      <div className="col" style={{ gap: 16 }}>
        {notInstalled && <span className="field-hint bad">{t("先安装一次这个世界，确定加载器版本后才能导出。", "Install this world once so its loader version is fixed, then export.")}</span>}
        <Field
          label={t("格式", "Format")}
          hint={
            cf
              ? s.curseforgeKey
                ? t("CurseForge 上有的模组只写链接，其余直接打包。", "Mods hosted on CurseForge are linked; the rest are packed in.")
                : t("没有填写 CurseForge API Key，所有模组都会直接打包，文件会更大。", "No CurseForge API key set, so every mod is packed in and the file is bigger.")
              : t("Modrinth 上有的模组只写链接，其余直接打包。", "Mods hosted on Modrinth are linked; the rest are packed in.")
          }
        >
          <Segmented value={options.format} onChange={(format) => set({ format })} options={[{ value: "mrpack" as const, label: "Modrinth .mrpack" }, { value: "curseforge" as const, label: "CurseForge .zip" }]} />
        </Field>
        <div className="manage-grid">
          <Field label={t("整合包名称", "Modpack name")}>
            <Input value={options.name} maxLength={64} onChange={(e) => set({ name: e.target.value })} />
          </Field>
          <Field label={t("版本号", "Version")}>
            <Input mono value={options.version} maxLength={32} onChange={(e) => set({ version: e.target.value })} />
          </Field>
        </div>
        <Field label={t("简介", "Summary")} hint={t("可选，显示在导入它的启动器里", "Optional; shown in the launcher that imports it")}>
          <Input value={options.summary} maxLength={200} onChange={(e) => set({ summary: e.target.value })} />
        </Field>
        <div className="col" style={{ gap: 10 }}>
          <span className="field-label">{t("一起打包", "Also include")}</span>
          <span className="field-hint">{t("模组、资源包、光影和 config 等配置文件夹总是包含在内。", "Mods, resource packs, shaders and config folders are always included.")}</span>
          {toggles.map((toggle) => (
            <div key={toggle.key} className="setting-row">
              <div className="col">
                <span className="setting-name">{toggle.label}</span>
                <span className="field-hint">{toggle.hint}</span>
              </div>
              <Switch label={toggle.label} checked={options[toggle.key]} onChange={(on) => set({ [toggle.key]: on })} />
            </div>
          ))}
        </div>
      </div>
    </Modal>
  );
}
