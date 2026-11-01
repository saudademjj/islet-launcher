import { motion } from "motion/react";
import { useEffect, useMemo, useState } from "react";

import type { Account, Instance } from "../api/types";
import { Icon } from "../components/Icon";
import { Modal, VirtualList } from "../components/overlay";
import { PlayerFace } from "../components/PlayerFace";
import { BrandMark } from "../components/Pixel";
import { Button, Empty, Field, Identicon, Input, Segmented, Status, Switch } from "../components/ui";
import { SceneHeader } from "../components/SceneHeader";
import { ShareLogButton } from "../components/ShareLog";
import { formatPlayTime, formatRelative, loaderLabel } from "../lib/format";
import { call } from "../api/backend";
import { usePick, useStore } from "../store";
import { t } from "../lib/i18n";

export function RecycleBin() {
  const s = usePick((s) => ({ recycled: s.recycled, fileBusy: s.fileBusy, closeModal: s.closeModal, fileOperation: s.fileOperation, open: s.open }));
  // Deleting for good takes a second click on the same button.
  const [confirm, setConfirm] = useState<string | null>(null);
  return (
    <Modal
      title={t("回收站", "Recycle bin")}
      subtitle={t("只有清空才会真正删除", "Only emptying deletes for good")}
      onClose={s.closeModal}
      footer={
        <>
          <Button variant="ghost" icon="folder" onClick={() => void s.open({ kind: "recycleDir" })} style={{ marginRight: "auto" }}>
            {t("打开目录", "Open folder")}
          </Button>
          <Button
            variant={confirm === "all" ? "danger" : "danger-ghost"}
            icon="trash"
            disabled={s.fileBusy}
            onBlur={() => setConfirm(null)}
            onClick={() => {
              if (confirm !== "all") return setConfirm("all");
              setConfirm(null);
              void s.fileOperation(null, { kind: "purgeRecycle" });
            }}
          >
            {confirm === "all" ? t("确认清空，无法撤销", "Confirm: this can't be undone") : t("清空回收站", "Empty recycle bin")}
          </Button>
          <Button onClick={s.closeModal}>{t("完成", "Done")}</Button>
        </>
      }
    >
      {s.recycled.length === 0 ? (
        <Empty icon="trash" title={t("回收站里没有世界", "No worlds in the recycle bin")}>
          {t("删除的模组、资源包、存档和截图也在回收站目录里，清空时一并删除。", "Removed mods, resource packs, saves and screenshots are in the recycle folder too and go when you empty it.")}
        </Empty>
      ) : (
        <div className="list">
          {s.recycled.map((instance) => (
            <div key={instance.id} className="list-row">
              <Identicon seed={instance.id} size={30} />
              <div className="col" style={{ flex: 1 }}>
                <span style={{ fontWeight: 600 }}>{instance.name}</span>
                <span className="mono faint" style={{ fontSize: "var(--text-xs)" }}>
                  {instance.versionId} · {loaderLabel(instance.loader)} · {formatRelative(instance.lastLaunchedAt)}
                </span>
              </div>
              <Button
                size="sm"
                variant={confirm === instance.id ? "danger" : "danger-ghost"}
                disabled={s.fileBusy}
                onBlur={() => setConfirm(null)}
                onClick={() => {
                  if (confirm !== instance.id) return setConfirm(instance.id);
                  setConfirm(null);
                  void s.fileOperation(null, { kind: "purgeRecycle", only: instance.id });
                }}
              >
                {confirm === instance.id ? t("确认永久删除", "Confirm delete forever") : t("永久删除", "Delete forever")}
              </Button>
              <Button size="sm" icon="refresh" disabled={s.fileBusy} onClick={() => void s.fileOperation(instance.id, { kind: "restoreDeleted" })}>
                {t("恢复", "Restore")}
              </Button>
            </div>
          ))}
        </div>
      )}
    </Modal>
  );
}

