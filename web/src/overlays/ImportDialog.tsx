import { useEffect, useState } from "react";

import { call } from "../api/backend";
import type { ImportCandidate } from "../api/types";
import { Icon } from "../components/Icon";
import { Modal } from "../components/overlay";
import { Button, Empty } from "../components/ui";
import { loaderLabel } from "../lib/format";
import { usePick } from "../store";
import { t } from "../lib/i18n";

const KIND_LABEL: Record<string, string> = { official: t("官方启动器", "Official launcher"), isolated: t("版本隔离", "Isolated version"), prism: "Prism / MultiMC" };

/** Bringing games in: a modpack file, or games another launcher already has
    (the official launcher, HMCL, PCL, Prism). Nothing is moved; saves, mods
    and settings are copied into a new world. */
export function ImportDialog() {
  const s = usePick((s) => ({ fileBusy: s.fileBusy, filesChanged: s.filesChanged, lastOk: s.lastOperationOk, closeModal: s.closeModal, chooseFiles: s.chooseFiles, fileOperation: s.fileOperation }));
  const [known, setKnown] = useState<[string, string][]>([]);
  const [folder, setFolder] = useState<string | null>(null);
  const [found, setFound] = useState<ImportCandidate[] | null>(null);
  const [scanning, setScanning] = useState(false);
  const [imported, setImported] = useState<string[]>([]);
  const [pending, setPending] = useState<string | null>(null);

  useEffect(() => {
    call<[string, string][]>("known_game_dirs").then(setKnown, () => setKnown([]));
  }, []);

  // An import finished: mark it done (a failure was already shown).
  useEffect(() => {
    if (!pending) return;
    if (s.lastOk) setImported((list) => [...list, pending]);
    setPending(null);
  }, [s.filesChanged]);

  const scan = async (path: string) => {
    setFolder(path);
    setScanning(true);
    setFound(null);
    try {
      setFound(await call<ImportCandidate[]>("scan_games", { path }));
    } catch {
      setFound([]);
    }
    setScanning(false);
  };

  return (
    <Modal title={t("导入", "Import")} subtitle={t("整合包，或者其他启动器里已有的游戏；原来的文件不会被改动", "A modpack, or games you already have in other launchers; the original files are left untouched")} onClose={s.closeModal}>
      <div className="import-dialog">
        <section className="import-section">
          <div className="section-label">{t("整合包", "Modpack")}</div>
          <div className="row">
            <span className="muted" style={{ flex: 1 }}>
              {t("Modrinth 的 .mrpack，或 CurseForge 的整合包 .zip（需要在设置里填 CurseForge API Key）。", "A Modrinth .mrpack, or a CurseForge modpack .zip (needs a CurseForge API key in Settings).")}
            </span>
            <Button
              icon="archive"
              disabled={s.fileBusy}
              onClick={async () => {
                const [path] = await s.chooseFiles("pack");
                if (path) {
                  void s.fileOperation(null, { kind: "importPack", path });
                  s.closeModal();
                }
              }}
            >
              {t("选择整合包", "Choose modpack")}
            </Button>
          </div>
        </section>

        <section className="import-section">
          <div className="section-label">{t("其他启动器的游戏", "Games from other launchers")}</div>
          <div className="row" style={{ flexWrap: "wrap" }}>
            {known.map(([label, path]) => (
              <button key={path} type="button" className="filter-chip" aria-pressed={folder === path} title={path} onClick={() => void scan(path)}>
                {label}
              </button>
            ))}
            <Button
              size="sm"
              icon="folder"
              onClick={async () => {
                const [path] = await s.chooseFiles("gameDir");
                if (path) void scan(path);
              }}
            >
              {t("选择文件夹…", "Choose folder…")}
            </Button>
          </div>
          <span className="field-hint">{t("可以选 .minecraft、其中某个版本的文件夹、HMCL / PCL 的游戏目录，或 Prism 的 instances 文件夹。", "Pick .minecraft, one of its version folders, an HMCL / PCL game folder, or Prism's instances folder.")}</span>
          {folder && (
            <div className="mono faint truncate" style={{ fontSize: "var(--text-xs)" }} title={folder}>
              {folder}
            </div>
          )}
          {scanning ? (
            <div className="saves-empty muted">
              <span className="spinner" />{t(" 正在查找游戏…", " Looking for games…")}
            </div>
          ) : found == null ? null : found.length === 0 ? (
            <Empty icon="search" title={t("这里没有找到游戏版本", "No game versions found here")}>
              {t("试试选 .minecraft 文件夹本身，或者它里面 versions 下的某个版本。", "Try the .minecraft folder itself, or a version inside its versions folder.")}
            </Empty>
          ) : (
            <div className="list">
              {found.map((game) => {
                const done = imported.includes(game.source);
                return (
                  <div key={game.source} className="list-row">
                    <Icon name="block" />
                    <div className="col fill gap-2">
                      <span className="truncate" style={{ fontWeight: 650 }}>
                        {game.name}
                      </span>
                      <span className="faint truncate" style={{ fontSize: "var(--text-xs)" }}>
                        <span className="mono">{game.versionId}</span> · {loaderLabel(game.loader)}
                        {game.loaderVersion ? ` ${game.loaderVersion}` : ""} · {KIND_LABEL[game.kind] ?? game.kind} · {game.saves}{t(" 个存档", " saves")}
                        {game.loader !== "vanilla" ? t(` · ${game.mods} 个模组`, ` · ${game.mods} mods`) : ""}
                      </span>
                    </div>
                    {done ? (
                      <span className="pill-accent">
                        <Icon name="check" />{t(" 已导入", " Imported")}
                      </span>
                    ) : (
                      <Button
                        size="sm"
                        variant="go"
                        icon="download"
                        busy={pending === game.source}
                        disabled={s.fileBusy}
                        onClick={() => {
                          setPending(game.source);
                          void s.fileOperation(null, { kind: "importGame", source: game.source });
                        }}
                      >
                        {t("导入", "Import")}
                      </Button>
                    )}
                  </div>
                );
              })}
            </div>
          )}
          {found && found.some((g) => g.kind === "official") && (
            <span className="field-hint">{t("官方启动器的各个版本共用一份存档，导入多个版本会各自复制一份。", "The official launcher's versions share one set of saves; importing several copies it into each.")}</span>
          )}
        </section>
      </div>
    </Modal>
  );
}
