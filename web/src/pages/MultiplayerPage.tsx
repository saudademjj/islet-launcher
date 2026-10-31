import { AnimatePresence, motion } from "motion/react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { useShallow } from "zustand/react/shallow";

import type { LanGame, P2pLevel, P2pStatus } from "../api/types";
import { Icon } from "../components/Icon";
import { Landscape, PX } from "../components/Pixel";
import { PlayerFace } from "../components/PlayerFace";
import { BlockProgress, Button, Input, Notice, SectionTitle } from "../components/ui";
import { Toolbar } from "../layout/Shell";
import { formatBytes } from "../lib/format";
import { t } from "../lib/i18n";
import { item, stagger } from "../lib/motion";
import { codeComplete, formatCode, LEVELS, loadNodes, saveNodes, useP2p, validNode } from "../lib/p2p";
import { instanceState, useActiveAccount, useActiveInstance, usePick, useStore } from "../store";

/** Focuses `ref` when a command asked for this card, then clears the ask. */
function useIntent(card: "host" | "join", ref: React.RefObject<HTMLInputElement | null>) {
  const intent = useP2p((s) => s.intent);
  useEffect(() => {
    if (intent !== card) return;
    useP2p.setState({ intent: null });
    const el = ref.current;
    if (el) {
      el.focus();
      el.scrollIntoView({ block: "center" });
    }
  }, [intent, card, ref]);
}

const isWindows = typeof navigator !== "undefined" && /Win/i.test(navigator.platform || navigator.userAgent);

export function MultiplayerPage() {
  const s = useP2p(useShallow((s) => ({ status: s.status, busy: s.busy, error: s.error, defaults: s.defaults, stop: s.stop, clearError: s.clearError, loadDefaults: s.loadDefaults })));
  const [advanced, setAdvanced] = useState(false);
  useEffect(() => {
    void s.loadDefaults();
  }, [s.loadDefaults]);
  const status = s.status;
  const live = !!status?.active;
  const preparing = !live && (s.busy || !!status?.preparing);
  const level = status?.health?.level;

  return (
    <>
      <Toolbar
        title={t("联机", "Multiplayer")}
        meta={live && level ? `${status?.role === "host" ? t("房主", "Host") : t("访客", "Guest")} · ${LEVELS[level].label}` : t("P2P 直连，无需公网 IP 和虚拟网卡", "Peer-to-peer, no public IP or virtual adapter needed")}
      >
        {(live || preparing) && (
          <Button variant="danger-ghost" icon="close" onClick={() => void s.stop()}>
            {live ? t("退出房间", "Leave room") : t("取消", "Cancel")}
          </Button>
        )}
      </Toolbar>
      <div className="content">
        <motion.div className="page narrow" variants={stagger} initial="initial" animate="animate">
          <AnimatePresence initial={false}>
            {s.error && (
              <motion.div key="error" variants={item} initial="initial" animate="animate" exit={{ opacity: 0 }} style={{ marginBottom: 16 }}>
                <Notice tone="danger" title={t("没能进入房间", "Couldn't open the room")} actions={<Button size="sm" variant="ghost" icon="close" aria-label={t("关闭", "Dismiss")} onClick={s.clearError} />}>
                  {s.error}
                </Notice>
              </motion.div>
            )}
          </AnimatePresence>
          {live && status ? <RoomView status={status} onNodes={() => setAdvanced(true)} /> : preparing ? <Preparing status={status} /> : <Start downloadSize={s.defaults?.downloadSize ?? null} />}
          <Advanced open={advanced} onToggle={setAdvanced} defaults={s.defaults?.nodes ?? []} />
        </motion.div>
      </div>
    </>
  );
}

// ----- the forest panel -------------------------------------------------------

/** A square of light whose colour is the room's health. */
// A hanging lantern on the 3 px grid, 7 × 8 cells: iron frame, light inside.
const LANTERN_FRAME: [number, number, number, number][] = [
  [3, 0, 1, 1],
  [2, 1, 3, 1],
  [1, 2, 5, 1],
  [1, 3, 1, 3],
  [5, 3, 1, 3],
  [1, 6, 5, 1],
  [2, 7, 1, 1],
  [4, 7, 1, 1],
];

