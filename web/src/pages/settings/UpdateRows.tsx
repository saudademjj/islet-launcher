import { useEffect, useState } from "react";

import { call, toError } from "../../api/backend";
import { Button, Switch } from "../../components/ui";
import { t } from "../../lib/i18n";
import { usePick } from "../../store";

/** Checking for a newer launcher. Off entirely when the build has no
    update source. */
export function UpdateRows() {
  const s = usePick((s) => ({ settings: s.settings, update: s.update, mutate: s.mutate, checkUpdate: s.checkUpdate, openUrl: s.openUrl, toast: s.toast }));
  const [available, setAvailable] = useState<boolean | null>(null);
  const [checking, setChecking] = useState(false);
  const [installing, setInstalling] = useState(false);
  useEffect(() => {
    let live = true;
    call<boolean>("update_source").then(
      (ok) => live && setAvailable(ok),
      () => live && setAvailable(false),
    );
    return () => {
      live = false;
    };
  }, []);
  if (available === false) {
    return <span className="field-hint">{t("这个版本没有配置更新地址，请到发布页查看新版本。", "This build has no update source; check the release page for new versions.")}</span>;
  }
  return (
    <>
      <div className="setting-row">
        <div className="col">
          <span className="setting-name">{t("启动时检查更新", "Check for updates at start")}</span>
          <span className="field-hint">{t("有新版本时在状态栏提醒，由你决定是否更新", "Notifies in the status bar when an update is available")}</span>
        </div>
        <Switch
          label={t("启动时检查更新", "Check for updates at start")}
          checked={s.settings.checkUpdates}
          onChange={(checkUpdates) => void s.mutate("update_settings", { settings: { ...s.settings, checkUpdates } })}
        />
      </div>
      <div className="row">
        {s.update ? (
          <>
            <span style={{ flex: 1 }}>
              {t("新版本 ", "New version ")}<span className="mono">{s.update.version}</span>
              {s.update.notes ? ` · ${s.update.notes}` : ""}
            </span>
            {s.update.package ? (
              <>
                <Button variant="ghost" icon="external" onClick={() => void s.openUrl(s.update!.url)}>
                  {t("更新说明", "Release notes")}
                </Button>
                <Button
                  variant="primary"
                  icon="download"
                  busy={installing}
                  onClick={async () => {
                    setInstalling(true);
                    try {
                      const message = await call<string>("install_update");
                      if (message) s.toast(message, "success");
                    } catch (error) {
                      s.toast(toError(error).message, "error");
                    }
                    setInstalling(false);
                  }}
                >
                  {installing ? t("正在下载…", "Downloading…") : t("下载并安装", "Download and install")}
                </Button>
              </>
            ) : (
              <Button variant="primary" icon="external" onClick={() => void s.openUrl(s.update!.url)}>
                {t("去下载", "Download")}
              </Button>
            )}
          </>
        ) : (
          <>
            <span className="muted" style={{ flex: 1 }}>
              {t("没有发现新版本", "You're up to date")}
            </span>
            <Button
              icon="refresh"
              busy={checking}
              onClick={async () => {
                setChecking(true);
                await s.checkUpdate();
                setChecking(false);
              }}
            >
              {t("检查更新", "Check for updates")}
            </Button>
          </>
        )}
      </div>
    </>
  );
}