export function DeleteInstance({ instance }: { instance: Instance }) {
  const s = usePick((s) => ({ closeModal: s.closeModal, mutate: s.mutate }));
  return (
    <Modal
      scene={
        <SceneHeader
          seed={instance.id}
          tone="danger"
          eyebrow={t("移入回收站", "Move to recycle bin")}
          title={instance.name}
          meta={
            <>
              <span className="mono">{instance.versionId}</span>
              <span className="dot" />
              {loaderLabel(instance.loader)}
              {instance.playSeconds >= 60 && (
                <>
                  <span className="dot" />
                  {t("已玩 ", "Played ")}{formatPlayTime(instance.playSeconds)}
                </>
              )}
            </>
          }
          aside={<Identicon seed={instance.id} size={46} hero />}
          onClose={s.closeModal}
        />
      }
      onClose={s.closeModal}
      footer={
        <>
          <Button variant="ghost" onClick={s.closeModal}>
            {t("取消", "Cancel")}
          </Button>
          <Button
            variant="danger"
            icon="trash"
            onClick={async () => {
              if (await s.mutate("delete_instance", { id: instance.id }, t("已移入回收站，可随时恢复", "Moved to the recycle bin; restore it any time"))) s.closeModal();
            }}
          >
            {t("移入回收站", "Move to recycle bin")}
          </Button>
        </>
      }
    >
      <ul className="facts-list">
        <li>{t("存档、模组和设置会一起移进回收站，不会删除任何文件", "Saves, mods and settings move to the recycle bin together; nothing is deleted")}</li>
        <li>{t("随时可以在「世界」页的回收站里恢复", "Restore it any time from the recycle bin on the Worlds page")}</li>
      </ul>
    </Modal>
  );
}

export function AddOffline() {
  const s = usePick((s) => ({ closeModal: s.closeModal, mutate: s.mutate }));
  const [name, setName] = useState("");
  const value = name.trim();
  const tooLong = value.length > 16 || /\s/.test(value);
  const strict = /^[A-Za-z0-9_]{3,16}$/.test(value);
  const hint = !value
    ? t("用于游戏内显示的名字", "The name shown in game")
    : tooLong
      ? t("不能超过 16 个字符，也不能包含空格", "At most 16 characters, without spaces")
      : strict
        ? t("可以使用", "Good to go")
        : t("可以使用，但部分服务器和模组只接受 3–16 位字母、数字或下划线", "Usable, but some servers and mods accept only 3–16 letters, digits or underscores");
  const add = async () => {
    if (!value || tooLong) return;
    if (await s.mutate("add_offline_account", { username: value }, t(`已添加离线账号 ${value}`, `Added offline account ${value}`))) s.closeModal();
  };
  return (
    <Modal
      scene={
        <SceneHeader
          seed="offline-account"
          eyebrow={t("添加离线账号", "Add offline account")}
          title={value || t("新玩家", "New player")}
          aside={<PlayerFace name={value || "player"} size={48} />}
          onClose={s.closeModal}
        />
      }
      onClose={s.closeModal}
      footer={
        <>
          <Button variant="ghost" onClick={s.closeModal}>
            {t("取消", "Cancel")}
          </Button>
          <Button variant="primary" icon="plus" disabled={!value || tooLong} onClick={() => void add()}>
            {t("添加", "Add")}
          </Button>
        </>
      }
    >
      <form
        className="col gap-12"
        
        onSubmit={(e) => {
          e.preventDefault();
          void add();
        }}
      >
        <Field label={t("用户名", "Username")} hint={hint} tone={!value ? undefined : tooLong ? "bad" : strict ? "ok" : undefined}>
          <Input autoFocus value={name} invalid={tooLong} placeholder={t("3–16 位字母、数字或下划线", "3–16 letters, digits or underscores")} onChange={(e) => setName(e.target.value)} />
        </Field>
        <span className="faint" style={{ fontSize: "var(--text-xs)" }}>
          {t("离线账号无法进入开启正版验证的服务器，UUID 按 Minecraft 标准算法生成。", "Offline accounts can't join servers that verify accounts; the UUID follows Minecraft's standard algorithm.")}
        </span>
      </form>
    </Modal>
  );
}

