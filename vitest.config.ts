import { defineConfig } from "vitest/config";

// Unit tests for the UI's pure logic. They run in Node, without a WebView;
// web/src/test/setup.ts supplies the storage and language they need.
export default defineConfig({
  root: "web",
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
    setupFiles: ["src/test/setup.ts"],
    // Vitest empties stylesheets by default; the contrast test reads the
    // colour tokens as text.
    css: { include: [/tokens\.css/] },
  },
});
