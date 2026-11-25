import { call, toError } from "../api/backend";
import type { CrashAnalysis, JavaInstallation, ServerModCheck } from "../api/types";
import { t } from "../lib/i18n";
import { beginOperation, storeRuntime, type Actions, type SliceGet, type SliceSet } from "./types";

export async function beginInstall(set: SliceSet, get: SliceGet, id: string, launch: boolean): Promise<boolean> {
  const store = get();
  storeRuntime.installStarting = true;
  try {
    const taskId = await call<string>("install_instance", { id });
    storeRuntime.installStarting = false;
    const lastError = get().lastError;
    set({
      lastError: lastError?.instanceId === id ? null : lastError,
      installOutcome: null,
      launchAfterInstall: launch ? id : null,
      install: { taskId, instanceId: id, stage: "metadata", done: 0, total: 0, bytes: 0, totalBytes: 0, speed: null, sample: null },
    });
    const early = storeRuntime.earlyInstallEnds.get(taskId);
    storeRuntime.earlyInstallEnds.clear();
    if (early) get().handleEvent(early);
    return true;
  } catch (error) {
    storeRuntime.installStarting = false;
    store.toast(toError(error).message, "error");
    // A world that can't start downloading doesn't hold up the rest.
    startQueuedInstall(set, get);
    return false;
  }
}

/** Starts the next world waiting for a download; worlds removed meanwhile are skipped. */
export function startQueuedInstall(set: SliceSet, get: SliceGet) {
  const s = get();
  if (s.install || storeRuntime.installStarting) return;
  const [next, ...rest] = s.installQueue;
  if (!next) return;
  set({ installQueue: rest });
  if (!s.instances.some((instance) => instance.id === next.id)) return startQueuedInstall(set, get);
  void beginInstall(set, get, next.id, next.launch);
}

type LaunchActionKeys =
  | "loadVersions"
  | "scanJava"
  | "scanJavaDirectory"
  | "downloadJava"
  | "primaryAction"
  | "stopInstance"
  | "installInstance"
  | "unqueueInstall"
  | "cancelInstall"
  | "launchInstance"
  | "launchFix"
  | "showCrashDialog"
  | "installJavaForInstance"
  | "setInstanceMemory"
  | "setActiveInstance"
  | "organize"
  | "startLogin"
  | "cancelLogin"
  | "openAuthUrl";

