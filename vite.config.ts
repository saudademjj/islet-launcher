import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The web UI lives in web/; Tauri bundles the built files from dist/.
export default defineConfig({
  root: "web",
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    outDir: "../dist",
    emptyOutDir: true,
    // macOS 12 ships Safari 15's WebKit.
    target: "safari15",
    cssTarget: "safari15",
    chunkSizeWarningLimit: 600,
  },
});
