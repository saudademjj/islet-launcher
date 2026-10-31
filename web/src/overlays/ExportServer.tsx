import { useState } from "react";

import { Modal } from "../components/overlay";
import { Button, Field, Input, Switch } from "../components/ui";
import { usePick } from "../store";
import { t } from "../lib/i18n";

const SUPPORTED = ["vanilla", "fabric", "quilt", "forge", "neoforge"];
const EULA = "https://aka.ms/MinecraftEULA";

/** A world as a dedicated server in an empty folder: the official server
 *  for its loader, the mods a server runs, configs and start scripts. */
export function ExportServer({ id }: { id: string }) {
  const s = usePick((s) => ({
    instance: s.instances.find((i) => i.id === id) ?? null,
    fileBusy: s.fileBusy,
    openModal: s.openModal,
    openUrl: s.openUrl,
    chooseFiles: s.chooseFiles,
    fileOperation: s.fileOperation,
    memory: s.settings.memoryAuto ? 4096 : s.settings.memoryMb,
  }));
  const [eula, setEula] = useState(false);
  const [memory, setMemory] = useState(String(s.instance?.memoryMb ?? s.memory));
  if (!s.instance) return null;
  const back = () => s.openModal({ kind: "manage", id, tab: "data" });
  const supported = SUPPORTED.includes(s.instance.loader);
  const notInstalled = s.instance.loader !== "vanilla" && !s.instance.loaderVersion;
  const memoryMb = Number(memory);
  const memoryOk = memoryMb >= 512 && memoryMb <= 32768;
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
            disabled={s.fileBusy || !supported || notInstalled || !memoryOk}
            onClick={async () => {
              const [destination] = await s.chooseFiles("serverDir");
              if (!destination) return;
              void s.fileOperation(id, { kind: "exportServer", destination, options: { acceptEula: eula, memoryMb } });
              back();
            }}
          >
            {t("选择空文件夹并生成", "Choose an empty folder")}
          </Button>
        </>
      }
    >
      <div className="col" style={{ gap: 16 }}>
        {!supported && <span className="field-hint bad">{t("这个加载器没有可以一键生成的服务端；原版、Fabric、Quilt、Forge 和 NeoForge 可以。", "This loader has no server Islet can set up; vanilla, Fabric, Quilt, Forge and NeoForge do.")}</span>}
        {supported && notInstalled && <span className="field-hint bad">{t("先安装一次这个世界，确定加载器版本后才能导出。", "Install this world once so its loader version is fixed, then export.")}</span>}
        <span className="field-hint">
          {t(
            "生成的文件夹里有官方服务端、服务器需要的模组和 config 等配置，以及 start.bat / start.sh。Modrinth 标明只在客户端运行的模组（小地图、光影类等）会自动去掉。",
            "The folder gets the official server, the mods a server needs, config folders, and start.bat / start.sh. Mods Modrinth marks as client-only (minimaps, shaders and the like) are left out.",
          )}
        </span>
        <Field label={t("服务端内存（MB）", "Server memory (MB)")} hint={memoryOk ? t("写进启动脚本，之后也可以改", "Goes into the start scripts; you can change it later") : t("填 512 到 32768 之间的数", "Enter a number from 512 to 32768")}>
          <Input mono inputMode="numeric" value={memory} style={{ width: 140 }} onChange={(e) => setMemory(e.target.value.replace(/\D/g, "").slice(0, 5))} />
        </Field>
        <div className="setting-row">
          <div className="col">
            <span className="setting-name">{t("我同意 Minecraft 最终用户许可协议（EULA）", "I agree to the Minecraft EULA")}</span>
            <span className="field-hint">
              {t("服务端要先同意才能运行；不勾选的话，首次启动后在 eula.txt 里改成 true。", "The server only runs once it's agreed to; otherwise set it to true in eula.txt after the first start.")}{" "}
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
