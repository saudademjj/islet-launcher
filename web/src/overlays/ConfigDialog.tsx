import { useEffect, useMemo, useRef, useState } from "react";

import { call, toError } from "../api/backend";
import type { ConfigFile } from "../api/types";
import { Modal } from "../components/overlay";
import { Button, Input } from "../components/ui";
import { formatBytes, formatRelative } from "../lib/format";
import { t } from "../lib/i18n";
import { usePick } from "../store";

/** A world's mod config files as plain text: pick one on the left, edit it
    on the right. The previous text is kept by the launcher on every save. */
export function ConfigDialog({ id, initial }: { id: string; initial?: string }) {
  const s = usePick((s) => ({
    instance: s.instances.find((i) => i.id === id) ?? null,
    running: s.runningIds.includes(id),
    openModal: s.openModal,
    toast: s.toast,
  }));
  const [files, setFiles] = useState<ConfigFile[] | null>(null);
  const [query, setQuery] = useState(initial ?? "");
  const [path, setPath] = useState<string | null>(null);
  const [text, setText] = useState<string | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  // Where to go once unsaved changes are dealt with: a file, or "" to close.
  const [pending, setPending] = useState<string | null>(null);
  const readSeq = useRef(0);
  const mounted = useRef(true);
  const pathRef = useRef(path);
  pathRef.current = path;
  const dirty = text != null && text !== saved;
  const leave = () => s.openModal({ kind: "manage", id, tab: "mods" });
  const back = () => (dirty ? setPending("") : leave());

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  // Intercept Esc while edits are unsaved so the global shortcut cannot drop them.
  useEffect(() => {
    if (!dirty && pending == null) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.isComposing || e.keyCode === 229) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      if (pending != null) setPending(null);
      else setPending("");
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [dirty, pending]);

  useEffect(() => {
    let live = true;
    call<ConfigFile[]>("list_configs", { id })
      .then((list) => live && setFiles(list))
      .catch(() => live && setFiles([]));
    return () => {
      live = false;
    };
  }, [id]);

  const shown = useMemo(() => {
    const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
    return (files ?? []).filter((f) => words.every((w) => f.path.toLowerCase().includes(w)));
  }, [files, query]);

  const open = (next: string, discard = false) => {
    if (next === path) return;
    if (dirty && !discard) return setPending(next);
    const seq = ++readSeq.current;
    setPending(null);
    setPath(next);
    setText(null);
    setSaved(null);
    setError(null);
    call<string>("read_config", { id, path: next })
      .then((value) => {
        if (!mounted.current || readSeq.current !== seq) return;
        setText(value);
        setSaved(value);
      })
      .catch((e) => {
        if (!mounted.current || readSeq.current !== seq) return;
        setError(toError(e).message);
      });
  };

  const save = async () => {
    if (saving || !path || text == null) return;
    const targetPath = path;
    const targetText = text;
    setSaving(true);
    try {
      await call("write_config", { id, path: targetPath, text: targetText });
      if (mounted.current && pathRef.current === targetPath) setSaved(targetText);
      s.toast(t("已保存（已自动备份原文件）", "Saved (previous file backed up)"), "success");
    } catch (e) {
      s.toast(toError(e).message, "error");
    } finally {
      if (mounted.current) setSaving(false);
    }
  };

  if (!s.instance) return null;
  return (
    <Modal
      wide
      title={t("模组配置文件", "Mod config files")}
      subtitle={files ? t(`${s.instance.name} · ${files.length} 个文件`, `${s.instance.name} · ${files.length} files`) : t("正在读取…", "Loading…")}
      onClose={back}
      footer={
        <>
          <Button variant="ghost" icon="chevronLeft" onClick={back} style={{ marginRight: "auto" }}>
            {t("返回模组", "Back to mods")}
          </Button>
          {dirty && (
            <Button variant="ghost" onClick={() => setText(saved)}>
              {t("撤销修改", "Discard changes")}
            </Button>
          )}
          <Button variant="primary" icon="disk" busy={saving} disabled={!dirty || saving} onClick={() => void save()}>
            {t("保存", "Save")}
          </Button>
        </>
      }
    >
      <div className="config-dialog">
        <div className="config-files">
          <Input icon="search" value={query} placeholder={t("按文件名筛选，例如 sodium", "Filter by name, e.g. sodium")} onChange={(e) => setQuery(e.target.value)} />
          <div className="config-list" role="listbox" aria-label={t("配置文件", "Config files")}>
            {files == null ? null : shown.length === 0 ? (
              <div className="faint" style={{ fontSize: "var(--text-sm)", padding: 8 }}>
                {files.length === 0 ? t("暂无配置文件，启动一次游戏后生成。", "No config files yet; generated on first launch.") : t("没有匹配的文件。", "No matching files.")}
              </div>
            ) : (
              shown.map((f) => (
                <button key={f.path} type="button" role="option" aria-selected={f.path === path} className="config-item" onClick={() => open(f.path)}>
                  <span className="config-name mono">{f.path.replace(/^config\//, "")}</span>
                  <span className="faint mono" style={{ fontSize: "var(--text-xs)" }}>
                    {formatBytes(f.bytes)} · {formatRelative(f.modifiedMs)}
                  </span>
                </button>
              ))
            )}
          </div>
        </div>
        <div className="config-editor">
          {pending != null && (
            <div className="row config-pending">
              <span style={{ flex: 1 }}>{t("这个文件改了还没保存。", "This file has unsaved changes.")}</span>
              <Button size="sm" variant="ghost" onClick={() => setPending(null)}>
                {t("继续编辑", "Keep editing")}
              </Button>
              <Button size="sm" variant="danger-ghost" onClick={() => (pending === "" ? leave() : open(pending, true))}>
                {t("放弃修改", "Discard")}
              </Button>
            </div>
          )}
          {s.running && <div className="faint" style={{ fontSize: "var(--text-xs)" }}>{t("游戏运行中修改配置可能被覆盖，建议改完重启游戏。", "Edits while playing may be overwritten; restart the game after saving.")}</div>}
          {path == null ? (
            <div className="faint config-empty">{t("从左边选一个文件。", "Pick a file on the left.")}</div>
          ) : error ? (
            <div className="faint config-empty">{error}</div>
          ) : text == null ? (
            <div className="faint config-empty">{t("正在读取…", "Loading…")}</div>
          ) : (
            <textarea
              className="config-text mono"
              spellCheck={false}
              aria-label={path}
              value={text}
              onChange={(e) => setText(e.target.value)}
              onKeyDown={(e) => {
                if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "s") {
                  e.preventDefault();
                  if (dirty) void save();
                }
              }}
            />
          )}
        </div>
      </div>
    </Modal>
  );
}
