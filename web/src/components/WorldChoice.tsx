import type { Instance } from "../api/types";
import { formatBytes } from "../lib/format";
import { t } from "../lib/i18n";
import { useWorlds } from "../lib/worlds";
import { Identicon } from "./ui";

/** Which save a server starts from: one of the world's, or a fresh one the
    server generates. */
export function WorldChoice({ instance, value, onChange }: { instance: Instance; value: string | null; onChange: (folder: string | null) => void }) {
  const list = useWorlds(instance);
  const rows: { folder: string | null; name: string; meta: string; icon: string | null }[] = [
    { folder: null, name: t("新世界", "A new world"), meta: t("服务器第一次启动时自己生成", "The server makes one when it first starts"), icon: null },
    ...(list?.worlds ?? []).map((w) => ({ folder: w.folder, name: w.name ?? w.folder, meta: formatBytes(w.bytes), icon: w.icon })),
  ];
  return (
    <div className="list" role="radiogroup" aria-label={t("服务器的世界", "The server's world")} style={{ maxHeight: 228, overflowY: "auto" }}>
      {rows.map((row) => (
        <div
          key={row.folder ?? ""}
          className={`list-row clickable${value === row.folder ? " selected" : ""}`}
          role="radio"
          aria-checked={value === row.folder}
          tabIndex={0}
          onClick={() => onChange(row.folder)}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === " ") {
              e.preventDefault();
              onChange(row.folder);
            }
          }}
        >
          {row.icon ? <img src={row.icon} alt="" width={28} height={28} style={{ imageRendering: "pixelated" }} /> : <Identicon seed={`${instance.id}:${row.folder ?? "new"}`} size={28} />}
          <span className="col gap-2 fill">
            <span className="truncate setting-name">{row.name}</span>
            <span className="field-hint">{row.meta}</span>
          </span>
          {value === row.folder && <span className="field-hint ok">{t("已选", "Chosen")}</span>}
        </div>
      ))}
      {list === null && <div className="list-row field-hint">{t("正在读取存档…", "Loading saves…")}</div>}
    </div>
  );
}
