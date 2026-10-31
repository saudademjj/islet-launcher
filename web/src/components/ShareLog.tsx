import { useState } from "react";

import { call, toError } from "../api/backend";
import { useStore } from "../store";
import { Button } from "./ui";
import { t } from "../lib/i18n";

/** Uploads the game log to mclo.gs and copies the link, for asking for
    help. The first click explains; the second sends, since the link is
    public. Home-folder paths and tokens are masked before sending. */
export function ShareLogButton({ instanceId }: { instanceId: string }) {
  const [confirm, setConfirm] = useState(false);
  const [busy, setBusy] = useState(false);
  return (
    <Button
      size="sm"
      variant={confirm ? "primary" : "default"}
      icon="external"
      busy={busy}
      title={t("上传到 mclo.gs 并复制链接，方便发给别人帮忙看；用户目录和登录令牌会被隐去", "Uploads to mclo.gs and copies the link to share with helpers; your home folder and sign-in tokens are hidden")}
      onBlur={() => setConfirm(false)}
      onClick={async () => {
        if (!confirm) return setConfirm(true);
        setConfirm(false);
        setBusy(true);
        try {
          const url = await call<string>("upload_log", { id: instanceId });
          await useStore.getState().copy(url, t("日志链接", "Log link"));
        } catch (error) {
          useStore.getState().toast(toError(error).message, "error");
        }
        setBusy(false);
      }}
    >
      {confirm ? t("确认上传（公开链接）", "Confirm upload (public link)") : t("上传日志求助", "Upload log for help")}
    </Button>
  );
}
