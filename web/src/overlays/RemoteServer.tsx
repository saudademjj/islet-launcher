import { useCallback, useEffect, useRef, useState } from "react";

import { call, toError } from "../api/backend";
import type { ErrorPayload, Instance, RemoteInfo, RemoteProbe, RemoteStatus, RemoteTarget } from "../api/types";
import { Modal } from "../components/overlay";
import { WorldChoice } from "../components/WorldChoice";
import { Button, Field, Input, Notice, ProgressBar, Segmented, Status, Switch, Tabs } from "../components/ui";
import { formatBytes, formatRelative } from "../lib/format";
import { t } from "../lib/i18n";
import { findInstance, usePick, useStore } from "../store";

const EULA = "https://aka.ms/MinecraftEULA";
const SUPPORTED = ["vanilla", "fabric", "quilt", "forge", "neoforge", "paper", "purpur"];
const portOk = (text: string) => /^\d{1,5}$/.test(text) && Number(text) > 0 && Number(text) < 65536;
const digits = (text: string, max: number) => text.replace(/\D/g, "").slice(0, max);
/** "0" is automatic: the start script sizes it to the host. */
export const memoryValid = (text: string) => text === "0" || (Number(text) >= 512 && Number(text) <= 32768);

type Form = {
  host: string;
  port: string;
  user: string;
  auth: "password" | "key";
  keyPath: string;
  secret: string;
  dir: string;
  gamePort: string;
  address: string;
  memory: string;
  onlineMode: boolean;
  autostart: boolean;
  dailyBackup: boolean;
  keep: string;
};

/** A new server starts from the project's settings, or automatic memory. */
function formFrom(info: RemoteInfo, instance: Instance | null): Form {
  const saved = info.target;
  const project = instance?.server;
  return {
    host: saved?.host ?? "",
    port: String(saved?.port ?? 22),
    user: saved?.user ?? "root",
    auth: saved?.auth ?? "password",
    keyPath: saved?.keyPath ?? "",
    secret: "",
    dir: saved?.dir || info.defaultDir,
    gamePort: String(saved?.gamePort ?? project?.port ?? 25565),
    address: saved?.address ?? "",
    memory: String(saved?.memoryMb ?? project?.memoryMb ?? 0),
    onlineMode: saved?.onlineMode ?? project?.onlineMode ?? true,
    autostart: saved?.autostart ?? true,
    dailyBackup: saved?.dailyBackup ?? true,
    keep: String(saved?.keepBackups ?? 7),
  };
}

function targetFrom(form: Form, saved: RemoteTarget | null): RemoteTarget {
  return {
    host: form.host.trim(),
    port: Number(form.port),
    user: form.user.trim(),
    auth: form.auth,
    keyPath: form.auth === "key" ? form.keyPath : null,
    dir: form.dir.trim(),
    gamePort: Number(form.gamePort),
    address: form.address.trim(),
    fingerprint: saved?.fingerprint ?? null,
    autostart: form.autostart,
    dailyBackup: form.dailyBackup,
    keepBackups: Number(form.keep),
    memoryMb: Number(form.memory),
    onlineMode: form.onlineMode,
    deployedAt: saved?.deployedAt ?? null,
  };
}

/** A world's server on a Linux host the player rents: connect over SSH,
    upload the world and its mods, then start, stop, back up and update it
    from here. Nothing in between but the host itself. */