/** Stopping a running game: first a gentle stop that lets it save, then,
    while that is under way, the choice to end it at once. */
export function StopGame({ id }: { id: string }) {
  const s = usePick((s) => ({
    instance: s.instances.find((i) => i.id === id),
    running: s.runningIds.includes(id),
    stopping: s.stopping.includes(id),
    closeModal: s.closeModal,
    stopInstance: s.stopInstance,
  }));
  // The game may close by itself while the question is open.
  useEffect(() => {
    if (s.running) return;
    const modal = useStore.getState().modal;
    if (modal?.kind === "stopGame" && modal.id === id) s.closeModal();
  }, [s.running, id]); // eslint-disable-line react-hooks/exhaustive-deps
  if (!s.instance) return null;
  const stop = (force: boolean) => {
    s.stopInstance(id, force);
    s.closeModal();
  };
  return (
    <Modal
      scene={
        <SceneHeader
          seed={id}
          tone="danger"
          eyebrow={s.stopping ? t("正在关闭", "Closing") : t("停止游戏", "Stop the game")}
          title={s.instance.name}
          meta={
            <>
              <span className="mono">{s.instance.versionId}</span>
              <span className="dot" />
              {loaderLabel(s.instance.loader)}
            </>
          }
          aside={<Identicon seed={id} size={46} hero />}
          onClose={s.closeModal}
        />
      }
      onClose={s.closeModal}
      footer={
        <>
          <Button variant="ghost" onClick={s.closeModal}>
            {s.stopping ? t("继续等待", "Keep waiting") : t("取消", "Cancel")}
          </Button>
          <Button variant="danger" icon="stop" onClick={() => stop(s.stopping)}>
            {s.stopping ? t("强制结束", "Force quit") : t("停止游戏", "Stop game")}
          </Button>
        </>
      }
    >
      {s.stopping ? (
        <ul className="facts-list">
          <li>{t("游戏正在保存并退出；存档大或模组多时，可能要等半分钟", "The game is saving and quitting; with a big save or many mods this can take half a minute")}</li>
          <li>{t("强制结束会立刻关掉游戏，没保存的进度会丢失，存档也可能损坏", "Force quitting closes the game at once: unsaved progress is lost and the save may be damaged")}</li>
        </ul>
      ) : (
        <ul className="facts-list">
          <li>{t("最稳妥的是在游戏里按 Esc，选「保存并退出」，再关掉游戏窗口", "The safest way is Esc → Save and Quit in the game, then close its window")}</li>
          <li>{t("从这里停止时，游戏会先自己保存；30 秒后还没关掉才会强制结束", "Stopping from here lets the game save first; it is only forced to close if it is still open after 30 seconds")}</li>
        </ul>
      )}
    </Modal>
  );
}

/** An explicit quit request while games, work or connections are active:
    the same night-and-red scene as stopping a game, over the current world. */
export function QuitConfirm() {
  const closeModal = useStore((s) => s.closeModal);
  const playing = useStore((s) => s.runningIds.length > 0 || s.launchPending);
  const seed = useStore((s) => s.settings.activeInstanceId ?? "launcher");
  const [waiting, setWaiting] = useState(false);
  const waitAndQuit = async () => {
    setWaiting(true);
    try { await call("quit_when_idle"); closeModal(); }
    catch { useStore.getState().toast(t("暂时无法安排退出，请重试", "Couldn't schedule quitting; try again"), "error"); }
    finally { setWaiting(false); }
  };
  return (
    <Modal
      scene={
        <SceneHeader
          seed={seed}
          tone="danger"
          eyebrow={t("退出启动器", "Quit the launcher")}
          title={playing ? t("游戏还在运行", "A game is still running") : t("还有任务没完成", "Work is still running")}
          aside={<BrandMark size={40} />}
          onClose={closeModal}
        />
      }
      onClose={closeModal}
      footer={
        <>
          <Button variant="ghost" onClick={closeModal}>
            {t("返回启动器", "Back to launcher")}
          </Button>
          <Button disabled={waiting} onClick={() => void waitAndQuit()}>
            {t("任务完成后退出", "Quit when work finishes")}
          </Button>
          {!playing && <Button variant="danger" onClick={() => void call("quit_launcher")}>
            {t("立即退出", "Quit now")}
          </Button>}
        </>
      }
    >
      {playing ? (
        <ul className="facts-list">
          <li>{t("游戏运行时，启动器要继续记录日志", "While a game runs, the launcher keeps recording its log")}</li>
          <li>{t("可以等游戏和后台任务都结束后自动退出，也可以从托盘菜单停止游戏", "Quit automatically once games and background work finish, or stop the game from the tray menu")}</li>
        </ul>
      ) : (
        <ul className="facts-list">
          <li>{t("下载、文件操作或联机连接还在进行", "Downloads, file work or multiplayer connections are still active")}</li>
          <li>{t("「任务完成后退出」会等它们结束；「立即退出」会中断任务并断开联机", "\"Quit when work finishes\" waits for them; \"Quit now\" interrupts the work and disconnects multiplayer")}</li>
        </ul>
      )}
    </Modal>
  );
}

