// @vitest-environment happy-dom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const callMock = vi.fn(async (command: string, _args?: Record<string, unknown>) => {
  switch (command) {
    case "list_worlds":
      return {
        quickPlay: true,
        worlds: [
          { folder: "survival-1", name: "Main Base", bytes: 4_200_000, modifiedMs: 1_700_000_000_000, icon: null },
        ],
      };
    case "list_servers":
    case "list_screenshots":
    case "list_mods":
      return [];
    case "mod_meta":
    case "mod_icons":
      return {};
    case "modpack_info":
    case "last_mod_change":
      return null;
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
import { useStore } from "../store";
import { installDomShims, makeInstance, resetStore } from "../test/dom";
import { ManageInstance } from "./ManageInstance";

function ManageHost() {
  const modal = useStore((s) => s.modal);
  if (!modal || modal.kind !== "manage") return null;
  return <ManageInstance id={modal.id} tab={modal.tab} />;
}

describe("ManageInstance", () => {
  beforeEach(() => {
    installDomShims();
    callMock.mockClear();
    const instance = makeInstance();
    resetStore({
      instances: [instance],
      versionsLoaded: true,
      versions: [{ id: "1.21.4", versionType: "release", url: "", time: "", releaseTime: "", sha1: "", complianceLevel: null }],
      modal: { kind: "manage", id: instance.id, tab: "saves" },
    });
  });

  afterEach(() => {
    cleanup();
  });

  it("decouples Saves from Tools and jumps to Tools via the shortcut button", async () => {
    render(<ManageHost />);

    await waitFor(() => {
      expect(screen.getByText("Main Base")).toBeTruthy();
    });

    // Tools actions should not be mixed into the Saves tab.
    expect(screen.queryByText(t("立即备份", "Back up now"))).toBeNull();

    // Clicking "Open Tools" switches to the dedicated Tools tab.
    fireEvent.click(screen.getByRole("button", { name: t("打开工具", "Open Tools") }));

    await waitFor(() => {
      const modal = useStore.getState().modal;
      expect(modal?.kind === "manage" && modal.tab).toBe("tools");
    });

    await waitFor(() => {
      expect(screen.getByText(t("导出为整合包", "Export as modpack"))).toBeTruthy();
    });
  });

  it("prompts with LeaveBar when switching tabs with unsaved edits in GeneralTab", async () => {
    useStore.setState({ modal: { kind: "manage", id: "world-1", tab: "general" } });
    render(<ManageHost />);

    const nameInput = screen.getByDisplayValue("Forest Survival");
    fireEvent.change(nameInput, { target: { value: "Renamed World" } });

    // Switching tabs triggers LeaveBar instead of discarding silently.
    fireEvent.click(screen.getByRole("tab", { name: t("存档", "Saves") }));
    await waitFor(() => {
      expect(screen.getByRole("alertdialog")).toBeTruthy();
    });
    expect(useStore.getState().modal).toEqual({ kind: "manage", id: "world-1", tab: "general" });

    // "Keep editing" dismisses the bar and preserves the edited name.
    fireEvent.click(screen.getByRole("button", { name: t("继续编辑", "Keep editing") }));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(screen.getByDisplayValue("Renamed World")).toBeTruthy();

    // Switching again and choosing "Discard" leaves to Saves and clears the guard.
    fireEvent.click(screen.getByRole("tab", { name: t("存档", "Saves") }));
    await waitFor(() => {
      expect(screen.getByRole("alertdialog")).toBeTruthy();
    });
    fireEvent.click(screen.getByRole("button", { name: t("放弃修改", "Discard") }));

    await waitFor(() => {
      const modal = useStore.getState().modal;
      expect(modal?.kind === "manage" && modal.tab).toBe("saves");
    });
    expect(useStore.getState().formGuard).toBeNull();
  });
});
