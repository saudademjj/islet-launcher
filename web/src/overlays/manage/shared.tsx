import type { Instance, LocalMod } from "../../api/types";
import { Button, Notice } from "../../components/ui";
import { loaderLabel } from "../../lib/format";
import { t } from "../../lib/i18n";
import { usePick, useStore } from "../../store";

/** Paper and Purpur take plugins where other servers take mods. */
export const isPluginServer = (loader: string) => loader === "paper" || loader === "purpur";

export function NeedsLoader() {
  return (
    <Notice tone="warn" title={t("原版不会加载模组", "Vanilla doesn't load mods")}>
      {t("请先在「常规」中选择并保存 Fabric、Quilt、Forge 或 NeoForge。", "Choose and save Fabric, Quilt, Forge or NeoForge under General first.")}
    </Notice>
  );
}

/** The running download or file operation, with a way to cancel it. */
export function OperationLine() {
  const s = usePick((s) => ({ task: s.operationTask, busy: s.fileBusy }));
  if (!s.task || !s.busy) return null;
  return (
    <div className="row muted" style={{ fontSize: "var(--text-sm)" }}>
      <span className="spinner" />{t(" 正在下载文件…", " Downloading files…")}
      <div className="spacer" />
      <Button size="sm" variant="danger-ghost" onClick={() => void useStore.getState().run("cancel_task", { taskId: s.task })}>
        {t("取消", "Cancel")}
      </Button>
    </div>
  );
}

/** Whether this instance can load shader packs, and what is missing if not. */
export function shaderSupport(instance: Instance, mods: LocalMod[]): { ok: boolean; text: string; need?: { slug: string; name: string } } {
  const state = (...names: string[]) => {
    const found = mods.filter((mod) => names.some((name) => mod.name.toLowerCase().includes(name)));
    return found.some((mod) => mod.enabled) ? "on" : found.length > 0 ? "off" : null;
  };
  const ready = t("进游戏后在「选项 → 视频设置 → 光影包」启用。", "Enable in game under Options → Video Settings → Shader Packs.");
  if (instance.loader === "vanilla") return { ok: false, text: t("原版不支持光影，请切换 Fabric + Iris 或 Forge + Oculus。", "Vanilla doesn't support shaders; use Fabric + Iris or Forge + Oculus.") };
  const [names, need] =
    instance.loader === "fabric" || instance.loader === "quilt"
      ? [["iris"], { slug: "iris", name: "Iris" }]
      : instance.loader === "neoforge"
        ? [["iris", "oculus"], { slug: "iris", name: "Iris" }]
        : [["oculus", "optifine"], { slug: "oculus", name: "Oculus" }];
  const loaded = state(...names);
  if (loaded === "on") return { ok: true, text: t(`已安装 ${need.name}，${ready}`, `${need.name} is installed. ${ready}`) };
  if (loaded === "off") return { ok: false, text: t(`${need.name} 已禁用，请先在「模组」页启用。`, `${need.name} is disabled; enable it in Mods first.`) };
  return { ok: false, text: t(`${loaderLabel(instance.loader)} 需先安装 ${need.name} 模组才能使用光影。`, `${loaderLabel(instance.loader)} requires ${need.name} to load shaders.`), need };
}

/** A pack name to compare: colour codes, case and punctuation dropped. */
export function packKey(name: string): string {
  return name
    .replace(/§./g, "")
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, "");
}
