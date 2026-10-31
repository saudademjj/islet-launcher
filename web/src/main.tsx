import { StrictMode } from "react";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";

import { App } from "./App";
import { inTauri, loadBackend, toError } from "./api/backend";
import { applyPixelType } from "./lib/pixelScale";
import { useStore } from "./store";
import "./styles/tokens.css";
import "./styles/app.css";
import { t } from "./lib/i18n";

applyPixelType();
const root = createRoot(document.getElementById("root")!);

async function start() {
  const scriptAt = performance.now();
  const backend = await loadBackend();
  if (inTauri) {
    const report = (message: string) => void backend.call("frontend_log", { level: "error", message }).catch(() => {});
    window.addEventListener("error", (e) => report(`${e.message} @ ${e.filename}:${e.lineno}`));
    window.addEventListener("unhandledrejection", (e) => report(`unhandled rejection: ${toError(e.reason).message}`));
    // The WebView's own menu (Reload, Inspect) is noise outside text.
    document.addEventListener("contextmenu", (e) => {
      if (!(e.target as HTMLElement).closest("input, textarea, .selectable")) e.preventDefault();
    });
  }
  try {
    await useStore.getState().init();
    const initAt = performance.now();
    // Commit synchronously so the window never shows before the first render.
    flushSync(() =>
      root.render(
        <StrictMode>
          <App />
        </StrictMode>,
      ),
    );
    const renderedAt = performance.now();
    if (inTauri) {
      const ms = (n: number) => Math.round(n);
      void backend
        .call("frontend_log", { level: "info", message: `startup: script ${ms(scriptAt)} ms, data ${ms(initAt)} ms, first render ${ms(renderedAt)} ms after page start` })
        .catch(() => {});
    }
  } catch (error) {
    if (inTauri) void backend.call("frontend_log", { level: "error", message: `init failed: ${toError(error).message}` }).catch(() => {});
    root.render(
      <div className="boot-error">
        <strong>{t("启动器初始化失败", "The launcher failed to start")}</strong>
        <span className="selectable">{toError(error).message}</span>
      </div>,
    );
  }
  if (import.meta.env.DEV && import.meta.env.VITE_SELFTEST) void import("./dev/selftest").then((m) => m.run());
  // Show the window once React has committed the first render. WebKit pauses
  // requestAnimationFrame while the window is hidden, so use a timer instead.
  window.setTimeout(() => backend.reveal(), 0);
}

void start();
