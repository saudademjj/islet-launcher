import { AnimatePresence, MotionConfig, motion } from "motion/react";
import { useEffect, useLayoutEffect, useState } from "react";

import { backend } from "./api/backend";
import type { Theme } from "./api/types";
import { TunnelDialog } from "./overlays/TunnelDialog";
import { GroupDialog } from "./overlays/GroupDialog";
import { migrateFavorites } from "./lib/prefs";
import { FileDrop } from "./components/FileDrop";
import { Tooltips } from "./components/Tooltips";
import { ContextMenuHost } from "./components/ContextMenu";
import { Toasts } from "./components/overlay";
import { Sidebar, StatusBar, WindowBar } from "./layout/Shell";
import { isMac } from "./lib/platform";
import { SaveDialog } from "./overlays/SaveDialog";
import { ConfigDialog } from "./overlays/ConfigDialog";
import { ImportDialog } from "./overlays/ImportDialog";
import { ExportPack } from "./overlays/ExportPack";
import { ExportServer } from "./overlays/ExportServer";
import { RemoteServer } from "./overlays/RemoteServer";
import { ServerModsDialog } from "./overlays/ServerModsDialog";
import { AddYggdrasil, SkinDialog } from "./overlays/AccountDialogs";
import { CommandPalette } from "./overlays/CommandPalette";
import { CrashDialog } from "./overlays/CrashDialog";
import { CreateInstance } from "./overlays/CreateInstance";
import { EnterWorld } from "./overlays/EnterWorld";
import { ModpackBrowser } from "./overlays/ModpackBrowser";
import { AddOffline, DeleteAccount, DeleteInstance, LoginOverlay, LogsPanel, QuitConfirm, RecycleBin, StopGame } from "./overlays/Dialogs";
import { ManageInstance } from "./overlays/ManageInstance";
import { ShotViewer } from "./overlays/Screenshots";
import { AccountsPage } from "./pages/AccountsPage";
import { HomePage } from "./pages/HomePage";
import { InstancesPage } from "./pages/InstancesPage";
import { MultiplayerPage } from "./pages/MultiplayerPage";
import { ServerPage } from "./pages/ServerPage";
import { useP2pPolling } from "./lib/p2p";
import { SettingsPage } from "./pages/SettingsPage";
import { overlayOpen, usePick, useStore, type Page } from "./store";

export function App() {
  const page = useStore((s) => s.page);
  const reduceMotion = useStore((s) => s.reduceMotion);
  const theme = useStore((s) => s.settings.theme);
  const covered = useStore(overlayOpen);
  const windows = useStore((s) => s.appInfo.platform === "windows");
  useTheme(theme);
  useShortcuts();
  useP2pPolling();

  useEffect(() => {
    document.documentElement.classList.toggle("reduce-motion", reduceMotion);
  }, [reduceMotion]);

  const worldCount = useStore((s) => s.instances.length);
  useEffect(() => {
    if (worldCount > 0) void migrateFavorites(useStore.getState().instances, useStore.getState().organize);
  }, [worldCount]);

  return (
    <MotionConfig reducedMotion={reduceMotion ? "always" : "user"}>
      {/* Behind an open dialog, the app takes no clicks or focus. */}
      <div className={windows ? "shell win" : "shell"} inert={covered || undefined}>
        <Sidebar />
        <main className="main">
          <div className="page-frame">
            <motion.div
              key={page}
              className="page-anim"
              initial={{ opacity: 0, y: 4 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ duration: 0.18, ease: [0.2, 0.8, 0.2, 1] }}
            >
              {page === "home" && <HomePage />}
              {page === "instances" && <InstancesPage />}
              {page === "multiplayer" && <MultiplayerPage />}
              {page === "server" && <ServerPage />}
              {page === "accounts" && <AccountsPage />}
              {page === "settings" && <SettingsPage />}
            </motion.div>
          </div>
          <StatusBar />
        </main>
      </div>
      <Overlays />
      <EnterWorld />
      <ContextMenuHost />
      <Toasts />
      <FileDrop />
      <Tooltips />
      {windows && <WindowBar />}
    </MotionConfig>
  );
}