export function RemoteServer({ id }: { id: string }) {
  const s = usePick((s) => ({
    instance: findInstance(s, id),
    fileBusy: s.fileBusy,
    filesChanged: s.filesChanged,
    progress: s.remoteProgress?.instanceId === id ? s.remoteProgress : null,
    openModal: s.openModal,
    closeModal: s.closeModal,
    openUrl: s.openUrl,
    chooseFiles: s.chooseFiles,
    chooseSavePath: s.chooseSavePath,
    fileOperation: s.fileOperation,
    copy: s.copy,
    open: s.open,
    toast: s.toast,
  }));
  const [info, setInfo] = useState<RemoteInfo | null>(null);
  const [form, setForm] = useState<Form | null>(null);
  const [editing, setEditing] = useState(false);
  const [probe, setProbe] = useState<RemoteProbe | null>(null);
  const [world, setWorld] = useState<string | null>(null);
  const [eula, setEula] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<ErrorPayload | null>(null);
  const [status, setStatus] = useState<RemoteStatus | null>(null);
  const [tab, setTab] = useState<"console" | "backups" | "settings">("console");
  const [log, setLog] = useState<string | null>(null);
  const [command, setCommand] = useState("");

  const mountedRef = useRef(true);
  const busyRef = useRef(false);
  const refreshingRef = useRef(false);
  const loadSeqRef = useRef(0);
  const logSeqRef = useRef(0);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const load = useCallback(async () => {
    const seq = ++loadSeqRef.current;
    try {
      const value = await call<RemoteInfo>("remote_info", { id });
      if (!mountedRef.current || loadSeqRef.current !== seq) return;
      setInfo(value);
      setForm((old) => old ?? formFrom(value, findInstance(useStore.getState(), id)));
    } catch (e) {
      if (mountedRef.current && loadSeqRef.current === seq) setError(toError(e));
    }
  }, [id]);
  useEffect(() => {
    void load();
  }, [load, s.filesChanged]);

  const deploying = s.fileBusy && !!s.progress;
  const deployed = !!info?.target?.deployedAt && !editing;

  const refresh = useCallback(async () => {
    if (refreshingRef.current) return;
    refreshingRef.current = true;
    try {
      const next = await call<RemoteStatus>("remote_status", { id });
      if (mountedRef.current) setStatus(next);
    } catch (e) {
      if (mountedRef.current) setError(toError(e));
    } finally {
      refreshingRef.current = false;
    }
  }, [id]);
  const readLog = useCallback(async () => {
    const seq = ++logSeqRef.current;
    try {
      const next = await call<string>("remote_log", { id, lines: 300 });
      if (mountedRef.current && logSeqRef.current === seq) setLog(next);
    } catch (e) {
      if (mountedRef.current && logSeqRef.current === seq) setError(toError(e));
    }
  }, [id]);
  useEffect(() => {
    if (!deployed || deploying) return;
    void refresh();
    const timer = setInterval(() => void refresh(), 15000);
    return () => clearInterval(timer);
  }, [deployed, deploying, refresh, s.filesChanged]);
  useEffect(() => {
    if (deployed && !deploying && tab === "console") void readLog();
  }, [deployed, deploying, tab, readLog]);

  if (!s.instance || !info || !form) return null;
  const instance = s.instance;
  const set = (patch: Partial<Form>) => setForm((old) => (old ? { ...old, ...patch } : old));
  // A project's dialog opens from the server page and goes back to it.
  const back = () => (instance.server ? s.closeModal() : s.openModal({ kind: "manage", id, tab: "tools" }));
  const run = async (label: string, work: () => Promise<void>) => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(label);
    setError(null);
    try {
      await work();
    } catch (e) {
      if (mountedRef.current) setError(toError(e));
    } finally {
      busyRef.current = false;
      if (mountedRef.current) setBusy(null);
    }
  };

  const supported = SUPPORTED.includes(instance.loader);
  const memoryOk = memoryValid(form.memory);
  const keepOk = Number(form.keep) >= 1 && Number(form.keep) <= 100;
  const connectionOk = !!form.host.trim() && portOk(form.port) && !!form.user.trim() && !!form.dir.trim() && (form.auth === "password" ? !!form.secret || info.hasSecret : !!form.keyPath);
  const settingsOk = portOk(form.gamePort) && memoryOk && keepOk;
  const target = () => targetFrom(form, info.target);

  const connect = () =>
    run("connect", async () => {
      setProbe(null);
      const found = await call<RemoteProbe>("remote_connect", { id, target: target(), secret: form.secret || null });
      setProbe(found);
      set({ secret: "" });
      await load();
    });
  const deploy = () =>
    run("deploy", async () => {
      await call("remote_settings", { id, target: target() });
      setEditing(false);
      await s.fileOperation(id, { kind: "deployServer", options: { acceptEula: eula, world } }, true);
    });
  const action = (name: "start" | "stop" | "restart" | "backup") =>
    run(name, async () => {
      const out = await call<string>("remote_action", { id, action: name });
      if (name === "backup") s.toast(t(`已备份：${out.split("/").pop()}`, `Backed up: ${out.split("/").pop()}`), "success");
      await refresh();
      if (tab === "console") await readLog();
    });
  const send = () =>
    run("command", async () => {
      await call("remote_command", { id, text: command });
      setCommand("");
      await new Promise((r) => setTimeout(r, 800));
      await readLog();
    });
  const saveSettings = () =>
    run("settings", async () => {
      await call("remote_settings", { id, target: target() });
      await load();
      s.toast(t("设置已保存；内存、端口和正版验证在重启服务器后生效", "Saved; memory, port and online mode apply after a restart"), "success");
    });
  const download = (name: string) =>
    run("download", async () => {
      const file = name.split("/").pop() ?? "world.tar.gz";
      const destination = await s.chooseSavePath(t("保存备份", "Save backup"), file, "gz");
      if (!destination) return;
      await call("remote_download_backup", { id, name, destination });
      s.toast(t("备份已下载", "Backup downloaded"), "success");
    });
  const forget = () =>
    run("forget", async () => {
      await call("remote_forget", { id });
      setProbe(null);
      setStatus(null);
      setEditing(false);
      const value = await call<RemoteInfo>("remote_info", { id });
      setInfo(value);
      setForm(formFrom(value, instance));
    });

  const errorNotice = error && (
    <Notice tone="danger" title={error.message} detail={error.detail}>
      {null}
    </Notice>
  );

  const progress = s.progress && (
    <div className="col gap-8">
      <span className="setting-name">
        {s.progress.stage}
        {s.progress.total > 0 ? ` · ${formatBytes(s.progress.done)} / ${formatBytes(s.progress.total)}` : s.progress.done > 0 ? ` · ${s.progress.done}s` : "…"}
      </span>
      <ProgressBar ratio={s.progress.total > 0 ? s.progress.done / s.progress.total : null} />
      <span className="field-hint">{t("可以关掉这个窗口，完成后会提醒你。", "You can close this window; you'll be told when it's done.")}</span>
    </div>
  );

  // ----- set up: where the server goes, then what it runs ---------------------
  if (!deployed) {
    const first = !probe?.deployed;
    return (
      <Modal
        wide
        bodyClass="modal-body fill"
        title={t("部署到云服务器", "Deploy to a cloud server")}
        subtitle={`${instance.name} · ${instance.versionId}`}
        onClose={back}
        footer={
          <>
            {info.target?.deployedAt && (
              <Button variant="ghost" onClick={() => setEditing(false)}>
                {t("返回管理", "Back to the server")}
              </Button>
            )}
            <Button variant="ghost" onClick={back}>
              {t("关闭", "Close")}
            </Button>
            {!probe ? (
              <Button variant="primary" icon="link" busy={busy === "connect"} disabled={!connectionOk || !!busy} onClick={() => void connect()}>
                {t("连接并检查", "Connect and check")}
              </Button>
            ) : (
              <Button variant="primary" icon="server" busy={busy === "deploy"} disabled={!!busy || s.fileBusy || !supported || !settingsOk || (first && !eula)} onClick={() => void deploy()}>
                {first ? t("上传并启动", "Upload and start") : t("更新并重启", "Update and restart")}
              </Button>
            )}
          </>
        }
      >
        <div className="col gap-16">
          {deploying ? (
            progress
          ) : (
            <>
              <span className="field-hint">
                {instance.server
                  ? t(
                      "需 Linux 云服务器（建议 2 核 4G 起），直连上传服务端并自动配置 Java 与内存。",
                      "Requires a Linux server (2+ cores, 4+ GB RAM). Uploads directly and configures Java and memory.",
                    )
                  : t(
                      "需 Linux 云服务器（建议 2 核 4G 起），直连上传服务端、模组与存档并自动配置 Java。",
                      "Requires a Linux server (2+ cores, 4+ GB RAM). Uploads server, mods, and save directly.",
                    )}
              </span>
              {!supported && <span className="field-hint bad">{t("仅支持原版、Fabric、Quilt、Forge、NeoForge、Paper 和 Purpur。", "Supported for Vanilla, Fabric, Quilt, Forge, NeoForge, Paper, and Purpur.")}</span>}
              {errorNotice}
              <div className="row gap-12 wrap items-start">
                <Field label={t("服务器 IP 或域名", "Server IP or domain")}>
                  <Input mono value={form.host} placeholder="203.0.113.10" style={{ width: 220 }} onChange={(e) => (set({ host: e.target.value.trim().slice(0, 253) }), setProbe(null))} />
                </Field>
                <Field label={t("SSH 端口", "SSH port")} tone={portOk(form.port) ? undefined : "bad"}>
                  <Input mono inputMode="numeric" value={form.port} style={{ width: 90 }} invalid={!portOk(form.port)} onChange={(e) => (set({ port: digits(e.target.value, 5) }), setProbe(null))} />
                </Field>
                <Field label={t("用户名", "User")}>
                  <Input mono value={form.user} style={{ width: 130 }} onChange={(e) => (set({ user: e.target.value.trim().slice(0, 64) }), setProbe(null))} />
                </Field>
              </div>
              <Field label={t("登录方式", "Sign in with")}>
                <Segmented
                  value={form.auth}
                  options={[
                    { value: "password", label: t("密码", "Password") },
                    { value: "key", label: t("SSH 密钥", "SSH key") },
                  ]}
                  onChange={(auth) => (set({ auth }), setProbe(null))}
                />
              </Field>
              {form.auth === "password" ? (
                <Field label={t("密码", "Password")} hint={info.hasSecret ? t("已安全保存，留空使用已存密码", "Saved securely; leave blank to reuse") : t("安全保存于系统钥匙串，不写入配置文件", "Stored in the system keychain, never in config files")}>
                  <Input type="password" autoComplete="off" value={form.secret} placeholder={info.hasSecret ? "••••••••" : ""} style={{ width: 260 }} onChange={(e) => set({ secret: e.target.value })} />
                </Field>
              ) : (
                <div className="row gap-12 wrap" style={{ alignItems: "flex-end" }}>
                  <Field label={t("私钥文件", "Private key")}>
                    <div className="row gap-8">
                      <Input mono readOnly value={form.keyPath} placeholder="~/.ssh/id_ed25519" style={{ width: 260 }} />
                      <Button
                        size="sm"
                        icon="folder"
                        onClick={async () => {
                          const [path] = await s.chooseFiles("sshKey");
                          if (path) (set({ keyPath: path }), setProbe(null));
                        }}
                      >
                        {t("选择…", "Choose…")}
                      </Button>
                    </div>
                  </Field>
                  <Field label={t("私钥密码（没有就留空）", "Key passphrase (if any)")}>
                    <Input type="password" autoComplete="off" value={form.secret} style={{ width: 200 }} onChange={(e) => set({ secret: e.target.value })} />
                  </Field>
                </div>
              )}
              <Field label={t("服务器文件夹", "Server folder")} hint={t("相对于登录用户的主目录；不存在会自动创建", "Relative to the user's home; created if missing")}>
                <Input mono value={form.dir} style={{ width: 260 }} onChange={(e) => (set({ dir: e.target.value.trim().slice(0, 200) }), setProbe(null))} />
              </Field>

              {probe && (
                <>
                  <Notice icon="check" title={t(`已连接：${probe.os} ${probe.arch}`, `Connected: ${probe.os} ${probe.arch}`)}>
                    {[
                      probe.java ? t(`Java ${probe.java[1]}`, `Java ${probe.java[1]}`) : t("没有 Java（会自动装上）", "No Java (it'll be installed)"),
                      probe.freeMb != null ? t(`剩余 ${formatBytes(probe.freeMb * 1024 * 1024)}`, `${formatBytes(probe.freeMb * 1024 * 1024)} free`) : null,
                      probe.deployed ? t("这个文件夹里已有服务器：会更新模组并保留世界", "A server is already in this folder: mods are updated and the world kept") : null,
                      probe.root ? null : t("普通用户，将使用定时任务自启", "Standard user; autostart uses scheduled tasks"),
                    ]
                      .filter(Boolean)
                      .join(" · ")}
                  </Notice>
                  {first && !instance.server && (
                    <Field label={t("服务器的世界", "The server's world")} hint={world ? t("复制该存档上传，保留你的背包和位置", "Uploads a copy of this save, keeping your inventory and spot") : undefined}>
                      <WorldChoice instance={instance} value={world} onChange={setWorld} />
                    </Field>
                  )}
                  <ServerSettings form={form} set={set} />
                  {first && (
                    <div className="setting-row">
                      <div className="col">
                        <span className="setting-name">{t("我同意 Minecraft 最终用户许可协议（EULA）", "I agree to the Minecraft EULA")}</span>
                        <span className="field-hint">
                          {t("服务器要先同意才能运行。", "The server only runs once it's agreed to.")}{" "}
                          <button type="button" className="link" onClick={() => void s.openUrl(EULA)}>
                            {t("阅读 EULA", "Read the EULA")}
                          </button>
                        </span>
                      </div>
                      <Switch label={t("同意 EULA", "Agree to the EULA")} checked={eula} onChange={setEula} />
                    </div>
                  )}
                </>
              )}
            </>
          )}
        </div>
      </Modal>
    );
  }

  // ----- a running server: status, console, backups, settings ----------------
  const st = status?.state;
  const tone = !st ? "neutral" : st.running ? (st.ready ? "accent" : "warn") : "neutral";
  const label = !st ? t("读取中…", "Checking…") : st.running ? (st.ready ? t("运行中", "Running") : t("启动中", "Starting")) : t("已停止", "Stopped");
  const address = status?.address ?? info.target?.address ?? "";
  return (
    <Modal
      wide
      bodyClass="modal-body fill"
      title={t("云服务器", "Cloud server")}
      subtitle={`${instance.name} · ${info.target?.user}@${info.target?.host}`}
      onClose={back}
      footer={
        <>
          <Button variant="ghost" onClick={back}>
            {t("关闭", "Close")}
          </Button>
          <Button icon="refresh" disabled={!!busy || s.fileBusy} title={t("重新生成服务端并上传：同步模组和配置，世界保留", "Rebuild and upload: mods and configs are synced, the world is kept")} onClick={() => void s.fileOperation(id, { kind: "deployServer", options: { acceptEula: true, world: null } }, true)}>
            {t("同步模组并重启", "Sync mods and restart")}
          </Button>
          {info.pack && (
            <Button variant="primary" icon="folder" onClick={() => void s.open({ kind: "serverPack", id })}>
              {t("好友用的整合包", "Pack for friends")}
            </Button>
          )}
        </>
      }
    >
      <div className="col gap-16">
        {deploying && progress}
        {errorNotice}
        <div className="setting-row" style={{ flexWrap: "wrap" }}>
          <div className="row gap-12 wrap">
            <Status tone={tone} pulse={!!st?.running}>
              {label}
            </Status>
            <span className="mono">{address}</span>
            <Button size="sm" variant="ghost" icon="copy" aria-label={t("复制地址", "Copy address")} title={t("复制地址", "Copy address")} onClick={() => void s.copy(address, t("服务器地址", "Server address"))} />
            {status?.ping && (
              <span className="field-hint">
                {t(`${status.ping.online}/${status.ping.max} 人在线 · ${status.ping.latencyMs} ms`, `${status.ping.online}/${status.ping.max} online · ${status.ping.latencyMs} ms`)}
                {status.ping.players.length > 0 && ` · ${status.ping.players.join(", ")}`}
              </span>
            )}
            {st?.running && st.ready && !status?.ping && <span className="field-hint bad">{t(`无法从本机连通，请在防火墙放行 TCP ${info.target?.gamePort}`, `Unreachable: allow TCP ${info.target?.gamePort} in firewall`)}</span>}
          </div>
          <div className="row gap-6">
            {st?.running ? (
              <>
                <Button size="sm" busy={busy === "restart"} disabled={!!busy || deploying} onClick={() => void action("restart")}>
                  {t("重启", "Restart")}
                </Button>
                <Button size="sm" variant="danger-ghost" icon="stop" busy={busy === "stop"} disabled={!!busy || deploying} onClick={() => void action("stop")}>
                  {t("停止", "Stop")}
                </Button>
              </>
            ) : (
              <Button size="sm" variant="go" icon="play" busy={busy === "start"} disabled={!!busy || deploying || !st} onClick={() => void action("start")}>
                {t("启动", "Start")}
              </Button>
            )}
          </div>
        </div>
        <Tabs
          value={tab}
          onChange={setTab}
          tabs={[
            { value: "console", label: t("控制台", "Console") },
            { value: "backups", label: t(`备份${st ? ` ${st.backups.length}` : ""}`, `Backups${st ? ` ${st.backups.length}` : ""}`) },
            { value: "settings", label: t("设置", "Settings") },
          ]}
        />
        {tab === "console" && (
          <div className="col gap-8">
            <pre className="console selectable" style={{ margin: 0, padding: "10px 14px", height: 260, overflow: "auto", whiteSpace: "pre-wrap", borderRadius: "var(--radius-lg)" }}>
              {log ?? t("正在读取日志…", "Loading the log…")}
            </pre>
            <form
              className="row gap-8"
              
              onSubmit={(e) => {
                e.preventDefault();
                if (command.trim()) void send();
              }}
            >
              <Input mono value={command} placeholder={t("输入命令，比如 say 大家好 或 op 玩家名", "A command, like say hello or op PlayerName")} style={{ flex: 1 }} disabled={!st?.running} onChange={(e) => setCommand(e.target.value.slice(0, 1000))} />
              <Button type="submit" busy={busy === "command"} disabled={!st?.running || !command.trim() || !!busy}>
                {t("发送", "Send")}
              </Button>
              <Button type="button" variant="ghost" icon="refresh" aria-label={t("刷新日志", "Reload the log")} title={t("刷新日志", "Reload the log")} onClick={() => void readLog()} />
            </form>
          </div>
        )}
        {tab === "backups" && (
          <div className="col gap-12">
            <div className="setting-row">
              <span className="field-hint">
                {info.target?.dailyBackup
                  ? st?.scheduled === "none"
                    ? t("当前服务器不支持定时任务，无法开启自动备份", "Scheduled tasks unavailable on this host; automatic backups disabled")
                    : t(`每天 5:17 自动备份，保留最近 ${info.target.keepBackups} 份`, `Backed up daily at 5:17, the newest ${info.target.keepBackups} kept`)
                  : t("自动备份已关闭", "Automatic backups are off")}
              </span>
              <Button size="sm" icon="archive" busy={busy === "backup"} disabled={!!busy || deploying} onClick={() => void action("backup")}>
                {t("立即备份", "Back up now")}
              </Button>
            </div>
            <div className="list" style={{ maxHeight: 260, overflowY: "auto" }}>
              {st?.backups.length ? (
                st.backups.map(([name, bytes]) => (
                  <div key={name} className="list-row">
                    <span className="mono truncate" style={{ flex: 1 }}>
                      {name.split("/").pop()}
                    </span>
                    <span className="field-hint">{formatBytes(bytes)}</span>
                    <Button size="sm" variant="go" icon="download" disabled={!!busy} onClick={() => void download(name)}>
                      {t("下载", "Download")}
                    </Button>
                  </div>
                ))
              ) : (
                <div className="list-row field-hint">{t("还没有备份", "No backups yet")}</div>
              )}
            </div>
          </div>
        )}
        {tab === "settings" && (
          <div className="col gap-16">
            <ServerSettings form={form} set={set} />
            <div className="row gap-8 wrap">
              <Button variant="primary" busy={busy === "settings"} disabled={!!busy || deploying || !settingsOk} onClick={() => void saveSettings()}>
                {t("保存设置", "Save settings")}
              </Button>
              <Button variant="ghost" icon="edit" disabled={deploying} onClick={() => (setEditing(true), setProbe(null))}>
                {t("修改连接", "Change connection")}
              </Button>
              <Button variant="danger-ghost" icon="trash" busy={busy === "forget"} disabled={!!busy || deploying} onClick={() => void forget()}>
                {t("忘记这台服务器", "Forget this server")}
              </Button>
            </div>
            <span className="field-hint">
              {t(
                `上次上传：${formatRelative(info.target?.deployedAt ?? null)}；仅移除本机连接信息，不影响服务器文件。`,
                `Last upload: ${formatRelative(info.target?.deployedAt ?? null)}. Only removes local connection info; server files remain untouched.`,
              )}
            </span>
          </div>
        )}
      </div>
    </Modal>
  );
}

