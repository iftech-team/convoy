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
  build: {
    target: "es2022",
    sourcemap: true,
  },
});
