import { useCallback, useEffect, useState } from "react";

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

  const load = useCallback(async () => {
    try {
      const value = await call<RemoteInfo>("remote_info", { id });
      setInfo(value);
      setForm((old) => old ?? formFrom(value, findInstance(useStore.getState(), id)));
    } catch (e) {
      setError(toError(e));
    }
  }, [id]);
  useEffect(() => {
    void load();
  }, [load, s.filesChanged]);

  const deploying = s.fileBusy && !!s.progress;
  const deployed = !!info?.target?.deployedAt && !editing;

  const refresh = useCallback(async () => {
    try {
      setStatus(await call<RemoteStatus>("remote_status", { id }));
    } catch (e) {
      setError(toError(e));
    }
  }, [id]);
  const readLog = useCallback(async () => {
    try {
      setLog(await call<string>("remote_log", { id, lines: 300 }));
    } catch (e) {
      setError(toError(e));
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
  const set = (patch: Partial<Form>) => setForm({ ...form, ...patch });
  // A project's dialog opens from the server page and goes back to it.
  const back = () => (instance.server ? s.closeModal() : s.openModal({ kind: "manage", id, tab: "data" }));
  const run = async (label: string, work: () => Promise<void>) => {
    setBusy(label);
    setError(null);
    try {
      await work();
    } catch (e) {
      setError(toError(e));
    } finally {
      setBusy(null);
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
    <div className="col" style={{ gap: 8 }}>
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
        <div className="col" style={{ gap: 16 }}>
          {deploying ? (
            progress
          ) : (
            <>
              <span className="field-hint">
                {instance.server
                  ? t(
                      "租一台 Linux 云服务器（2 核 4G 起），把它的 IP、用户名和密码填在这里。启动器会把服务端和模组、插件传上去，装好 Java，按服务器的内存调好参数并启动；之后在这里开关、备份和更新。不经过任何第三方中转。",
                      "Rent a Linux cloud server (2 cores and 4 GB or more) and fill in its IP, user and password. The launcher uploads the server with its mods or plugins, installs Java, tunes it to the host's memory and starts it; start, stop, back up and update it from here later. Nothing sits in between.",
                    )
                  : t(
                      "租一台 Linux 云服务器（2 核 4G 起），把它的 IP、用户名和密码填在这里。启动器会把这个世界的服务端、模组和存档传上去，装好 Java 并启动；之后在这里开关、备份和更新。不经过任何第三方中转。",
                      "Rent a Linux cloud server (2 cores and 4 GB or more) and fill in its IP, user and password. The launcher uploads this world's server, mods and save, installs Java and starts it; start, stop, back up and update it from here later. Nothing sits in between.",
                    )}
              </span>
              {!supported && <span className="field-hint bad">{t("这个加载器没有可以一键生成的服务端；原版、Fabric、Quilt、Forge 和 NeoForge 可以。", "This loader has no server Islet can set up; vanilla, Fabric, Quilt, Forge and NeoForge do.")}</span>}
              {errorNotice}
              <div className="row" style={{ gap: 12, flexWrap: "wrap", alignItems: "flex-start" }}>
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
                <Field label={t("密码", "Password")} hint={info.hasSecret ? t("已保存在系统钥匙串里；留空就用保存的", "Saved in the system keychain; leave empty to use it") : t("保存在系统钥匙串里，不写进任何文件", "Kept in the system keychain, never in a file")}>
                  <Input type="password" autoComplete="off" value={form.secret} placeholder={info.hasSecret ? "••••••••" : ""} style={{ width: 260 }} onChange={(e) => set({ secret: e.target.value })} />
                </Field>
              ) : (
                <div className="row" style={{ gap: 12, flexWrap: "wrap", alignItems: "flex-end" }}>
                  <Field label={t("私钥文件", "Private key")}>
                    <div className="row" style={{ gap: 8 }}>
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
                      probe.root ? null : t("非 root 用户：开机自启用 crontab", "Not root: starting at boot uses crontab"),
                    ]
                      .filter(Boolean)
                      .join(" · ")}
                  </Notice>
                  {first && !instance.server && (
                    <Field label={t("服务器的世界", "The server's world")} hint={world ? t("复制这个存档上传；你在单人里的背包和位置会跟着你进服务器", "A copy of this save is uploaded; your singleplayer inventory and spot come with you") : undefined}>
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
              {t("朋友用的整合包", "Pack for friends")}
            </Button>
          )}
        </>
      }
    >
      <div className="col" style={{ gap: 16 }}>
        {deploying && progress}
        {errorNotice}
        <div className="setting-row" style={{ flexWrap: "wrap" }}>
          <div className="row" style={{ gap: 10, flexWrap: "wrap" }}>
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
            {st?.running && st.ready && !status?.ping && <span className="field-hint bad">{t(`从这里连不上：请在云服务器的安全组里放行 TCP ${info.target?.gamePort}`, `Unreachable from here: allow TCP ${info.target?.gamePort} in the cloud security group`)}</span>}
          </div>
          <div className="row" style={{ gap: 6 }}>
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
          <div className="col" style={{ gap: 8 }}>
            <pre className="console selectable" style={{ margin: 0, padding: "10px 14px", height: 260, overflow: "auto", whiteSpace: "pre-wrap", borderRadius: "var(--radius-lg)" }}>
              {log ?? t("正在读取日志…", "Loading the log…")}
            </pre>
            <form
              className="row"
              style={{ gap: 8 }}
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
          <div className="col" style={{ gap: 10 }}>
            <div className="setting-row">
              <span className="field-hint">
                {info.target?.dailyBackup
                  ? st?.scheduled === "none"
                    ? t("这台服务器没有 cron 也不是 root，自动备份没法设置", "This host has no cron and you're not root, so automatic backups can't be set up")
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
          <div className="col" style={{ gap: 14 }}>
            <ServerSettings form={form} set={set} />
            <div className="row" style={{ gap: 8, flexWrap: "wrap" }}>
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
                `上次上传：${formatRelative(info.target?.deployedAt ?? null)}。「忘记」只删掉启动器里的连接信息和密码，服务器上的文件和世界不动。`,
                `Last upload: ${formatRelative(info.target?.deployedAt ?? null)}. "Forget" only removes the connection and password here; files and the world on the host stay.`,
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
      hint={auto ? t("每次启动按机器内存分配，留出 1/4 给系统，视距也随之调整", "Sized to the machine at each start, leaving a quarter for the system; view distance follows") : ok ? t("单位 MB；比机器总内存少 1G 左右", "In MB; about 1 GB under the machine's total") : t("512 到 32768 MB", "512 to 32768 MB")}
    >
      <div className="row" style={{ gap: 8 }}>
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
      <div className="row" style={{ gap: 12, flexWrap: "wrap", alignItems: "flex-start" }}>
        <Field label={t("游戏端口", "Game port")} tone={portOk(form.gamePort) ? undefined : "bad"} hint={t("记得在安全组里放行", "Allow it in the security group")}>
          <Input mono inputMode="numeric" value={form.gamePort} style={{ width: 100 }} invalid={!portOk(form.gamePort)} onChange={(e) => set({ gamePort: digits(e.target.value, 5) })} />
        </Field>
        <MemoryField value={form.memory} onChange={(memory) => set({ memory })} />
        <Field label={t("朋友连接的地址（可不填）", "Address friends join (optional)")} hint={t("有域名就填域名；不填用 IP 和端口", "A domain if you have one; else the IP and port")}>
          <Input mono value={form.address} placeholder="mc.example.com" style={{ width: 220 }} onChange={(e) => set({ address: e.target.value.trim().slice(0, 255) })} />
        </Field>
      </div>
      <div className="setting-row">
        <div className="col">
          <span className="setting-name">{t("正版验证", "Online mode")}</span>
          <span className="field-hint">{t("只让微软正版账号进入；关掉后离线账号也能进，但谁都能冒用别人的名字", "Only Microsoft accounts can join; turned off, offline accounts can too, but anyone can use any name")}</span>
        </div>
        <Switch label={t("正版验证", "Online mode")} checked={form.onlineMode} onChange={(onlineMode) => set({ onlineMode })} />
      </div>
      <div className="setting-row">
        <div className="col">
          <span className="setting-name">{t("开机自动启动", "Start when the host boots")}</span>
          <span className="field-hint">{t("云服务器重启后，Minecraft 服务器自己起来", "The Minecraft server comes back up after the host restarts")}</span>
        </div>
        <Switch label={t("开机自动启动", "Start at boot")} checked={form.autostart} onChange={(autostart) => set({ autostart })} />
      </div>
      <div className="setting-row">
        <div className="col">
          <span className="setting-name">{t("每天自动备份", "Back up every day")}</span>
          <span className="field-hint">{t("凌晨 5:17 打包一份世界，不停服", "Packs the world at 5:17 each morning without stopping the server")}</span>
        </div>
        <div className="row" style={{ gap: 8 }}>
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
