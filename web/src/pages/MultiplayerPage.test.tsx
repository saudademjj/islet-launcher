// @vitest-environment happy-dom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { LanGame, P2pStatus } from "../api/types";

let detectImpl: () => Promise<LanGame[]> = async () => [];
let statusImpl: () => Promise<P2pStatus | null> = async () => null;

const callMock = vi.fn(async (command: string, _args?: Record<string, unknown>) => {
  switch (command) {
    case "p2p_defaults":
      return { nodes: ["tcp://node1.example:11010"], downloadSize: null };
    case "p2p_detect_lan":
      return detectImpl();
    case "p2p_status":
      return statusImpl();
    case "set_unsaved_changes":
      return undefined;
    default:
      return null;
  }
});

vi.mock("../api/backend", () => ({
  inTauri: false,
  call: (command: string, args?: Record<string, unknown>) => callMock(command, args),
  toError: (e: unknown) => {
    if (e && typeof e === "object" && "message" in e) {
      return { code: (e as { code?: string }).code ?? "error", message: String((e as { message: unknown }).message), detail: null };
    }
    return { code: "error", message: String(e), detail: null };
  },
  loadBackend: vi.fn(),
  backend: vi.fn(),
}));

import { t } from "../lib/i18n";
import { useP2p } from "../lib/p2p";
import { useStore } from "../store";
import { installDomShims, makeInstance, resetStore } from "../test/dom";
import { MultiplayerPage } from "./MultiplayerPage";

describe("MultiplayerPage LAN discovery & offline host hints", () => {
  beforeEach(() => {
    installDomShims();
    callMock.mockClear();
    detectImpl = async () => [];
    statusImpl = async () => null;
    useP2p.setState({ status: null, defaults: null, busy: false, error: "", intent: null });
    resetStore({
      instances: [makeInstance()],
      accounts: [
        {
          id: "offline:steve",
          provider: "offline",
          username: "Steve",
          uuid: "00000000-0000-0000-0000-000000000001",
          xuid: null,
          ownership: null,
          skinUrl: null,
          createdAt: 0,
        },
      ],
      settings: {
        ...useStore.getState().settings,
        activeAccountId: "offline:steve",
        activeInstanceId: "world-1",
      },
    });
  });

  afterEach(() => {
    cleanup();
  });

  it("surfaces backend lan_listen error and recovers when a subsequent scan finds a local game", async () => {
    detectImpl = async () => {
      throw { code: "lan_listen", message: "无法监听局域网游戏广播（端口 4445）。可以直接填写游戏显示的端口号。" };
    };

    render(<MultiplayerPage />);

    // Backend diagnostic error is shown instead of being silently dropped.
    await waitFor(() => {
      expect(screen.getByRole("alert").textContent).toContain("4445");
    });

    // Once LAN detection succeeds on manual retry, the error clears and port autofills.
    detectImpl = async () => [{ motd: "Steve - 孤岛生存", port: 51234 }];
    fireEvent.click(screen.getByRole("button", { name: t("重新检测", "Scan again") }));

    await waitFor(() => {
      expect(screen.getByText("Steve - 孤岛生存")).toBeTruthy();
    });
    expect(screen.queryByRole("alert")).toBeNull();
    expect((screen.getByLabelText(t("游戏端口", "Game port")) as HTMLInputElement).value).toBe("51234");
  });

  it("shows CustomLAN / online-mode hint when hosting a room with an offline account", () => {
    useP2p.setState({
      status: {
        active: true,
        preparing: false,
        download: null,
        role: "host",
        code: "U/AAAA-BBBB-CCCC-DDDD",
        hostname: "Steve",
        gamePort: 51234,
        address: null,
        uptimeSeconds: 12,
        health: {
          level: "good",
          summary: "房间已就绪，等待玩家加入",
          checks: [{ id: "game", level: "good", label: "游戏世界", detail: "已对局域网开放（端口 51234）" }],
          players: [],
          ip: "10.126.126.1",
        },
        world: null,
        members: [],
        notice: null,
      },
    });

    render(<MultiplayerPage />);
    expect(screen.getByText(/CustomLAN/)).toBeTruthy();
    expect(screen.getByText("U/AAAA-BBBB-CCCC-DDDD")).toBeTruthy();
  });
});