export function DeleteAccount({ account }: { account: Account }) {
  const s = usePick((s) => ({ closeModal: s.closeModal, mutate: s.mutate }));
  const kind = account.provider === "microsoft" ? t("微软账号", "Microsoft account") : account.provider === "yggdrasil" ? (account.serverName ?? t("外置账号", "Third-party account")) : t("离线账号", "Offline account");
  return (
    <Modal
      scene={
        <SceneHeader
          seed={account.id}
          tone="danger"
          eyebrow={t("删除账号", "Remove account")}
          title={account.username}
          meta={kind}
          aside={<PlayerFace account={account} size={40} />}
          onClose={s.closeModal}
        />
      }
      onClose={s.closeModal}
      footer={
        <>
          <Button variant="ghost" onClick={s.closeModal}>
            {t("取消", "Cancel")}
          </Button>
          <Button
            variant="danger"
            icon="trash"
            onClick={async () => {
              if (await s.mutate("remove_account", { id: account.id }, t("账号已删除", "Account removed"))) s.closeModal();
            }}
          >
            {t("删除账号", "Remove account")}
          </Button>
        </>
      }
    >
      <ul className="facts-list">
        {account.provider === "offline" ? (
          <li>{t("只删除这个离线账号，之后可以随时用同一个名字再添加", "Only this offline account goes; you can add the same name again any time")}</li>
        ) : (
          <li>{t("保存在系统凭据里的登录令牌会一起删除，之后要重新登录", "Its sign-in token in the system keychain goes too; you'll need to sign in again")}</li>
        )}
        <li>{t("存档保存在各个世界里，不受影响", "Saves live in the worlds and are not affected")}</li>
      </ul>
    </Modal>
  );
}

