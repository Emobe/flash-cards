import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import { buildId } from "../../scripts/lib/build-id.ts";

// Tauri expects a fixed dev server port and must not have the screen cleared.
// See https://v2.tauri.app/start/frontend/vite/
const host = process.env.TAURI_DEV_HOST;

export default defineConfig(({ command }) => ({
  plugins: [react()],
  define: { __BUILD_ID__: JSON.stringify(buildId(command)) },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
}));