function Overlays() {
  const s = usePick((s) => ({ modal: s.modal, logsOpen: s.logsOpen, login: s.auth !== null || s.authBusy }));
  const modal = s.modal;
  return (
    <AnimatePresence>
      {modal?.kind === "create" && <CreateInstance key="create" />}
      {modal?.kind === "manage" && <ManageInstance key={`manage-${modal.id}`} id={modal.id} tab={modal.tab} />}
      {modal?.kind === "crash" && (
        <CrashDialog key="crash" instanceId={modal.instanceId} code={modal.code} analysis={modal.analysis} />
      )}
      {modal?.kind === "recycle" && <RecycleBin key="recycle" />}
      {modal?.kind === "deleteInstance" && <DeleteInstance key="delete-instance" instance={modal.instance} />}
      {modal?.kind === "addOffline" && <AddOffline key="add-offline" />}
      {modal?.kind === "deleteAccount" && <DeleteAccount key="delete-account" account={modal.account} />}
      {modal?.kind === "quit" && <QuitConfirm key="quit" />}
      {modal?.kind === "stopGame" && <StopGame key={`stop-${modal.id}`} id={modal.id} />}
      {modal?.kind === "modpacks" && <ModpackBrowser key="modpacks" />}
      {modal?.kind === "palette" && <CommandPalette key="palette" />}
      {modal?.kind === "shot" && <ShotViewer key="shot" id={modal.id} name={modal.name} back={modal.back} />}
      {modal?.kind === "import" && <ImportDialog key="import" />}
      {modal?.kind === "exportPack" && <ExportPack key="export-pack" id={modal.id} />}
      {modal?.kind === "exportServer" && <ExportServer key="export-server" id={modal.id} />}
      {modal?.kind === "remote" && <RemoteServer key={`remote-${modal.id}`} id={modal.id} />}
      {modal?.kind === "serverMods" && <ServerModsDialog key={`server-mods-${modal.id}`} id={modal.id} address={modal.address} check={modal.check} offline={modal.offline} />}
      {modal?.kind === "addYggdrasil" && <AddYggdrasil key="add-yggdrasil" />}
      {modal?.kind === "tunnel" && <TunnelDialog key={`tunnel-${modal.id}`} id={modal.id} />}
      {modal?.kind === "group" && <GroupDialog key={`group-${modal.id}`} id={modal.id} />}
      {modal?.kind === "skin" && <SkinDialog key={`skin-${modal.accountId}`} accountId={modal.accountId} />}
      {modal?.kind === "save" && <SaveDialog key={`save-${modal.id}-${modal.folder}`} id={modal.id} folder={modal.folder} />}
      {modal?.kind === "configs" && <ConfigDialog key={`configs-${modal.id}`} id={modal.id} initial={modal.query} />}
      {s.logsOpen && <LogsPanel key="logs" />}
      {s.login && <LoginOverlay key="login" />}
    </AnimatePresence>
  );
}

/** Applies the theme before paint and keeps native materials in step. */
function useTheme(preference: Theme) {
  const query = "(prefers-color-scheme: dark)";
  const [systemDark, setSystemDark] = useState(() => window.matchMedia(query).matches);
  useEffect(() => {
    const mq = window.matchMedia(query);
    const onChange = () => setSystemDark(mq.matches);
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, []);
  const effective = preference === "system" ? (systemDark ? "dark" : "light") : preference;
  useLayoutEffect(() => {
    document.documentElement.dataset.theme = effective;
  }, [effective]);
  useEffect(() => {
    backend().setWindowTheme(preference === "system" ? null : preference);
  }, [preference]);
}

const PAGE_KEYS: Record<string, Page> = { "1": "home", "2": "instances", "3": "multiplayer", "4": "server", "5": "accounts", "6": "settings", ",": "settings" };

function useShortcuts() {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const s = useStore.getState();
      if (e.key === "Escape") {
        // Esc while composing cancels the input method's text, nothing more.
        if (e.isComposing || e.keyCode === 229) return;
        if (s.auth || s.authBusy) s.cancelLogin();
        else if (s.logsOpen) s.closeLogs();
        else if (s.modal) s.closeModal();
        else if (s.leavingPage) useStore.setState({ leavingPage: null });
        else return;
        e.preventDefault();
        return;
      }
      const mod = isMac ? e.metaKey : e.ctrlKey;
      if (!mod || e.altKey || e.shiftKey) return;
      const key = e.key.toLowerCase();
      if (PAGE_KEYS[key]) {
        if (!overlayOpen(s)) s.setPage(PAGE_KEYS[key]);
      } else if (key === "n") {
        if (!overlayOpen(s)) s.openCreate();
      } else if (key === "l") {
        s.toggleLogs();
      } else if (key === "k") {
        if (s.modal?.kind === "palette") s.closeModal();
        else if (!overlayOpen(s)) s.openModal({ kind: "palette" });
      } else return;
      e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}
