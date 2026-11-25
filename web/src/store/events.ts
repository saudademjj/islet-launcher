import type { LauncherEvent } from "../api/types";
import { t } from "../lib/i18n";
import { startQueuedInstall } from "./launchSlice";
import { MAX_LOG_LINES, storeRuntime, type SliceGet, type SliceSet } from "./types";

export function createEventHandler(set: SliceSet, get: SliceGet) {
  return function handleEvent(event: LauncherEvent) {
    const s = get();
    switch (event.type) {
      case "quitRequested":
        // Unsaved edits are asked about first, where they are; quitting
        // goes on from `resumeQuit` once they are saved or discarded.
        if (s.formGuard) set({ leaving: "quit" });
        else if (s.pageGuard) set({ leavingPage: "quit", modal: null });
        else set({ modal: { kind: "quit" } });
        break;
      case "quitScheduled":
        set({ quitScheduled: event.data.scheduled });
        break;
      case "launchRequest":
        s.launchFromShortcut(event.data.instanceId);
        break;
      case "remoteProgress":
        set({ remoteProgress: event.data });
        break;
      case "operationComplete": {
        storeRuntime.operationEnds++;
        const join = s.joinAfterFix;
        set({
          operationTask: null,
          fileBusy: false,
          filesChanged: s.filesChanged + 1,
          lastOperationOk: "Ok" in event.data,
          remoteProgress: null,
          joinAfterFix: null,
        });
        if (join && "Ok" in event.data) void s.launchInstance(join.id, undefined, join.address, join.offline, true);
        void s.refresh().then(() => {
          const { modal } = get();
          if (modal?.kind === "manage") {
            if (!get().instances.some((i) => i.id === modal.id)) set({ modal: null });
            else {
              void get().reloadLocalMods();
              void get().reloadPacks();
              // Files changed, so names and updates may have too.
              if (get().modInsights.checked) void get().inspectMods();
            }
          }
          if (modal?.kind === "recycle") void get().openRecycle();
        });
        if ("Ok" in event.data) s.toast(event.data.Ok, "success");
        else s.toast(event.data.Err.message, "error");
        break;
      }
      case "versions": {
        if ("Ok" in event.data) {
          const list = event.data.Ok;
          set({ versionsLoading: false, versions: list.versions, latest: list.latest, versionsLoaded: true, versionsFromCache: list.fromCache });
        } else {
          set({ versionsLoading: false });
          s.toast(event.data.Err.message, "error");
        }
        break;
      }
      case "javaRuntimes":
        set({ javaRuntimes: event.data, javaScanning: false });
        break;
      case "loaderVersions": {
        if (event.data.key !== s.loader.key) break;
        const r = event.data.result;
        set({ loader: { ...s.loader, loading: false, versions: "Ok" in r ? r.Ok : [], error: "Ok" in r ? null : r.Err.message } });
        break;
      }
      case "modSearch": {
        if (event.data.key !== s.modSearch.key) break;
        const r = event.data.result;
        set({
          modSearch: "Ok" in r
            ? { ...s.modSearch, loading: false, hits: r.Ok.hits, total: r.Ok.total_hits, error: null }
            : { ...s.modSearch, loading: false, hits: [], error: r.Err.message },
        });
        break;
      }
      case "modVersions": {
        if (event.data.key !== s.modVersions.key) break;
        const r = event.data.result;
        set({ modVersions: { ...s.modVersions, loading: false, versions: "Ok" in r ? r.Ok : [] } });
        if ("Err" in r) set({ modSearch: { ...get().modSearch, error: r.Err.message } });
        break;
      }
      case "storageChanged": {
        storeRuntime.operationEnds++;
        set({ fileBusy: false });
        if ("Ok" in event.data) {
          void s.refresh();
          set({ javaRuntimes: [] });
          s.scanJava();
          s.toast(t("已切换存储目录。原目录保留为备份。", "Storage folder switched. The old folder is kept as a backup."), "success");
        } else s.toast(event.data.Err.message, "error");
        break;
      }
      case "installProgress": {
        const install = s.install;
        const p = event.data;
        if (!install || install.taskId !== p.taskId) break;
        const now = performance.now();
        let { speed, sample } = install.stage === p.stage ? install : { speed: null, sample: null };
        if (!sample) sample = { at: now, bytes: p.bytes };
        else if (now - sample.at >= 900) {
          const current = Math.max(0, p.bytes - sample.bytes) / ((now - sample.at) / 1000);
          speed = speed == null ? current : speed * 0.6 + current * 0.4;
          sample = { at: now, bytes: p.bytes };
        }
        set({ install: { ...install, stage: p.stage, done: p.done, total: p.total, bytes: p.bytes, totalBytes: p.totalBytes, speed, sample } });
        break;
      }
      case "installComplete": {
        if (s.install?.taskId !== event.data.taskId) {
          if (!s.install) storeRuntime.earlyInstallEnds.set(event.data.taskId, event);
          break;
        }
        const launchNext = s.launchAfterInstall === event.data.instanceId;
        set({
          install: null,
          lastError: s.lastError?.instanceId === event.data.instanceId ? null : s.lastError,
          launchAfterInstall: null,
          installOutcome: { instanceId: event.data.instanceId, ok: true },
        });
        void s.refresh();
        if (launchNext) void s.launchInstance(event.data.instanceId);
        else s.toast(t(`${event.data.versionId} 已准备就绪`, `${event.data.versionId} is ready`), "success");
        startQueuedInstall(set, get);
        break;
      }
      case "installError": {
        if (s.install?.taskId !== event.data.taskId) {
          if (!s.install) storeRuntime.earlyInstallEnds.set(event.data.taskId, event);
          break;
        }
        set({
          install: null,
          launchAfterInstall: null,
          installOutcome: { instanceId: event.data.instanceId, ok: false },
        });
        if (event.data.error.code === "cancelled") s.toast(t("下载已取消", "Download cancelled"));
        else {
          set({ lastError: { instanceId: event.data.instanceId, message: event.data.error.message, code: event.data.error.code, detail: event.data.error.detail, what: "install" } });
          s.toast(event.data.error.message, "error");
        }
        // One world failing doesn't hold up the others in line.
        startQueuedInstall(set, get);
        break;
      }
      case "launchStarted": {
        const id = event.data.instanceId;
        set({
          launchPending: s.launchPending === id ? null : s.launchPending,
          running: id,
          runningIds: [...s.runningIds.filter((other) => other !== id), id],
          stopping: s.stopping.filter((other) => other !== id),
          launchSince: { ...s.launchSince, [id]: s.launchSince[id] ?? Date.now() },
        });
        void s.refresh();
        break;
      }
      case "launchLogs": {
        if (s.logsInstance !== event.data.instanceId) break;
        let logs = s.logs.concat(event.data.lines.map((l) => (l.stream === "stderr" ? `[stderr] ${l.line}` : l.line)));
        if (logs.length > MAX_LOG_LINES) logs = logs.slice(logs.length - MAX_LOG_LINES + 1000);
        set({ logs });
        break;
      }
      case "launchExit": {
        const id = event.data.instanceId;
        if (!s.runningIds.includes(id)) break;
        const stopped = s.stopping.includes(id);
        const since = s.launchSince[id] ?? 0;
        const runningIds = s.runningIds.filter((other) => other !== id);
        const { [id]: _ended, ...launchSince } = s.launchSince;
        set({
          running: runningIds[runningIds.length - 1] ?? null,
          runningIds,
          stopping: s.stopping.filter((other) => other !== id),
          launchSince,
          lastExits: { ...s.lastExits, [id]: stopped || event.data.success },
        });
        void s.refresh();
        if (stopped) {
          s.toast(t("游戏已停止", "Game stopped"));
          break;
        }
        if (event.data.success) {
          s.toast(t("游戏已退出", "Game closed"));
          break;
        }
        const message = event.data.code != null ? t(`游戏异常退出（代码 ${event.data.code}）`, `The game quit unexpectedly (code ${event.data.code})`) : t("游戏异常退出", "The game quit unexpectedly");
        set({ lastError: { instanceId: id, message, what: "crash" } });
        void s.showCrashDialog(event.data.instanceId, event.data.code, since);
        break;
      }
      case "launchError": {
        const id = event.data.instanceId;
        if (s.launchPending === id) set({ launchPending: null });
        const { [id]: _failed, ...launchSince } = s.launchSince;
        const { message, code, detail } = event.data.error;
        set({
          lastError: { instanceId: id, message, code, detail, what: "launch" },
          stopping: s.stopping.filter((other) => other !== id),
          launchSince,
        });
        s.toast(message, "error", s.launchFix(id, code) ?? { label: t("查看日志", "Show log"), run: () => void get().openLogs(id) });
        break;
      }
      case "accountsChanged":
        set({ accounts: event.data });
        break;
      case "loginStarted": {
        // A login cancelled while it was starting must not come back.
        if (!s.authBusy && s.loginPending !== event.data.loginId) break;
        set({ authBusy: false, loginPending: event.data.loginId, auth: event.data });
        const url = event.data.authorizeUrl ?? event.data.verificationUri;
        if (url) void s.openUrl(url);
        break;
      }
      case "loginComplete":
        if (s.loginPending === event.data.loginId || s.auth?.loginId === event.data.loginId) {
          storeRuntime.endedLogins.add(event.data.loginId);
          set({ auth: null, loginPending: null, authBusy: false, launchAfterLogin: null });
          void s.refresh();
          s.toast(t(`登录成功：${event.data.account.username}`, `Signed in as ${event.data.account.username}`), "success");
          // Signed in again because a game couldn't start: start it now.
          const relaunch = s.launchAfterLogin;
          if (relaunch && s.instances.some((i) => i.id === relaunch) && !s.runningIds.includes(relaunch)) void s.launchInstance(relaunch);
        } else if (s.authBusy && !s.loginPending) {
          storeRuntime.earlyLoginEnds.set(event.data.loginId, event);
        } else {
          // Finished as it was being cancelled: the account was stored anyway.
          void s.refresh();
        }
        break;
      case "loginError":
        if (s.loginPending === event.data.loginId || s.auth?.loginId === event.data.loginId) {
          storeRuntime.endedLogins.add(event.data.loginId);
          set({ auth: null, loginPending: null, authBusy: false, launchAfterLogin: null });
          if (event.data.error.code !== "cancelled") s.toast(event.data.error.message, "error");
        } else if (s.authBusy && !s.loginPending) {
          storeRuntime.earlyLoginEnds.set(event.data.loginId, event);
        }
        break;
    }
  };
}
