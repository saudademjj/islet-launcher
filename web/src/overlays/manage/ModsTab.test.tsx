// @vitest-environment happy-dom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { LocalMod, ModMeta } from "../../api/types";

let mockMetas: Record<string, ModMeta> = {};
const callMock = vi.fn(async (command: string, _args?: Record<string, unknown>) => {
  if (command === "mod_meta") return mockMetas;
  if (command === "last_mod_change") return null;
  if (command === "mod_icons") return {};
  if (command === "set_unsaved_changes") return undefined;
  return null;
});

vi.mock("../../api/backend", () => ({
  inTauri: false,
  call: (command: string, args?: Record<string, unknown>) => callMock(command, args),
  toError: (e: unknown) => ({ code: "error", message: String(e), detail: null }),
  loadBackend: vi.fn(),
  backend: vi.fn(),
}));

import { t } from "../../lib/i18n";
import { useStore } from "../../store";
import { installDomShims, makeInstance, resetStore } from "../../test/dom";
import { ModsTab } from "./ModsTab";

describe("ModsTab", () => {
  let shims: ReturnType<typeof installDomShims>;
  let fileOperationMock: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    shims = installDomShims();
    callMock.mockClear();
    mockMetas = {};
    fileOperationMock = vi.fn(async () => {});
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  function setupMods(mods: LocalMod[], metas: Record<string, ModMeta> = {}) {
    mockMetas = metas;
    const instance = makeInstance();
    resetStore({
      instances: [instance],
      localMods: mods,
      modInsights: { id: instance.id, checked: true, loading: false, error: null, byFile: {} },
      fileOperation: fileOperationMock as unknown as ReturnType<typeof useStore.getState>["fileOperation"],
    });
    return instance;
  }

  it("virtualizes a 200-mod pack and filters by search query", async () => {
    const mods: LocalMod[] = Array.from({ length: 200 }, (_, i) => ({
      name: `mod-${String(i).padStart(3, "0")}.jar`,
      enabled: true,
      bytes: 120_000,
    }));
    const instance = setupMods(mods);
    const { container } = render(<ModsTab instance={instance} busy={false} browse={vi.fn()} />);

    const viewport = container.querySelector<HTMLElement>(".virtual")!;
    Object.defineProperty(viewport, "clientHeight", { configurable: true, value: 420 });
    act(() => shims.triggerResize(viewport));

    // Only the visible slice + overscan should be mounted, not all 200 rows.
    const mounted = container.querySelectorAll(".mod-item");
    expect(mounted.length).toBeGreaterThan(5);
    expect(mounted.length).toBeLessThan(35);

    const search = screen.getByPlaceholderText(t("筛选模组，可用拼音", "Filter mods"));
    fireEvent.change(search, { target: { value: "mod-150" } });

    await waitFor(() => {
      expect(container.querySelectorAll(".mod-item").length).toBe(1);
    });
    expect(screen.getByText("mod-150")).toBeTruthy();
  });

  it("enforces single-row expansion mutual exclusion between detail and dependency warning", async () => {
    const mods: LocalMod[] = [
      { name: "fabric-api.jar", enabled: true, bytes: 1_500_000 },
      { name: "lithium.jar", enabled: true, bytes: 500_000 },
      { name: "sodium.jar", enabled: true, bytes: 900_000 },
    ];
    const instance = setupMods(mods, {
      "fabric-api.jar": { id: "fabric-api", name: "Fabric API", description: "Core API", version: "0.100", loaders: ["fabric"], depends: [], provides: [] },
      "lithium.jar": { id: "lithium", name: "Lithium", description: "Game physics optimizations", version: "0.13", loaders: ["fabric"], depends: [], provides: [] },
      "sodium.jar": { id: "sodium", name: "Sodium", description: "Rendering engine", version: "0.6", loaders: ["fabric"], depends: ["fabric-api"], provides: [] },
    });

    const { container } = render(<ModsTab instance={instance} busy={false} browse={vi.fn()} />);
    await waitFor(() => {
      expect(screen.getByText("Fabric API")).toBeTruthy();
    });

    // Open Lithium detail panel.
    const lithiumBtn = [...container.querySelectorAll<HTMLButtonElement>("button.mod-main")].find((btn) =>
      btn.textContent?.includes("Lithium"),
    )!;
    fireEvent.click(lithiumBtn);
    expect(lithiumBtn.getAttribute("aria-expanded")).toBe("true");
    expect(container.querySelectorAll(".mod-detail").length).toBe(1);

    // Toggling Fabric API off warns about Sodium and closes Lithium's detail so only one row expands.
    const switches = screen.getAllByRole("switch");
    fireEvent.click(switches[0]);

    const alert = screen.getByRole("alert");
    expect(alert.textContent).toContain("Sodium");
    expect(lithiumBtn.getAttribute("aria-expanded")).toBe("false");
    expect(container.querySelectorAll(".mod-detail").length).toBe(0);

    // Confirming "Disable anyway" runs the toggle operation.
    fireEvent.click(screen.getByRole("button", { name: t("仍然禁用", "Disable anyway") }));
    expect(fileOperationMock).toHaveBeenCalledWith(instance.id, { kind: "toggleMod", name: "fabric-api.jar" });
  });

  it("clears batch dependency warning when exiting multi-select mode", async () => {
    const mods: LocalMod[] = [
      { name: "fabric-api.jar", enabled: true, bytes: 1_500_000 },
      { name: "sodium.jar", enabled: true, bytes: 900_000 },
    ];
    const instance = setupMods(mods, {
      "fabric-api.jar": { id: "fabric-api", name: "Fabric API", description: null, version: "0.100", loaders: ["fabric"], depends: [], provides: [] },
      "sodium.jar": { id: "sodium", name: "Sodium", description: null, version: "0.6", loaders: ["fabric"], depends: ["fabric-api"], provides: [] },
    });

    const { container } = render(<ModsTab instance={instance} busy={false} browse={vi.fn()} />);
    await waitFor(() => {
      expect(screen.getByText("Fabric API")).toBeTruthy();
    });

    // Enter multi-select, pick only Fabric API, trigger batch Disable warning.
    fireEvent.click(screen.getByRole("button", { name: t("多选", "Select") }));
    const rows = container.querySelectorAll<HTMLElement>(".mod-row");
    fireEvent.click(rows[0]);
    fireEvent.click(screen.getByRole("button", { name: t("禁用", "Disable") }));
    expect(screen.getByRole("alert").textContent).toContain("Sodium");

    // Exiting multi-select must clear the warning rather than leaking it onto the row.
    fireEvent.click(screen.getByRole("button", { name: t("完成", "Done") }));
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
