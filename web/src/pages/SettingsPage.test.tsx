// @vitest-environment happy-dom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const callMock = vi.fn(async (command: string, _args?: Record<string, unknown>) => {
  if (command === "update_source") return false;
  if (command === "set_unsaved_changes") return undefined;
  return null;
});

vi.mock("../api/backend", () => ({
  inTauri: false,
  call: (command: string, args?: Record<string, unknown>) => callMock(command, args),
  toError: (e: unknown) => ({ code: "error", message: String(e), detail: null }),
  loadBackend: vi.fn(),
  backend: vi.fn(),
}));

import { t } from "../lib/i18n";
import { usePrefs } from "../lib/prefs";
import { installDomShims, resetStore } from "../test/dom";
import { SettingsPage } from "./SettingsPage";

describe("SettingsPage", () => {
  beforeEach(() => {
    installDomShims();
    callMock.mockClear();
    usePrefs.setState({ intro: true, devInfo: false });
    resetStore({
      loginConfigured: true,
      appInfo: { version: "0.5.0", dataDir: "/tmp/islet", platform: "macos", arch: "aarch64", curseforgeConfigured: true },
    });
  });

  afterEach(() => {
    cleanup();
  });

  function renderPage() {
    return render(<SettingsPage />);
  }

  it("keeps low-frequency disclosures folded by default and toggles on click", () => {
    renderPage();

    const javaToggle = screen.getByRole("button", { name: /^Java/ });
    const advLaunchToggle = screen.getByRole("button", { name: new RegExp(t("高级启动与联动", "Advanced launch & integrations")) });
    const advDownloadToggle = screen.getByRole("button", { name: new RegExp(t("高级下载设置", "Advanced download settings")) });
    const lookDetailsToggle = screen.getByRole("button", { name: new RegExp(t("界面细节", "Interface details")) });

    expect(javaToggle.getAttribute("aria-expanded")).toBe("false");
    expect(advLaunchToggle.getAttribute("aria-expanded")).toBe("false");
    expect(advDownloadToggle.getAttribute("aria-expanded")).toBe("false");
    expect(lookDetailsToggle.getAttribute("aria-expanded")).toBe("false");

    fireEvent.click(lookDetailsToggle);
    expect(lookDetailsToggle.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByText(t("启动动画", "Intro animation"))).toBeTruthy();

    fireEvent.click(lookDetailsToggle);
    expect(lookDetailsToggle.getAttribute("aria-expanded")).toBe("false");
  });

  it("auto-expands folded sections on search and marks hits without highlighting toggle buttons", async () => {
    const { container } = renderPage();
    const search = screen.getByLabelText(t("搜索设置", "Search settings"));

    fireEvent.change(search, { target: { value: "Discord" } });

    await waitFor(() => {
      expect(screen.getByPlaceholderText("1234567890123456789")).toBeTruthy();
    });

    const hits = container.querySelectorAll(".search-hit");
    expect(hits.length).toBeGreaterThan(0);
    for (const el of hits) {
      expect(el.classList.contains("launch-options-toggle")).toBe(false);
    }

    // Clearing the query collapses folded disclosures and clears search highlights.
    fireEvent.change(search, { target: { value: "" } });
    await waitFor(() => {
      expect(container.querySelectorAll(".search-hit").length).toBe(0);
    });
    expect(screen.queryByPlaceholderText("1234567890123456789")).toBeNull();
  });

  it("keeps advanced download settings expanded while an invalid value is present", () => {
    renderPage();

    const advDownloadToggle = screen.getByRole("button", { name: new RegExp(t("高级下载设置", "Advanced download settings")) });
    fireEvent.click(advDownloadToggle);
    expect(advDownloadToggle.getAttribute("aria-expanded")).toBe("true");

    const input = screen.getByDisplayValue("32");
    fireEvent.change(input, { target: { value: "999" } });

    // Attempting to fold while invalid must keep the error visible.
    fireEvent.click(advDownloadToggle);
    expect(advDownloadToggle.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByDisplayValue("999")).toBeTruthy();

    // Fixing the value allows folding again cleanly.
    fireEvent.change(input, { target: { value: "16" } });
    expect(advDownloadToggle.getAttribute("aria-expanded")).toBe("false");
  });
});