export function createLaunchSlice(set: SliceSet, get: SliceGet): Pick<Actions, LaunchActionKeys> {
  return {
    loadVersions(force = false) {
      if (get().versionsLoading) return;
      set({ versionsLoading: true });
      void call("load_versions", { force }).catch((e) => {
        set({ versionsLoading: false });
        get().toast(toError(e).message, "error");
      });
    },

    scanJava() {
      if (get().javaScanning) return;
      set({ javaScanning: true });
      void call("load_java_runtimes").catch(() => set({ javaScanning: false }));
    },

    async scanJavaDirectory() {
      const [dir] = await get().chooseFiles("java-root");
      if (!dir) return;
      set({ javaScanning: true });
      try {
        await call("scan_java_directory", { path: dir });
      } catch (error) {
        set({ javaScanning: false });
        get().toast(toError(error).message, "error");
      }
    },

    async downloadJava(major) {
      const state = await beginOperation(set, async () => {
        const task = await get().run<string>("download_java", { major });
        return task ? { task } : null;
      });
      if (state === "running") get().toast(t(`正在检测或下载 Java ${major}…`, `Finding or downloading Java ${major}…`));
    },

    primaryAction(id) {
      const s = get();
      const instance = s.instances.find((i) => i.id === id);
      if (!instance) return;
      // Stopping a game is never one click: unsaved progress is at stake.
      if (s.runningIds.includes(id)) return set({ modal: { kind: "stopGame", id } });
      // Several worlds may run at once; one launch is prepared at a time.
      if (s.install?.instanceId === id || s.launchPending) return;
      // Pressed again while waiting, it leaves the queue, the way a running game stops.
      if (s.installQueue.some((queued) => queued.id === id)) {
        s.unqueueInstall(id);
        s.toast(t("已移出下载队列", "Left the download queue"));
        return;
      }
      const bound = instance.accountId && s.accounts.some((a) => a.id === instance.accountId);
      if (!bound && !s.accounts.some((a) => a.id === s.settings.activeAccountId)) {
        s.toast(t("请先添加一个账号", "Add an account first"));
        s.setPage("accounts");
        return;
      }
      if (instance.installed) void s.launchInstance(id);
      else void s.installInstance(id, true);
    },

    stopInstance(id, force) {
      const s = get();
      if (!s.runningIds.includes(id)) return;
      // Remember this before the exit event arrives; otherwise a stop would
      // be mistaken for a crash. Clear it again if a gentle stop fails, so a
      // later real crash is still reported.
      if (!s.stopping.includes(id)) set({ stopping: [...s.stopping, id] });
      void call("stop_instance", { id, force }).then(
        () => {
          if (!force && get().stopping.includes(id)) get().toast(t("正在保存并关闭游戏…", "Saving and closing the game…"));
        },
        (error) => {
          if (!force) set({ stopping: get().stopping.filter((other) => other !== id) });
          get().toast(toError(error).message, "error");
        },
      );
    },

    async installInstance(id, launch = false) {
      const { install, installQueue } = get();
      if (install || storeRuntime.installStarting || installQueue.length > 0) {
        if (install?.instanceId === id || installQueue.some((queued) => queued.id === id)) {
          get().toast(t("这个世界已经在下载队列里", "This world is already in the download queue"));
          return false;
        }
        const ahead = installQueue.length + (install ? 1 : 0);
        set({ installQueue: [...installQueue, { id, launch }] });
        get().toast(t(`已加入下载队列，前面还有 ${ahead} 个`, `Queued behind ${ahead} other download${ahead > 1 ? "s" : ""}`));
        return false;
      }
      return beginInstall(set, get, id, launch);
    },

    unqueueInstall(id) {
      set({ installQueue: get().installQueue.filter((queued) => queued.id !== id) });
    },

    cancelInstall() {
      const install = get().install;
      if (!install) return;
      set({ launchAfterInstall: null });
      void call("cancel_task", { taskId: install.taskId });
      get().toast(t("正在取消…", "Cancelling…"));
    },

    async launchInstance(id, world, server, offline = false, skipModCheck = false) {
      // A server made with Islet lists its mods; the world gets them first.
      if (server && !skipModCheck) {
        const check = await call<ServerModCheck | null>("check_server_mods", { id, address: server }).catch(() => null);
        if (check) {
          set({ modal: { kind: "serverMods", id, address: server, check, offline } });
          return;
        }
      }
      // Set pending state before the command so a fast launchStarted or
      // launchError event cannot be overwritten by the code after the await.
      const token = get().launchToken + 1;
      const lastError = get().lastError;
      set({
        stopping: get().stopping.filter((other) => other !== id),
        launchSince: { ...get().launchSince, [id]: Date.now() },
        launchToken: token,
        lastError: lastError?.instanceId === id ? null : lastError,
        launchPending: id,
        logs: [],
        logsInstance: id,
      });
      try {
        await call("launch_instance", { id, world: world ?? null, server: server ?? null, offline });
      } catch (error) {
        // Only clear state that belongs to this attempt. If the event already
        // moved the run on, it has handled (and reported) the failure itself.
        if (get().launchToken !== token || get().launchPending !== id) return;
        const { [id]: _failed, ...launchSince } = get().launchSince;
        set({ launchPending: null, launchSince, logs: [], logsInstance: null });
        get().toast(toError(error).message, "error");
      }
    },

    launchFix(id, code) {
      if (code === "auth_offline")
        return {
          label: t("离线进入单人", "Play offline"),
          run: () => {
            get().toast(t("将以离线模式启动，仅支持单人游戏", "Starting offline for singleplayer only"));
            void get().launchInstance(id, undefined, undefined, true);
          },
        };
      if (code === "owner_required")
        return { label: t("登录微软账号", "Sign in with Microsoft"), run: () => get().setPage("accounts") };
      if (code !== "auth_expired") return null;
      return {
        label: t("重新登录", "Sign in again"),
        run: () => {
          // The account the world plays as, looked up when pressed.
          const store = get();
          const instance = store.instances.find((i) => i.id === id);
          const bound = instance?.accountId && store.accounts.some((a) => a.id === instance.accountId) ? instance.accountId : store.settings.activeAccountId;
          const account = store.accounts.find((a) => a.id === bound);
          // A Microsoft account signs in right here and the game starts after;
          // others sign in on the Accounts page, with their server's password.
          if (account?.provider !== "microsoft" || !store.loginConfigured) return store.setPage("accounts");
          set({ launchAfterLogin: id });
          void store.startLogin("browser");
        },
      };
    },

    async showCrashDialog(instanceId, code, sinceMs) {
      const token = get().launchToken;
      let analysis: CrashAnalysis = { diagnoses: [], excerpt: "" };
      try {
        analysis = await call<CrashAnalysis>("analyze_crash", { instanceId, sinceMs });
      } catch (error) {
        get().toast(toError(error).message, "error");
      }
      // Do not cover a retry or a different run that started while the
      // analysis was in flight. Every launch attempt bumps launchToken.
      if (get().launchToken !== token) return;
      if (get().runningIds.includes(instanceId) || get().launchPending === instanceId) return;
      set({ modal: { kind: "crash", instanceId, code, analysis } });
    },

    async installJavaForInstance(id, major) {
      const java = await get().run<JavaInstallation>("install_java_for_instance", { id, major });
      if (!java) return false;
      await get().refresh();
      get().toast(t(`已准备 Java ${java.major}，该世界下次启动会使用它`, `Java ${java.major} is ready; this world uses it from the next launch`), "success");
      return true;
    },

    async setInstanceMemory(id, memoryMb) {
      const instance = get().instances.find((item) => item.id === id);
      if (!instance) return false;
      return get().mutate(
        "configure_instance",
        {
          id,
          name: instance.name,
          memoryMb,
          javaPath: instance.javaPath,
          loader: instance.loader,
          loaderVersion: instance.loaderVersion,
        },
        t(`世界内存已调整为 ${memoryMb} MB`, `World memory set to ${memoryMb} MB`),
      );
    },

    setActiveInstance(id) {
      void get().mutate("set_active_instance", { id });
    },

    organize(id, favorite, group) {
      return get().mutate("organize_instance", { id, favorite, group });
    },

    async startLogin(mode) {
      const s = get();
      if (s.auth || s.loginPending || s.authBusy) return;
      set({ authBusy: true });
      const attempt = ++storeRuntime.loginAttempt;
      storeRuntime.earlyLoginEnds.clear();
      storeRuntime.endedLogins.clear();
      try {
        const loginId = await call<string>("start_login", { mode });
        if (attempt !== storeRuntime.loginAttempt) {
          // Cancelled while it was starting: stop it now that it has an id.
          void call("cancel_login", { loginId });
          return;
        }
        const early = storeRuntime.earlyLoginEnds.get(loginId);
        storeRuntime.earlyLoginEnds.clear();
        if (storeRuntime.endedLogins.has(loginId)) return;
        set({ loginPending: loginId });
        if (early) get().handleEvent(early);
      } catch (error) {
        if (attempt !== storeRuntime.loginAttempt) return;
        set({ authBusy: false });
        const e = toError(error);
        s.toast(e.message, "error");
        if (e.code === "client_id" || !s.settings.clientId) set({ page: "settings", showAzureHelp: true });
      }
    },

    cancelLogin() {
      storeRuntime.loginAttempt++;
      const s = get();
      const loginId = s.loginPending ?? s.auth?.loginId;
      if (loginId) void call("cancel_login", { loginId });
      set({ auth: null, loginPending: null, authBusy: false, launchAfterLogin: null });
    },

    openAuthUrl() {
      const auth = get().auth;
      const url = auth?.authorizeUrl ?? auth?.verificationUri;
      if (url) void get().openUrl(url);
      else get().toast(t("当前没有可打开的登录链接", "There's no sign-in link to open"));
    },
  };
}