function Lamp({ level }: { level: P2pLevel }) {
  return (
    <svg className={`lamp lamp-${level}`} viewBox="0 0 7 8" width={7 * PX} height={8 * PX} shapeRendering="crispEdges" role="img" aria-label={LEVELS[level].label}>
      {LANTERN_FRAME.map(([x, y, w, h]) => (
        <rect key={`${x}-${y}`} className="lamp-frame" x={x} y={y} width={w} height={h} />
      ))}
      <rect className="lamp-light" x={2} y={3} width={3} height={3} />
      <rect className="lamp-shine" x={3} y={4} width={1} height={1} />
    </svg>
  );
}

/** This computer and the other side, joined by a row of blocks that is
    whole, patchy, gapped or broken with the connection. */
function Bridge({ level, left, right }: { level: P2pLevel | "none"; left: ReactNode; right: ReactNode }) {
  return (
    <div className={`bridge bridge-${level}`} aria-hidden>
      <span className="bridge-end">{left}</span>
      <span className="bridge-span">
        {Array.from({ length: 12 }, (_, i) => (
          <span key={i} className="bridge-cell" style={{ animationDelay: `${i * 90}ms` }} />
        ))}
      </span>
      <span className="bridge-end">{right}</span>
    </div>
  );
}

function RoomHero({ seed, children }: { seed: string; children: ReactNode }) {
  return (
    <motion.section className="room-hero" variants={item}>
      <Landscape seed={seed} unit={PX} cols={90} ground={[1, 3]} sky={7} treesFrom={60} treeChance={0.3} far={[3, 6]} />
      {children}
    </motion.section>
  );
}

