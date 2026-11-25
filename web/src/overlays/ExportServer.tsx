import { useState } from "react";

import { Modal } from "../components/overlay";
import { WorldChoice } from "../components/WorldChoice";
import { Button, Field, Input, Switch } from "../components/ui";
import { findInstance, usePick } from "../store";
import { t } from "../lib/i18n";

const SUPPORTED = ["vanilla", "fabric", "quilt", "forge", "neoforge", "paper", "purpur"];
const EULA = "https://aka.ms/MinecraftEULA";

/** A world as a dedicated server in an empty folder: the official server
 *  for its loader, the mods a server runs, configs and start scripts. A
 *  server project brings its own settings and gets the deploy kit too. */
export function ExportServer({ id }: { id: string }) {
  const s = usePick((s) => ({
    instance: findInstance(s, id),
    fileBusy: s.fileBusy,
    openModal: s.openModal,
    closeModal: s.closeModal,
    openUrl: s.openUrl,
    chooseFiles: s.chooseFiles,
    fileOperation: s.fileOperation,
    memory: s.settings.memoryAuto ? 4096 : s.settings.memoryMb,
  }));
  const [eula, setEula] = useState(false);
  const [memory, setMemory] = useState(String(s.instance?.memoryMb ?? s.memory));
  const [world, setWorld] = useState<string | null>(null);
  const [port, setPort] = useState("25565");
  const [onlineMode, setOnlineMode] = useState(true);
  const [clientPack, setClientPack] = useState(true);
  const [address, setAddress] = useState("");
  if (!s.instance) return null;
  const project = s.instance.server;
  const back = project ? s.closeModal : () => s.openModal({ kind: "manage", id, tab: "tools" });
  const supported = SUPPORTED.includes(s.instance.loader);
  const plugins = s.instance.loader === "paper" || s.instance.loader === "purpur";
  // A project picks its loader version on the first export.
  const notInstalled = !project && s.instance.loader !== "vanilla" && !s.instance.loaderVersion;
  // Friends need the pack only for a modded server.
  const offerPack = !plugins && s.instance.loader !== "vanilla";
  const memoryMb = Number(memory);
  const memoryOk = !!project || (memoryMb >= 512 && memoryMb <= 32768);
  const portOk = !!project || (/^\d{1,5}$/.test(port) && Number(port) > 0 && Number(port) < 65536);
  return (
    <Modal
      title={t("导出为服务端", "Export as server")}
      subtitle={`${s.instance.name} · ${s.instance.versionId}`}
      onClose={back}
      footer={
        <>
          <Button variant="ghost" onClick={back}>
            {t("取消", "Cancel")}
          </Button>
          <Button
            variant="primary"
            icon="folder"
            disabled={s.fileBusy || !supported || notInstalled || !memoryOk || !portOk}
            onClick={async () => {
              const [destination] = await s.chooseFiles("serverDir");
              if (!destination) return;
              const options = project
                ? { acceptEula: eula, memoryMb: project.memoryMb, world: null, port: project.port, onlineMode: project.onlineMode, clientPack: offerPack && clientPack, address: address.trim() }
                : { acceptEula: eula, memoryMb, world, port: Number(port), onlineMode, clientPack, address: address.trim() };
              void s.fileOperation(id, { kind: "exportServer", destination, options });
              back();
            }}
          >
            {t("选择空文件夹并生成", "Choose an empty folder")}
          </Button>
        </>
      }
    >
      <div className="col gap-16">
        {!supported && <span className="field-hint bad">{t("仅支持原版、Fabric、Quilt、Forge、NeoForge、Paper 和 Purpur。", "Supported for Vanilla, Fabric, Quilt, Forge, NeoForge, Paper, and Purpur.")}</span>}
        {supported && notInstalled && <span className="field-hint bad">{t("先安装一次这个世界，确定加载器版本后才能导出。", "Install this world once so its loader version is fixed, then export.")}</span>}
        <span className="field-hint">
          {project
            ? plugins
              ? t(
                  "生成服务端、插件和启动脚本；上传到 Linux 后运行 sh islet.sh setup 即可开服。",
                  "Generates the server, plugins, and start scripts; run sh islet.sh setup on Linux to launch.",
                )
              : t(
                  "生成服务端、模组和启动脚本；上传到 Linux 后运行 sh islet.sh setup 即可开服。",
                  "Generates the server, mods, and start scripts; run sh islet.sh setup on Linux to launch.",
                )
            : t(
                "生成服务端、模组、配置和启动脚本，自动排除仅客户端模组。",
                "Generates the server, mods, configs, and start scripts, excluding client-only mods.",
              )}
        </span>
        {!project && (
          <>
            <Field label={t("服务器的世界", "The server's world")} hint={world ? t("复制该存档作为服务器世界，保留你的背包和位置", "Copies this save as the server world, keeping your inventory and spot") : undefined}>
              <WorldChoice instance={s.instance} value={world} onChange={setWorld} />
            </Field>
            <div className="row gap-16 items-start wrap">
              <Field label={t("端口", "Port")} tone={portOk ? undefined : "bad"} hint={portOk ? t("默认 25565", "25565 by default") : t("填 1 到 65535", "Enter 1 to 65535")}>
                <Input mono inputMode="numeric" value={port} style={{ width: 110 }} invalid={!portOk} onChange={(e) => setPort(e.target.value.replace(/\D/g, "").slice(0, 5))} />
              </Field>
              <Field label={t("服务端内存（MB）", "Server memory (MB)")} hint={memoryOk ? t("写进启动脚本，之后也可以改", "Goes into the start scripts; you can change it later") : t("填 512 到 32768 之间的数", "Enter a number from 512 to 32768")}>
                <Input mono inputMode="numeric" value={memory} style={{ width: 140 }} onChange={(e) => setMemory(e.target.value.replace(/\D/g, "").slice(0, 5))} />
              </Field>
            </div>
            <div className="setting-row">
              <div className="col">
                <span className="setting-name">{t("正版验证", "Online mode")}</span>
                <span className="field-hint">{t("关闭后离线账号可加入，但名称无验证", "Off lets offline accounts join without name checks")}</span>
              </div>
              <Switch label={t("正版验证", "Online mode")} checked={onlineMode} onChange={setOnlineMode} />
            </div>
          </>
        )}
        {(!project || offerPack) && (
          <div className="setting-row">
            <div className="col">
              <span className="setting-name">{t("同时生成好友用的整合包", "Also make a pack for friends")}</span>
              <span className="field-hint">{t("好友导入即可同步模组和服务器地址，进服缺模组可自动补齐", "Syncs mods and server address for friends, with automatic missing-mod install")}</span>
            </div>
            <Switch label={t("生成好友用的整合包", "Make a pack for friends")} checked={clientPack} onChange={setClientPack} />
          </div>
        )}
        {(!project || offerPack) && clientPack && (
          <Field label={t("好友连接的地址（可不填）", "Address friends join (optional)")} hint={t("比如 mc.example.com 或 1.2.3.4:25565；写进好友的服务器列表", "Like mc.example.com or 1.2.3.4:25565; goes into friends' server list")}>
            <Input mono value={address} placeholder="mc.example.com" onChange={(e) => setAddress(e.target.value.slice(0, 255))} />
          </Field>
        )}
        <div className="setting-row">
          <div className="col">
            <span className="setting-name">{t("我同意 Minecraft 最终用户许可协议（EULA）", "I agree to the Minecraft EULA")}</span>
            <span className="field-hint">
              {t("需同意协议后服务端才能启动。", "Required before the server can start.")}{" "}
              <button type="button" className="link" onClick={() => void s.openUrl(EULA)}>
                {t("阅读 EULA", "Read the EULA")}
              </button>
            </span>
          </div>
          <Switch label={t("同意 EULA", "Agree to the EULA")} checked={eula} onChange={setEula} />
        </div>
      </div>
    </Modal>
  );
}
