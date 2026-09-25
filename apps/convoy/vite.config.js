import { defineConfig } from "vite";

// Tauri serves the dev build itself, so the port is fixed and the server must
// not wander off to another one when it is taken.
export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  // Modules the page imports only when first needed. Listed so the dev server
  // prepares them up front instead of reloading the page when one first
  // loads, which reset the UI tests part way through.
  optimizeDeps: {
    include: [
      "@tauri-apps/api/webview",
      "@tauri-apps/api/window",
      "@tauri-apps/plugin-fs",
      "@tauri-apps/plugin-notification",
    ],
  },
  build: {
    target: "es2022",
    sourcemap: true,
  },
});