export function LoginOverlay() {
  const s = usePick((s) => ({ auth: s.auth, busy: s.authBusy, cancel: s.cancelLogin, openAuthUrl: s.openAuthUrl, copy: s.copy }));
  const auth = s.auth;
  const link = auth?.authorizeUrl ?? auth?.verificationUri ?? "";
  const device = auth?.mode === "device";
  return (
    <Modal
      title={!auth ? t("微软账号登录", "Microsoft sign-in") : device ? t("使用设备代码登录", "Sign in with a device code") : t("微软账号登录", "Microsoft sign-in")}
      icon={<span className="login-mark"><Icon name="shield" /></span>}
      onClose={s.cancel}
      footer={
        <>
          <span className="faint" style={{ fontSize: "var(--text-xs)", marginRight: "auto", maxWidth: 330 }}>
            {t("密码只在微软官方页面输入。登录令牌保存在系统凭据存储中，不会写入配置文件或日志。", "Your password is entered only on Microsoft's own page. Sign-in tokens go to the system keychain, never to config files or logs.")}
          </span>
          <Button variant="ghost" onClick={s.cancel}>
            {t("取消登录", "Cancel sign-in")}
          </Button>
        </>
      }
    >
      {!auth ? (
        <div className="row muted" style={{ padding: "8px 0 4px" }}>
          <span className="spinner" />{t(" 正在向微软请求登录信息…", " Asking Microsoft to start sign-in…")}
        </div>
      ) : (
        <div className="col gap-16">
          <p className="muted" style={{ margin: 0 }}>
            {device ? t("在微软登录页输入下面的代码。", "Enter the code below on Microsoft's sign-in page.") : t("已在默认浏览器中打开微软登录页。", "Microsoft's sign-in page opened in your default browser.")}
          </p>
          {device && auth.userCode && (
            <motion.div className="device-code" initial={{ opacity: 0, scale: 0.96 }} animate={{ opacity: 1, scale: 1 }}>
              <span className="mono selectable">{auth.userCode}</span>
              <Button size="sm" icon="copy" onClick={() => void s.copy(auth.userCode ?? "", t("设备代码", "Device code"))}>
                {t("复制代码", "Copy code")}
              </Button>
            </motion.div>
          )}
          <div className="row">
            <Button variant="primary" icon="external" onClick={s.openAuthUrl}>
              {t("打开登录页", "Open sign-in page")}
            </Button>
            <Button icon="link" onClick={() => void s.copy(link, t("登录链接", "Sign-in link"))}>
              {t("复制链接", "Copy link")}
            </Button>
            <div className="spacer" />
            {device && auth.expiresIn ? (
              <span className="faint" style={{ fontSize: "var(--text-xs)" }}>
                {t("代码约 ", "The code is valid for about ")}{Math.round(auth.expiresIn / 60)}{t(" 分钟内有效", " minutes")}
              </span>
            ) : null}
          </div>
          <div className="row gap-12">
            <Status tone="warn" pulse>
              {t("等待中", "Waiting")}
            </Status>
            <span className="muted" style={{ fontSize: "var(--text-xs)" }}>
              {t("在浏览器中完成登录后，这里会自动继续", "This continues by itself once you finish in the browser")}
            </span>
          </div>
        </div>
      )}
    </Modal>
  );
}

type LogLevel = "all" | "warn" | "error";

const ERROR_LINE = /\[stderr\]|ERROR|Exception|FATAL/;
const WARN_LINE = /WARN/;

function lineLevel(line: string): "error" | "warn" | "info" {
  return ERROR_LINE.test(line) ? "error" : WARN_LINE.test(line) ? "warn" : "info";
}

