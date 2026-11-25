// @vitest-environment happy-dom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { CrashAnalysis } from "../api/types";

const crashFixture: CrashAnalysis = {
  excerpt: "ConfigLoadingException: Failed loading config file create-client.toml",
  diagnoses: [
    {
      kind: "corruptConfig",
      title: "配置文件 create-client.toml 已损坏",
      detail: "请前往世界管理的工具页检查或删除损坏文件。",
      fix: { kind: "openTools" },
    },
  ],
};

const callMock = vi.fn(async (command: string, _args?: Record<string, unknown>) => {
  switch (command) {
    case "launch_instance":
      return undefined;
    case "analyze_crash":
      return crashFixture;
    case "list_worlds":
      return { quickPlay: true, worlds: [] };
    case "list_servers":
    case "list_screenshots":
    case "list_mods":
      return [];
    case "mod_meta":
    case "mod_icons":
      return {};
    case "modpack_info":
    case "last_mod_change":
    case "patch_notes":
      return null;
    case "bootstrap":
      return {
        settings: useStore.getState().settings,
        accounts: useStore.getState().accounts,
        instances: useStore.getState().instances,
        appInfo: useStore.getState().appInfo,
      };
    case "set_unsaved_changes":
      return undefined;
    default:
      return null;
  }
});

vi.mock("../api/backend", () => ({
  inTauri: false,
  call: (command: string, args?: Record<string, unknown>) => callMock(command, args),
  toError: (e: unknown) => ({ code: "error", message: String(e), detail: null }),
  loadBackend: vi.fn(),
  backend: vi.fn(),
}));

import { t } from "../lib/i18n";
import { CrashDialog } from "../overlays/CrashDialog";
import { ManageInstance } from "../overlays/ManageInstance";
import { HomePage } from "../pages/HomePage";
import { installDomShims, makeInstance, resetStore } from "../test/dom";
import { useStore } from "./index";

function FlowHost() {
  const modal = useStore((s) => s.modal);
  return (
    <>
      <HomePage />
      {modal?.kind === "crash" && <CrashDialog instanceId={modal.instanceId} code={modal.code} analysis={modal.analysis} />}
      {modal?.kind === "manage" && <ManageInstance id={modal.id} tab={modal.tab} />}
    </>
  );
}

describe("Launch -> offline fallback & crash diagnosis end-to-end UI flow", () => {
  beforeEach(() => {
    installDomShims();
    callMock.mockClear();
    const instance = makeInstance({ id: "world-e2e", name: "E2E World" });
    resetStore({
      instances: [instance],
      accounts: [
        {
          id: "microsoft:owner-1",
          provider: "microsoft",
          username: "OwnerSteve",
          uuid: "00000000-0000-0000-0000-000000000001",
          xuid: null,
          ownership: true,
          skinUrl: null,
          createdAt: 0,
        },
        {
          id: "yggdrasil:demo:1",
          provider: "yggdrasil",
          username: "ThirdPartySteve",
          uuid: "00000000-0000-0000-0000-000000000002",
          xuid: null,
          ownership: null,
          skinUrl: null,
          createdAt: 0,
        },
      ],
      settings: {
        ...useStore.getState().settings,
        activeAccountId: "yggdrasil:demo:1",
        activeInstanceId: "world-e2e",
      },
    });
  });

  afterEach(() => {
    cleanup();
  });

  it("offers offline singleplayer launch on auth_offline and dispatches offline=true", async () => {
    render(<FlowHost />);

    // Initial online launch triggers launch_instance with offline=false.
    fireEvent.click(screen.getByRole("button", { name: t("开始游戏", "Play") }));
    await waitFor(() => {
      expect(callMock).toHaveBeenCalledWith("launch_instance", {
        id: "world-e2e",
        world: null,
        server: null,
        offline: false,
      });
    });

    // Backend reports unreachable sign-in server -> auth_offline.
    act(() => {
      useStore.getState().handleEvent({
        type: "launchError",
        data: {
          instanceId: "world-e2e",
          error: {
            code: "auth_offline",
            message: "连不上登录服务器。可以先以离线身份玩单人游戏，存档和进度都在；联机服务器要等网络恢复。",
            detail: "HTTP 503",
          },
        },
      });
    });

    const offlineBtn = await screen.findByRole("button", { name: t("离线进入单人", "Play offline") });
    fireEvent.click(offlineBtn);

    await waitFor(() => {
      expect(callMock).toHaveBeenCalledWith("launch_instance", {
        id: "world-e2e",
        world: null,
        server: null,
        offline: true,
      });
    });
  });

  it("opens CrashDialog on non-zero launchExit and routes corruptConfig fix straight to Tools", async () => {
    render(<FlowHost />);

    act(() => {
      useStore.getState().handleEvent({
        type: "launchStarted",
        data: { instanceId: "world-e2e", pid: 9999 },
      });
    });
    act(() => {
      useStore.getState().handleEvent({
        type: "launchExit",
        data: { instanceId: "world-e2e", code: 1, success: false },
      });
    });

    // Store invokes analyze_crash and opens CrashDialog with the diagnosis card.
    await waitFor(() => {
      expect(screen.getByText("配置文件 create-client.toml 已损坏")).toBeTruthy();
    });
    expect(callMock).toHaveBeenCalledWith("analyze_crash", expect.objectContaining({ instanceId: "world-e2e" }));

    // Clicking "Open tools" switches directly to ManageInstance on the Tools tab.
    fireEvent.click(screen.getByRole("button", { name: t("打开工具", "Open tools") }));
    await waitFor(() => {
      expect(useStore.getState().modal).toEqual({ kind: "manage", id: "world-e2e", tab: "tools" });
    });
    await waitFor(() => {
      expect(screen.getByText(t("导出为整合包", "Export as modpack"))).toBeTruthy();
    });
  });
});
