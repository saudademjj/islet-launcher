import { useEffect, useState } from "react";
import { call, toError } from "../api/backend";
import { Modal } from "../components/overlay";
import { Button, Field, Input, Notice } from "../components/ui";
import { useStore } from "../store";
import { t } from "../lib/i18n";

type Status = { running: boolean; address: string; detail: string };
type Saved = { host: string; user: string; ssh: string; remote: string };

/** The relay host is the same for every world, so it is remembered. */
const SAVED_KEY = "tunnel-host";
function loadSaved(): Saved {
  const blank = { host: "", user: "", ssh: "22", remote: "25565" };
  try {
    return { ...blank, ...JSON.parse(localStorage.getItem(SAVED_KEY) ?? "{}") };
  } catch {
    return blank;
  }
}
const port = (text: string) => /^\d{1,5}$/.test(text) && Number(text) > 0 && Number(text) < 65536;

export function TunnelDialog({ id }: { id: string }) {
  const [saved] = useState(loadSaved);
  const [host, setHost] = useState(saved.host);
  const [user, setUser] = useState(saved.user);
  const [local, setLocal] = useState("25565");
  const [remote, setRemote] = useState(saved.remote);
  const [ssh, setSsh] = useState(saved.ssh);
  const [status, setStatus] = useState<Status | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const close = useStore((s) => s.closeModal);
  useEffect(() => {
    let live = true;
    const refresh = () => call<Status>("tunnel_status", { id }).then((v) => live && setStatus(v), (e) => live && setError(toError(e).message));
    void refresh();
    const timer = setInterval(refresh, 3000);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [id]);
  const running = !!status?.running;
  const ready = host.trim() && user.trim() && port(local) && port(remote) && port(ssh);
  const start = async () => {
    setBusy(true);
    setError("");
    try {
      setStatus(await call<Status>("start_tunnel", { id, host: host.trim(), user: user.trim(), localPort: Number(local), remotePort: Number(remote), sshPort: Number(ssh) }));
      try {
        localStorage.setItem(SAVED_KEY, JSON.stringify({ host: host.trim(), user: user.trim(), ssh, remote }));
      } catch {
        /* remembering is a convenience */
      }
    } catch (e) {
      setError(toError(e).message);
    } finally {
      setBusy(false);
    }
  };
  const stop = async () => {
    try {
      await call("stop_tunnel", { id });
      setStatus(null);
    } catch (e) {
      setError(toError(e).message);
    }
  };
  const footer = running ? (
    <>
      <Button variant="danger-ghost" onClick={() => void stop()}>
        {t("停止隧道", "Stop tunnel")}
      </Button>
      <Button variant="primary" icon="copy" onClick={() => void useStore.getState().copy(status!.address, t("联机地址", "Server address"))}>
        {t("复制联机地址", "Copy address")}
      </Button>
    </>
  ) : (
    <Button variant="primary" busy={busy} disabled={!ready} onClick={() => void start()}>
      {t("启动隧道", "Start tunnel")}
    </Button>
  );
  return (
    <Modal title={t("联机隧道", "Multiplayer tunnel")} subtitle={t("通过你自己的 SSH 主机，把局域网游戏开放给朋友", "Share a LAN game with friends through your own SSH host")} onClose={close} footer={footer}>
      <div className="col gap-16">
        <span className="muted" style={{ fontSize: "var(--text-sm)" }}>
          {t("先在游戏里选择「对局域网开放」，把游戏显示的端口填在下面。系统 SSH 需要已配好这台主机的密钥和 known_hosts。", "First choose \"Open to LAN\" in the game and enter the port it shows below. System SSH must already have a key and known_hosts entry for this host.")}
        </span>
        <div className="row" style={{ alignItems: "flex-start" }}>
          <div style={{ flex: 2 }}>
            <Field label={t("SSH 主机", "SSH host")}>
              <Input value={host} placeholder="relay.example.com" onChange={(e) => setHost(e.target.value)} disabled={running} />
            </Field>
          </div>
          <div style={{ flex: 1 }}>
            <Field label={t("用户", "User")}>
              <Input value={user} onChange={(e) => setUser(e.target.value)} disabled={running} />
            </Field>
          </div>
        </div>
        <div className="row" style={{ alignItems: "flex-start" }}>
          <Field label={t("游戏局域网端口", "LAN game port")} tone={local && !port(local) ? "bad" : undefined}>
            <Input value={local} inputMode="numeric" onChange={(e) => setLocal(e.target.value)} disabled={running} />
          </Field>
          <Field label={t("公网端口", "Public port")} tone={remote && !port(remote) ? "bad" : undefined}>
            <Input value={remote} inputMode="numeric" onChange={(e) => setRemote(e.target.value)} disabled={running} />
          </Field>
          <Field label={t("SSH 端口", "SSH port")} tone={ssh && !port(ssh) ? "bad" : undefined}>
            <Input value={ssh} inputMode="numeric" onChange={(e) => setSsh(e.target.value)} disabled={running} />
          </Field>
        </div>
        {running ? (
          <Notice icon="check" title={t("隧道已运行", "Tunnel running")}>
            {t("让朋友在多人游戏里添加服务器：", "Ask your friend to add this server in Multiplayer: ")}
            <span className="mono selectable">{status!.address}</span>
          </Notice>
        ) : (
          <span className="faint" style={{ fontSize: "var(--text-xs)" }}>
            {t("SSH 服务端需要 GatewayPorts clientspecified 并放行公网端口。关掉这个窗口隧道仍会运行，退出启动器时自动停止。", "The SSH server needs GatewayPorts clientspecified and an open public port. The tunnel keeps running when this window closes and stops when the launcher quits.")}
          </span>
        )}
        {error && (
          <Notice tone="danger" title={t("隧道出错", "Tunnel error")}>
            {error}
          </Notice>
        )}
        {status?.detail && (
          <details className="crash-excerpt" open={!running}>
            <summary>{t("SSH 输出", "SSH output")}</summary>
            <pre className="mono selectable">{status.detail}</pre>
          </details>
        )}
      </div>
    </Modal>
  );
}