/** The server's memory: automatic (sized to the host when it starts) or
    a fixed number of MB. */
export function MemoryField({ value, onChange }: { value: string; onChange: (value: string) => void }) {
  const auto = value === "0";
  const ok = memoryValid(value);
  return (
    <Field
      label={t("内存", "Memory")}
      tone={ok ? undefined : "bad"}
      hint={auto ? t("启动时按服务器内存自动分配，并同步调整视距", "Sized automatically to host RAM on start, tuning view distance too") : ok ? t("单位 MB，建议比机器总内存少 1G 以上", "In MB; leave at least 1 GB for the OS") : t("512 到 32768 MB", "512 to 32768 MB")}
    >
      <div className="row gap-8">
        <Segmented
          value={auto ? "auto" : "fixed"}
          options={[
            { value: "auto", label: t("自动", "Automatic") },
            { value: "fixed", label: t("固定", "Fixed") },
          ]}
          onChange={(mode) => onChange(mode === "auto" ? "0" : "4096")}
        />
        {!auto && <Input mono inputMode="numeric" aria-label={t("内存（MB）", "Memory (MB)")} value={value} style={{ width: 90 }} invalid={!ok} onChange={(e) => onChange(digits(e.target.value, 5))} />}
      </div>
    </Field>
  );
}

