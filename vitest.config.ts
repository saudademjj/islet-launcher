import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

// Unit tests for pure UI logic (default node environment) and interactive
// React component tests (opt-in happy-dom via `@vitest-environment happy-dom`).
export default defineConfig({
  root: "web",
  plugins: [react()],
  test: {
    environment: "node",
    include: ["src/**/*.test.{ts,tsx}"],
    setupFiles: ["src/test/setup.ts"],
    // Vitest empties stylesheets by default; the contrast test reads the
    // colour tokens as text.
    css: { include: [/tokens\.css/] },
  },
});