function uptime(seconds: number) {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const sec = String(seconds % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
}

// ----- not in a room ----------------------------------------------------------

function Start({ downloadSize }: { downloadSize: number | null }) {
  const account = useActiveAccount();
  const name = account?.username ?? "";
  return (
    <>
      <RoomHero seed="p2p-start">
        <div className="room-hero-body">
          <span className="room-eyebrow">{t("P2P 联机", "P2P MULTIPLAYER")}</span>
          <h2 className="room-title">{t("和好友一起玩，不用公网 IP", "Play with friends, no public IP needed")}</h2>
          <p className="room-text">
            {t(
              "房主把世界「对局域网开放」，好友输入房间码就能加入。能直连时走直连，不能时由共享节点中转，全程加密。",
              "The host opens a world to LAN; friends join with the room code. Connections go direct when they can and through shared nodes when they can't, encrypted either way.",
            )}
          </p>
          <Bridge level="none" left={<PlayerFace account={account} size={32} />} right={<span className="bridge-ghost">?</span>} />
        </div>
      </RoomHero>
      <motion.div className="room-start" variants={item}>
        <HostCard name={name} />
        <JoinCard name={name} />
      </motion.div>
      <motion.div className="col" variants={item} style={{ gap: 10, marginTop: 16 }}>
        {downloadSize != null && (
          <Notice icon="download" title={t(`首次联机会下载约 ${formatBytes(downloadSize)} 的联机组件`, `The first room downloads about ${formatBytes(downloadSize)} of networking`)}>
            {t("EasyTier 开源组网核心（LGPL-3.0），从官方发布页下载并校验，只需下载一次。", "EasyTier, an open-source networking core (LGPL-3.0), downloaded from its official releases and verified, once.")}
          </Notice>
        )}
        {isWindows && (
          <p className="room-hint">
            <Icon name="shield" size={13} />
            {t("Windows 可能弹出防火墙提示，请选「允许」；否则只能通过中转连接，延迟会高一些。", "Windows may ask about the firewall; choose Allow, or connections can only go through a relay, with more delay.")}
          </p>
        )}
      </motion.div>
    </>
  );
}

/** How often an empty LAN list looks again. */
const RESCAN_MS = 4000;

function HostCard({ name }: { name: string }) {
  const host = useP2p((s) => s.host);
  const detect = useP2p((s) => s.detect);
  const busy = useP2p((s) => s.busy);
  const [games, setGames] = useState<LanGame[] | null>(null);
  const [scanning, setScanning] = useState(false);
  const [port, setPort] = useState("");
  const portRef = useRef<HTMLInputElement>(null);
  useIntent("host", portRef);
  const world = useActiveInstance();
  const game = usePick((s) => ({ runningIds: s.runningIds, launchPending: s.launchPending, install: s.install, installQueue: s.installQueue }));
  const worldState = world ? instanceState(world, game) : null;
  const live = useRef(true);
  const inFlight = useRef(false);
  // A background scan doesn't spin the button or blank the list.
  const scan = async (quiet = false) => {
    if (inFlight.current) return;
    inFlight.current = true;
    if (!quiet) setScanning(true);
    try {
      const found = await detect();
      if (!live.current) return;
      setGames(found);
      if (found.length) setPort((current) => current || String(found[0].port));
    } catch {
      if (live.current && !quiet) setGames([]);
    } finally {
      inFlight.current = false;
      if (live.current) setScanning(false);
    }
  };
  useEffect(() => {
    live.current = true;
    void scan();
    return () => {
      live.current = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  // Coming back from the game, where the world was just opened to LAN,
  // finds it without a click.
  useEffect(() => {
    const back = () => document.visibilityState === "visible" && void scan(true);
    window.addEventListener("focus", back);
    document.addEventListener("visibilitychange", back);
    return () => {
      window.removeEventListener("focus", back);
      document.removeEventListener("visibilitychange", back);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  // While nothing is found, look again every few seconds, only while the
  // launcher is on screen.
  const empty = games !== null && games.length === 0;
  useEffect(() => {
    if (!empty) return;
    const timer = window.setInterval(() => document.visibilityState === "visible" && void scan(true), RESCAN_MS);
    return () => window.clearInterval(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [empty]);
  const valid = /^\d{1,5}$/.test(port) && Number(port) > 0 && Number(port) < 65536;
  return (
    <section className="room-card">
      <div className="room-card-head">
        <span className="room-card-index">1</span>
        <div className="col" style={{ gap: 2 }}>
          <span className="room-card-title">{t("开房间", "Host a room")}</span>
          <span className="room-card-text">{t("在游戏里按 Esc →「对局域网开放」，再回来选中它。", "In the game, press Esc → Open to LAN, then pick it here.")}</span>
        </div>
      </div>
      <div className="lan-list">
        {scanning && !games ? (
          <div className="lan-game skeleton" aria-hidden />
        ) : games?.length ? (
          games.map((game) => (
            <button key={game.port} type="button" className="lan-game" aria-pressed={port === String(game.port)} onClick={() => setPort(String(game.port))}>
              <Icon name={port === String(game.port) ? "check" : "monitor"} size={14} />
              <span className="truncate" style={{ flex: 1 }}>{game.motd}</span>
              <span className="mono faint">:{game.port}</span>
            </button>
          ))
        ) : worldState === "running" || worldState === "starting" ? (
          <div className="lan-empty waiting">
            <span className="spinner" />
            {t("游戏开着：按 Esc →「对局域网开放」，这里会自动出现", "The game is open: press Esc → Open to LAN and it shows up here")}
          </div>
        ) : (
          <div className="lan-empty start">
            <span>{t("还没开始游戏", "The game isn't running")}</span>
            {world && worldState !== "installing" && worldState !== "queued" && (
              <Button size="sm" variant="go" icon="play" onClick={() => useStore.getState().primaryAction(world.id)}>
                {worldState === "notInstalled" ? t(`安装并开始 ${world.name}`, `Install and play ${world.name}`) : t(`开始 ${world.name}`, `Play ${world.name}`)}
              </Button>
            )}
          </div>
        )}
      </div>
      <div className="row" style={{ gap: 8 }}>
        <Input ref={portRef} mono value={port} inputMode="numeric" placeholder={t("端口", "Port")} aria-label={t("游戏端口", "Game port")} style={{ width: 110 }} onChange={(e) => setPort(e.target.value.replace(/\D/g, "").slice(0, 5))} />
        <Button size="sm" variant="ghost" icon="refresh" busy={scanning} onClick={() => void scan()}>
          {t("重新检测", "Scan again")}
        </Button>
        <div className="spacer" />
        <Button variant="primary" icon="play" busy={busy} disabled={!valid} onClick={() => void host(Number(port), name)}>
          {t("创建房间", "Create room")}
        </Button>
      </div>
    </section>
  );
}

function JoinCard({ name }: { name: string }) {
  const join = useP2p((s) => s.join);
  const busy = useP2p((s) => s.busy);
  const [code, setCode] = useState("");
  const codeRef = useRef<HTMLInputElement>(null);
  useIntent("join", codeRef);
  const ready = codeComplete(code);
  const go = () => ready && !busy && void join(code, name);
  return (
    <section className="room-card">
      <div className="room-card-head">
        <span className="room-card-index">2</span>
        <div className="col" style={{ gap: 2 }}>
          <span className="room-card-title">{t("加入房间", "Join a room")}</span>
          <span className="room-card-text">{t("向房主要房间码，粘贴到这里。HMCL、PCL CE 等启动器开的房间也能加入。", "Ask the host for the room code and paste it here. Rooms opened in HMCL, PCL CE and similar launchers work too.")}</span>
        </div>
      </div>
      <Input
        ref={codeRef}
        mono
        className="code-input"
        value={code}
        placeholder="U/XXXX-XXXX-XXXX-XXXX"
        aria-label={t("房间码", "Room code")}
        onChange={(e) => setCode(formatCode(e.target.value))}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.nativeEvent.isComposing) go();
        }}
      />
      <div className="row" style={{ gap: 8 }}>
        <span className="room-card-text">{t("加入后，房间会出现在游戏的局域网世界列表里。", "Once joined, the room shows up in the game's LAN worlds.")}</span>
        <div className="spacer" />
        <Button variant="primary" icon="link" busy={busy} disabled={!ready} onClick={go}>
          {t("加入", "Join")}
        </Button>
      </div>
    </section>
  );
}

// ----- setting up -------------------------------------------------------------

function Preparing({ status }: { status: P2pStatus | null }) {
  const download = status?.download;
  return (
    <RoomHero seed="p2p-start">
      <div className="room-hero-body">
        <div className="row" style={{ gap: 12 }}>
          <Lamp level="starting" />
          <span className="room-eyebrow">{LEVELS.starting.label}</span>
        </div>
        <h2 className="room-title">{download ? t("正在下载联机组件…", "Downloading the networking component…") : t("正在准备房间…", "Setting up the room…")}</h2>
        {download && (
          <div className="col" style={{ gap: 8, maxWidth: 420 }}>
            <BlockProgress ratio={download.total ? download.done / download.total : 0} />
            <span className="room-text mono">
              {formatBytes(download.done)} / {formatBytes(download.total)}
            </span>
          </div>
        )}
      </div>
    </RoomHero>
  );
}

// ----- in a room --------------------------------------------------------------

function RoomView({ status, onNodes }: { status: P2pStatus; onNodes: () => void }) {
  const account = useActiveAccount();
  const copy = useStore((s) => s.copy);
  const health = status.health;
  const level = health?.level ?? "starting";
  const host = status.role === "host";
  const others = health?.players ?? [];
  // Scaffolding rooms say which launcher each player uses.
  const vendorOf = (name: string) => status.members.find((m) => !m.me && m.name.toLowerCase() === name.toLowerCase())?.vendor;
  const peer = host ? others[0] : others.find((p) => p.host);
  const right = peer ? <PlayerFace name={peer.name} size={32} /> : <span className="bridge-ghost">{host ? "…" : "?"}</span>;
  return (
    <>
      <RoomHero seed={status.code ?? "p2p"}>
        <div className="room-hero-body">
          <div className="row" style={{ gap: 12 }}>
            <Lamp level={level} />
            <span className="room-eyebrow">{LEVELS[level].label}</span>
            <div className="spacer" />
            <span className="room-eyebrow" title={t("房间已开启", "Room open for")}>
              {uptime(status.uptimeSeconds)}
            </span>
          </div>
          <h2 className="room-title" aria-live="polite">
            {health?.summary || LEVELS[level].label}
          </h2>
          <Bridge level={level} left={<PlayerFace account={account} size={32} />} right={right} />
          {host && status.code ? (
            <div className="room-code-row">
              <div className="col" style={{ gap: 4, minWidth: 0 }}>
                <span className="room-eyebrow">{t("房间码", "ROOM CODE")}</span>
                <span className="room-code selectable">{status.code}</span>
              </div>
              <Button variant="hero" icon="copy" onClick={() => void copy(status.code!, t("房间码", "Room code"))}>
                {t("复制房间码", "Copy code")}
              </Button>
            </div>
          ) : status.address ? (
            <div className="room-code-row">
              <div className="col" style={{ gap: 4, minWidth: 0 }}>
                <span className="room-eyebrow">{t("游戏里连接", "JOIN IN GAME")}</span>
                <span className="room-code selectable">{status.address}</span>
              </div>
              <Button variant="hero" icon="copy" onClick={() => void copy(status.address!, t("地址", "Address"))}>
                {t("复制地址", "Copy address")}
              </Button>
            </div>
          ) : null}
        </div>
      </RoomHero>
      {!host && status.notice && (
        <motion.p className="room-hint" variants={item}>
          <Icon name="alert" size={13} />
          {status.notice}
        </motion.p>
      )}
      {!host && (
        <motion.p className="room-hint" variants={item}>
          <Icon name="info" size={13} />
          {t(
            `在游戏的「多人游戏」里，局域网世界会出现${status.world?.motd ? `「${status.world.motd} · P2P」` : "这个房间"}；也可以「直接连接」上面的地址。`,
            `In the game's Multiplayer screen the room shows up as a LAN world${status.world?.motd ? ` ("${status.world.motd} · P2P")` : ""}; or use Direct Connection with the address above.`,
          )}
        </motion.p>
      )}
      <motion.div className="room-grid" variants={item}>
        <section>
          <SectionTitle>{t("网络健康度诊断", "Network health")}</SectionTitle>
          <ul className="checks">
            {health?.checks.map((check) => (
              <li key={check.id} className={`check level-${check.level}`}>
                <span className="check-mark" aria-hidden />
                <div className="col" style={{ gap: 2, minWidth: 0, flex: 1 }}>
                  <span className="check-label">{check.label}</span>
                  <span className="check-detail">{check.detail}</span>
                </div>
                {check.id === "nodes" && (check.level === "poor" || check.level === "fair") && (
                  <Button size="sm" variant="ghost" onClick={onNodes}>
                    {t("换节点", "Change nodes")}
                  </Button>
                )}
              </li>
            ))}
          </ul>
        </section>
        <section>
          <SectionTitle trailing={<span className="tab-count">{others.length + 1}</span>}>{t("房间里的人", "In the room")}</SectionTitle>
          <ul className="players">
            <li className="player">
              <PlayerFace account={account} size={32} />
              <div className="col" style={{ gap: 2, minWidth: 0, flex: 1 }}>
                <span className="player-name truncate">
                  {status.hostname} <span className="chip">{t("你", "You")}</span>
                </span>
                <span className="check-detail mono">{health?.ip ?? t("正在分配地址…", "Getting an address…")}</span>
              </div>
            </li>
            {others.map((player) => (
              <li key={player.ip || player.name} className={`player level-${player.level}`}>
                <PlayerFace name={player.name} size={32} />
                <div className="col" style={{ gap: 2, minWidth: 0, flex: 1 }}>
                  <span className="player-name truncate">
                    {player.name} {player.host && <span className="chip">{t("房主", "Host")}</span>}
                  </span>
                  <span className="check-detail">
                    {player.detail}
                    {vendorOf(player.name) && ` · ${vendorOf(player.name)}`}
                  </span>
                </div>
                <span className="check-mark" aria-hidden />
              </li>
            ))}
            {others.length === 0 && <li className="players-empty">{host ? t("把房间码发给好友，他们加入后会出现在这里。", "Send friends the room code; they appear here once they join.") : t("正在寻找房主…", "Looking for the host…")}</li>}
          </ul>
          {status.world && (
            <div className="room-world">
              <span className="truncate" style={{ flex: 1 }}>{status.world.motd}</span>
              <span className="mono faint">{status.world.version}</span>
              <span className="mono">
                {status.world.online}/{status.world.max}
              </span>
            </div>
          )}
        </section>
      </motion.div>
    </>
  );
}

// ----- advanced ---------------------------------------------------------------

function Advanced({ open, onToggle, defaults }: { open: boolean; onToggle: (open: boolean) => void; defaults: string[] }) {
  const [text, setText] = useState(() => (loadNodes() ?? []).join("\n"));
  const [saved, setSaved] = useState(false);
  const lines = text.split("\n").map((l) => l.trim()).filter(Boolean);
  const invalid = lines.filter((l) => !validNode(l));
  const custom = loadNodes() !== null;
  const activeId = useStore((s) => s.settings.activeInstanceId);
  return (
    <motion.details className="room-advanced" variants={item} open={open} onToggle={(e) => onToggle((e.currentTarget as HTMLDetailsElement).open)}>
      <summary>
        {t("高级：共享节点、自己的服务器", "Advanced: shared nodes, your own server")}
        <span className="faint">{custom ? t(`自定义 ${loadNodes()!.length} 个`, `${loadNodes()!.length} custom`) : t(`内置 ${defaults.length} 个`, `${defaults.length} built in`)}</span>
      </summary>
      <p className="room-card-text">
        {t(
          "共享节点只负责让双方互相找到，以及不能直连时转发加密数据。双方至少要有一个相同的节点。每行一个地址，例如 tcp://example.com:11010；留空就用内置列表。",
          "Shared nodes only introduce the two sides and relay encrypted traffic when a direct link fails. Both sides need at least one node in common. One address per line, e.g. tcp://example.com:11010; leave empty for the built-in list.",
        )}
      </p>
      <textarea
        className="input mono room-nodes"
        rows={5}
        value={text}
        spellCheck={false}
        placeholder={defaults.join("\n")}
        onChange={(e) => {
          setText(e.target.value);
          setSaved(false);
        }}
      />
      <div className="row" style={{ gap: 8 }}>
        {invalid.length > 0 && <span className="field-hint bad">{t(`有 ${invalid.length} 行格式不对，会被忽略`, `${invalid.length} line(s) aren't valid and will be ignored`)}</span>}
        {saved && <span className="field-hint ok">{t("已保存，下次开房间或加入时生效", "Saved; used from the next room")}</span>}
        <div className="spacer" />
        <Button
          size="sm"
          variant="ghost"
          onClick={() => {
            setText("");
            saveNodes(null);
            setSaved(true);
          }}
        >
          {t("恢复默认", "Restore defaults")}
        </Button>
        <Button
          size="sm"
          onClick={() => {
            saveNodes(lines.filter(validNode));
            setSaved(true);
          }}
        >
          {t("保存", "Save")}
        </Button>
      </div>
      {/* The older way, for players with a server of their own. */}
      {activeId && (
        <div className="room-advanced-alt">
          <div className="col" style={{ gap: 2, flex: 1, minWidth: 0 }}>
            <span className="room-advanced-title">{t("用自己的服务器转发", "Forward through your own server")}</span>
            <span className="room-card-text">
              {t("有公网服务器时，可以用 SSH 反向隧道把当前世界开到服务器上，好友直接连服务器地址，不需要房间码。", "With a public server of your own, an SSH reverse tunnel puts the current world on it; friends connect to the server's address, no room code needed.")}
            </span>
          </div>
          <Button size="sm" onClick={() => useStore.getState().openModal({ kind: "tunnel", id: activeId })}>
            {t("SSH 隧道", "SSH tunnel")}
          </Button>
        </div>
      )}
    </motion.details>
  );
}