/** How the server is kept: shared by the first upload and later changes. */
function ServerSettings({ form, set }: { form: Form; set: (patch: Partial<Form>) => void }) {
  const keepOk = Number(form.keep) >= 1 && Number(form.keep) <= 100;
  return (
    <>
      <div className="row gap-12 wrap items-start">
        <Field label={t("游戏端口", "Game port")} tone={portOk(form.gamePort) ? undefined : "bad"} hint={t("需在防火墙放行", "Open in firewall")}>
          <Input mono inputMode="numeric" value={form.gamePort} style={{ width: 100 }} invalid={!portOk(form.gamePort)} onChange={(e) => set({ gamePort: digits(e.target.value, 5) })} />
        </Field>
        <MemoryField value={form.memory} onChange={(memory) => set({ memory })} />
        <Field label={t("好友连接的地址（可不填）", "Address friends join (optional)")} hint={t("有域名就填域名；不填用 IP 和端口", "A domain if you have one; else the IP and port")}>
          <Input mono value={form.address} placeholder="mc.example.com" style={{ width: 220 }} onChange={(e) => set({ address: e.target.value.trim().slice(0, 255) })} />
        </Field>
      </div>
      <div className="setting-row">
        <div className="col">
          <span className="setting-name">{t("正版验证", "Online mode")}</span>
          <span className="field-hint">{t("关闭后离线账号可加入，但名称无验证", "Off lets offline accounts join without name checks")}</span>
        </div>
        <Switch label={t("正版验证", "Online mode")} checked={form.onlineMode} onChange={(onlineMode) => set({ onlineMode })} />
      </div>
      <div className="setting-row">
        <div className="col">
          <span className="setting-name">{t("开机自动启动", "Start when the host boots")}</span>
          <span className="field-hint">{t("服务器重启后自动拉起 Minecraft", "Automatically restarts Minecraft after host reboot")}</span>
        </div>
        <Switch label={t("开机自动启动", "Start at boot")} checked={form.autostart} onChange={(autostart) => set({ autostart })} />
      </div>
      <div className="setting-row">
        <div className="col">
          <span className="setting-name">{t("每天自动备份", "Back up every day")}</span>
          <span className="field-hint">{t("凌晨 5:17 打包一份世界，不停服", "Packs the world at 5:17 each morning without stopping the server")}</span>
        </div>
        <div className="row gap-8">
          {form.dailyBackup && (
            <>
              <span className="field-hint">{t("保留", "Keep")}</span>
              <Input mono inputMode="numeric" aria-label={t("保留份数", "Backups to keep")} value={form.keep} style={{ width: 56 }} invalid={!keepOk} onChange={(e) => set({ keep: digits(e.target.value, 3) })} />
            </>
          )}
          <Switch label={t("每天自动备份", "Back up every day")} checked={form.dailyBackup} onChange={(dailyBackup) => set({ dailyBackup })} />
        </div>
      </div>
    </>
  );
}
