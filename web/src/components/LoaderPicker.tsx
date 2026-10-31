import { useEffect, useState } from "react";

import { loaderLabel } from "../lib/format";
import { usePick } from "../store";
import { Button, Chip, Segmented, Status } from "./ui";
import { t } from "../lib/i18n";

export const LOADERS = [
  { value: "vanilla", label: t("原版", "Vanilla") },
  { value: "fabric", label: "Fabric" },
  { value: "quilt", label: "Quilt" },
  { value: "forge", label: "Forge" },
  { value: "neoforge", label: "NeoForge" },
  { value: "legacyfabric", label: "Legacy Fabric" },
  { value: "liteloader", label: "LiteLoader" },
  { value: "cleanroom", label: "Cleanroom" },
];

/** Loaders for old versions or niche setups; they wait behind "More" so a
    first-timer chooses between the common ones. */
const NICHE = new Set(["legacyfabric", "liteloader", "cleanroom"]);

/** Vanilla needs nothing; a loader needs its list loaded and, when locked,
    a version that is still in that list. */
export function useLoaderSelectionValid(loader: string, version: string | null): boolean {
  const state = usePick((s) => ({ loading: s.loader.loading, error: s.loader.error, versions: s.loader.versions }));
  if (loader === "vanilla") return true;
  if (state.loading || state.error || state.versions.length === 0) return false;
  return version == null || state.versions.some((v) => v.version === version);
}

export function LoaderPicker({
  loader,
  version,
  minecraft,
  onLoader,
  onVersion,
  hint,
  disabled,
}: {
  loader: string;
  version: string | null;
  minecraft: string | null;
  onLoader: (loader: string) => void;
  onVersion: (version: string | null) => void;
  hint?: string;
  disabled?: boolean;
}) {
  const s = usePick((s) => ({ state: s.loader, refresh: s.refreshLoaderVersions }));
  const [more, setMore] = useState(false);
  const all = more || NICHE.has(loader);

  useEffect(() => {
    s.refresh(loader, minecraft);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loader, minecraft]);

  return (
    <div className="col loader-picker gap-8">
      <div className="row gap-8 wrap">
        <Segmented
          value={loader}
          disabled={disabled}
          options={all ? LOADERS : LOADERS.filter((option) => !NICHE.has(option.value))}
          onChange={(next) => {
            onLoader(next);
            onVersion(null);
          }}
        />
        {!all && (
          <Button size="sm" variant="ghost" disabled={disabled} title="Legacy Fabric · LiteLoader · Cleanroom" onClick={() => setMore(true)}>
            {t("更多加载器", "More loaders")}
          </Button>
        )}
      </div>
      <span className="field-hint">{loader === "vanilla" ? t("不加载模组的官方原版", "The official game, without mods") : hint ?? t("按游戏版本列出兼容加载器；可锁定具体版本", "Loaders that fit the game version; you can pin one")}</span>
      {loader !== "vanilla" && (
        <div className="col gap-6">
          <div className="row">
            <span className="field-label">{loaderLabel(loader)}{t(" 版本", " version")}</span>
            <span className="mono tone-accent" style={{ fontSize: "var(--text-xs)" }}>
              {version ?? t("自动", "Auto")}
            </span>
            <div className="spacer" />
            <Button size="sm" variant="ghost" icon="refresh" busy={s.state.loading} onClick={() => s.refresh(loader, minecraft)} aria-label={t("刷新", "Refresh")} />
          </div>
          <div className="picker" style={{ maxHeight: 168 }}>
            {s.state.loading ? (
              <div className="picker-row faint">{t("正在获取兼容版本…", "Loading compatible versions…")}</div>
            ) : s.state.error ? (
              <div className="picker-row">
                <Status tone="danger">{t("获取加载器版本失败", "Couldn't load loader versions")}</Status>
                <span className="faint truncate">{s.state.error}</span>
              </div>
            ) : s.state.versions.length === 0 ? (
              <div className="picker-row faint">{minecraft ? t("这个 Minecraft 版本没有可用的加载器，请换一个游戏版本。", "No loader available for this Minecraft version; choose another game version.") : t("先选择 Minecraft 版本", "Choose a Minecraft version first")}</div>
            ) : (
              <>
                <button type="button" className="picker-row" aria-selected={version == null} disabled={disabled} onClick={() => onVersion(null)}>
                  <span style={{ flex: 1 }}>{t("自动选择推荐版本", "Recommended (automatic)")}</span>
                  <span className="faint" style={{ fontSize: "var(--text-xs)" }}>
                    {t("安装后锁定", "Pinned after install")}
                  </span>
                </button>
                {s.state.versions.map((v) => (
                  <button key={v.version} type="button" className="picker-row" aria-selected={version === v.version} disabled={disabled} onClick={() => onVersion(v.version)}>
                    <span className="mono" style={{ flex: 1 }}>
                      {v.version}
                    </span>
                    {!v.stable && <Chip>{t("测试版", "Beta")}</Chip>}
                  </button>
                ))}
              </>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
