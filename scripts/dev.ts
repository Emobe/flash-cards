/**
 * Launches the desktop app in dev mode (`bun run dev`).
 *
 * (The Linux blank-window workaround, `WEBKIT_DISABLE_DMABUF_RENDERER`, is set by the app itself
 * in `main.rs`, so it applies to every way of starting it.)
 *
 * Merges `tauri.dev.conf.json`, which gives the dev app its own identifier (and so its own data
 * folder) and window title, so a dev build never opens the collection of an installed build
 * (ADR 0012).
 */

import { argv, spawn } from "bun";

const DEV_CONFIG = `${import.meta.dir}/../apps/native/src-tauri/tauri.dev.conf.json`;

const child = spawn(
  ["bun", "run", "--filter", "native", "tauri", "dev", "--config", DEV_CONFIG, ...argv.slice(2)],
  {
    stdio: ["inherit", "inherit", "inherit"],
  },
);

process.exit(await child.exited);
