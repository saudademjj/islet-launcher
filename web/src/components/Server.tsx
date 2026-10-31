import type { ReactNode } from "react";

import type { Instance, MotdSpan, ServerEntry } from "../api/types";
import { formatCount } from "../lib/format";
import { signalBars, usePing, type Ping } from "../lib/servers";
import { instanceState, usePick } from "../store";
import { Button, Identicon } from "./ui";
import { t } from "../lib/i18n";

/** The server's message of the day in its own chat colors. */
export function Motd({ spans }: { spans: MotdSpan[] }) {
  return (
    <span className="motd">
      {spans.map((span, i) => {
        const decoration = [span.underlined && "underline", span.strikethrough && "line-through"].filter(Boolean).join(" ");
        return (
          <span
            key={i}
            style={{
              color: span.color ?? undefined,
              fontWeight: span.bold ? 700 : undefined,
              fontStyle: span.italic ? "italic" : undefined,
              textDecoration: decoration || undefined,
            }}
          >
            {span.obfuscated ? span.text.replace(/\S/g, "▒") : span.text}
          </span>
        );
      })}
    </span>
  );
}

/** Five pixel bars like the game's server list; a red cross when offline. */
export function Signal({ ping }: { ping: Ping | null }) {
  const bars = signalBars(ping);
  const state = ping?.loading && !ping.status ? "scanning" : ping?.error ? "offline" : "online";
  return (
    <span className={`signal ${state}`} aria-hidden>
      {[1, 2, 3, 4, 5].map((n) => (
        <i key={n} className={n <= bars ? "on" : undefined} style={{ height: 3 + n * 2 }} />
      ))}
    </span>
  );
}

function statusLine(ping: Ping | null): ReactNode {
  if (!ping || (ping.loading && !ping.status)) return <span className="server-note">{t("正在连接…", "Connecting…")}</span>;
  if (ping.error) return <span className="server-note offline">{ping.error}</span>;
  return null;
}

/** One multiplayer server as a dark slot, the way the game lists them. */
export function ServerCard({ instance, server, compact, actions }: { instance: Instance; server: ServerEntry; compact?: boolean; actions?: ReactNode }) {
  const ping = usePing(server.address);
  const status = ping?.status ?? null;
  const icon = status?.favicon ?? server.icon;
  const players = status?.players.length ? t(`在线：${status.players.join("、")}`, `Online: ${status.players.join(", ")}`) : undefined;
  return (
    <div className={`server${compact ? " compact" : ""}${ping?.error ? " is-offline" : ""}`}>
      <span className="server-icon">{icon ? <img src={icon} alt="" /> : <Identicon seed={server.address} size={compact ? 40 : 48} hero />}</span>
      <div className="server-main">
        <div className="server-head">
          <span className="server-name truncate">{server.name || server.address}</span>
          <span className="server-ping" title={status ? t(`延迟 ${status.latencyMs} ms`, `Ping ${status.latencyMs} ms`) : undefined}>
            {status && <span className="mono">{status.latencyMs} ms</span>}
            <Signal ping={ping} />
          </span>
        </div>
        <div className="server-motd">{status ? <Motd spans={status.motd.length ? status.motd : [{ text: "A Minecraft Server", color: null, bold: false, italic: false, underlined: false, strikethrough: false, obfuscated: false }]} /> : statusLine(ping)}</div>
        <div className="server-foot">
          <span className="mono truncate">{server.address}</span>
          {status?.version && !compact && (
            <>
              <span className="dot" />
              <span className="truncate">{status.version}</span>
            </>
          )}
          {status && (
            <span className="server-players mono" title={players}>
              <i />
              {formatCount(status.online)} / {formatCount(status.max)}
            </span>
          )}
        </div>
      </div>
      {actions ?? <JoinButton instance={instance} server={server} />}
    </div>
  );
}

/** Starts the game straight into the server. */
export function JoinButton({ instance, server, onJoin, size = "sm" }: { instance: Instance; server: ServerEntry; onJoin?: () => void; size?: "sm" | "md" }) {
  const s = usePick((s) => ({
    account: s.settings.activeAccountId,
    busy: s.runningIds.includes(instance.id) || s.launchPending != null || s.install?.instanceId === instance.id,
    install: s.install,
    installQueue: s.installQueue,
    running: s.running,
    runningIds: s.runningIds,
    launchPending: s.launchPending,
    launch: s.launchInstance,
  }));
  const state = instanceState(instance, s);
  const reason = !instance.installed ? t("先安装这个世界", "Install this world first") : !s.account ? t("先添加账号", "Add an account first") : state === "running" ? t("游戏正在运行", "The game is running") : s.busy ? t("请等当前任务结束", "Wait for the current task to finish") : t(`开始游戏并加入 ${server.address}`, `Start the game and join ${server.address}`);
  return (
    <Button
      size={size}
      variant="go"
      icon="play"
      className="server-join"
      disabled={!instance.installed || !s.account || s.busy}
      title={reason}
      onClick={() => {
        onJoin?.();
        void s.launch(instance.id, undefined, server.address);
      }}
    >
      {t("加入", "Join")}
    </Button>
  );
}
