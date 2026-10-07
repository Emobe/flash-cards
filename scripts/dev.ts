/**
 * Launches the desktop app in dev mode (`bun run dev`).
 *
 * On Linux, disables WebKitGTK's DMA-BUF renderer, which shows a blank
 * window on some GPUs (notably NVIDIA). An already-set value is respected.
 * Other platforms are unaffected.
 *
 * Merges `tauri.dev.conf.json`, which gives the dev app its own identifier (and so its own data
 * folder) and window title, so a dev build never opens the collection of an installed build
 * (ADR 0012).
 */

import { argv, spawn } from "bun";

const DEV_CONFIG = `${import.meta.dir}/../apps/native/src-tauri/tauri.dev.conf.json`;

const env = { ...process.env };
if (process.platform === "linux" && env.WEBKIT_DISABLE_DMABUF_RENDERER === undefined) {
  env.WEBKIT_DISABLE_DMABUF_RENDERER = "1";
}

const child = spawn(
  ["bun", "run", "--filter", "native", "tauri", "dev", "--config", DEV_CONFIG, ...argv.slice(2)],
  {
    env,
    stdio: ["inherit", "inherit", "inherit"],
  },
);

process.exit(await child.exited);