export function LogsPanel() {
  const s = usePick((s) => ({
    logs: s.logs,
    instanceId: s.logsInstance,
    name: s.instances.find((i) => i.id === s.logsInstance)?.name ?? "",
    live: s.logsInstance !== null && (s.runningIds.includes(s.logsInstance) || s.launchPending === s.logsInstance),
    close: s.closeLogs,
    reload: s.reloadLogs,
    copy: s.copy,
    open: s.open,
    showCrashDialog: s.showCrashDialog,
    lastExit: s.logsInstance ? s.lastExits[s.logsInstance] : undefined,
  }));
  const [level, setLevel] = useState<LogLevel>("all");
  const [follow, setFollow] = useState(true);
  const [query, setQuery] = useState("");
  // Keep each line's number from the full log, so a filtered view still
  // points at the right place in the file.
  const view = useMemo(() => {
    const needle = query.trim().toLowerCase();
    let warn = 0;
    let error = 0;
    const rows: { line: string; n: number; tone: string }[] = [];
    s.logs.forEach((line, i) => {
      const kind = lineLevel(line);
      if (kind === "warn") warn++;
      if (kind === "error") error++;
      if (level === "warn" && kind === "info") return;
      if (level === "error" && kind !== "error") return;
      if (needle && !line.toLowerCase().includes(needle)) return;
      rows.push({ line, n: i + 1, tone: kind === "error" ? " err" : kind === "warn" ? " warn" : "" });
    });
    return { rows, warn, error };
  }, [s.logs, level, query]);
  const filtered = level !== "all" || query.trim() !== "";

  return (
    <Modal
      wide
      onClose={s.close}
      bodyClass="logs-body"
      scene={
        <SceneHeader
          seed={s.instanceId ?? "logs"}
          night
          eyebrow={t("日志", "Log")}
          title={s.name || t("世界", "Worlds")}
          meta={
            s.live ? (
              <Status tone="accent" pulse>
                {t("游戏运行中，输出实时更新", "The game is running; output updates live")}
              </Status>
            ) : (
              t("最近一次运行的输出", "Output of the last run")
            )
          }
          onClose={s.close}
        />
      }
      footer={
        <>
          <span className="mono faint" style={{ fontSize: "var(--text-xs)", marginRight: "auto" }}>
            {filtered ? t(`${view.rows.length} / ${s.logs.length} 行`, `${view.rows.length} / ${s.logs.length} lines`) : t(`${s.logs.length} 行`, `${s.logs.length} lines`)}
          </span>
          {!s.live && (
            <Button size="sm" variant="ghost" icon="refresh" onClick={() => void s.reload()}>
              {t("刷新", "Refresh")}
            </Button>
          )}
          {/* Errors in the last run: the crash analysis reads the same log.
              Not after a game that closed normally, whose log often has a
              few harmless errors; without that knowledge it says less. */}
          {!s.live && view.error > 0 && s.instanceId && s.lastExit !== true && (
            <Button
              size="sm"
              icon="alert"
              onClick={() => {
                const id = s.instanceId!;
                s.close();
                void s.showCrashDialog(id, null, 0);
              }}
            >
              {s.lastExit === false ? t("分析这次崩溃", "Analyze this crash") : t("分析日志里的错误", "Analyze errors in the log")}
            </Button>
          )}
          <Button size="sm" icon="copy" onClick={() => void s.copy(s.logs.join("\n"), t("日志", "Log"))}>
            {t("复制全部", "Copy all")}
          </Button>
          {s.instanceId && <ShareLogButton instanceId={s.instanceId} />}
          {s.instanceId && (
            <Button size="sm" icon="folder" onClick={() => void s.open({ kind: "logsDir", id: s.instanceId! })}>
              {t("打开目录", "Open folder")}
            </Button>
          )}
        </>
      }
    >
      <div className="logs-toolbar">
        <Segmented
          value={level}
          onChange={setLevel}
          options={[
            { value: "all", label: t("全部", "All") },
            { value: "warn", label: t(`警告 ${view.warn + view.error}`, `Warnings ${view.warn + view.error}`) },
            { value: "error", label: t(`错误 ${view.error}`, `Errors ${view.error}`) },
          ]}
        />
        <Input icon="search" placeholder={t("搜索日志", "Search log")} value={query} onChange={(e) => setQuery(e.target.value)} />
        <label className="logs-follow">
          {t("跟随最新", "Follow")}
          <Switch label={t("跟随最新输出", "Follow the latest output")} checked={follow} onChange={setFollow} />
        </label>
      </div>
      {s.logs.length === 0 ? (
        <div className="console console-empty">{t("暂无日志", "No log yet")}</div>
      ) : view.rows.length === 0 ? (
        <div className="console console-empty">{t("没有符合条件的行。", "No lines match.")}</div>
      ) : (
        <VirtualList
          className="console"
          style={{ flex: 1, minHeight: 0 }}
          items={view.rows}
          rowHeight={18}
          stickToBottom={follow}
          onPinnedChange={setFollow}
          render={(row) => {
            // "[12:00:01] [Render thread/INFO]: message": time and source are dimmed.
            const head = row.line.match(/^(\[\d{2}:\d{2}:\d{2}\])\s*(\[[^\]]*\]:?)?\s*/);
            return (
              <div className={`console-line${row.tone}`}>
                <span className="ln">{row.n}</span>
                {head ? (
                  <>
                    <span className="ts">{head[1]}</span> {head[2] && <span className="src">{head[2]} </span>}
                    {row.line.slice(head[0].length)}
                  </>
                ) : (
                  row.line
                )}
              </div>
            );
          }}
        />
      )}
    </Modal>
  );
}
