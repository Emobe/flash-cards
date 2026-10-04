/**
 * Launches the desktop app in dev mode (`bun run dev`).
 *
 * On Linux, disables WebKitGTK's DMA-BUF renderer, which shows a blank
 * window on some GPUs (notably NVIDIA). An already-set value is respected.
 * Other platforms are unaffected.
 */

import { argv, spawn } from "bun";

const env = { ...process.env };
if (process.platform === "linux" && env.WEBKIT_DISABLE_DMABUF_RENDERER === undefined) {
  env.WEBKIT_DISABLE_DMABUF_RENDERER = "1";
}

const child = spawn(["bun", "run", "--filter", "native", "tauri", "dev", ...argv.slice(2)], {
  env,
  stdio: ["inherit", "inherit", "inherit"],
});

process.exit(await child.exited);
