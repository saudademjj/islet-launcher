// @vitest-environment happy-dom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../api/backend", () => ({
  inTauri: false,
  call: vi.fn(async () => null),
  toError: (e: unknown) => ({ code: "error", message: String(e), detail: null }),
  loadBackend: vi.fn(),
  backend: vi.fn(),
}));

import type { CrashAnalysis } from "../api/types";
import { t } from "../lib/i18n";
import { useStore } from "../store";
import { installDomShims, makeInstance, resetStore } from "../test/dom";
import { CrashDialog } from "./CrashDialog";

describe("CrashDialog", () => {
  let openManageMock: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    installDomShims();
    openManageMock = vi.fn();
    const instance = makeInstance();
    resetStore({
      instances: [instance],
      openManage: openManageMock as unknown as ReturnType<typeof useStore.getState>["openManage"],
    });
  });

  afterEach(() => {
    cleanup();
  });

  it("routes modConflict to Mods and corruptConfig to Tools", () => {
    const analysis: CrashAnalysis = {
      excerpt: "ConfigLoadingException: Failed loading config file create-client.toml",
      diagnoses: [
        {
          kind: "modConflict",
          title: "OptiFine 与光影/优化模组冲突",
          detail: "请打开模组页移除 OptiFine。",
          fix: { kind: "openMods" },
        },
        {
          kind: "corruptConfig",
          title: "配置文件 create-client.toml 已损坏",
          detail: "请前往工具页检查配置文件。",
          fix: { kind: "openTools" },
        },
      ],
    };

    render(<CrashDialog instanceId="world-1" code={1} analysis={analysis} />);

    fireEvent.click(screen.getByRole("button", { name: t("管理模组", "Manage mods") }));
    expect(openManageMock).toHaveBeenCalledWith("world-1", "mods");

    fireEvent.click(screen.getByRole("button", { name: t("打开工具", "Open tools") }));
    expect(openManageMock).toHaveBeenCalledWith("world-1", "tools");
  });
});
