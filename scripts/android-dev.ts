/**
 * Runs the app on the connected Android phone with live reload
 * (`bun run android:dev`).
 *
 * Default: over USB. The phone's localhost ports are forwarded to this
 * machine with `adb reverse`, and the dev server listens on 127.0.0.1 only,
 * so it is never exposed to the network.
 *
 * `bun run android:dev --wifi`: Tauri's own default for physical devices.
 * The dev server listens on this machine's LAN address and the phone
 * connects over Wi-Fi. Both must be on the same network, and the firewall
 * must allow ports 1420 and 1421.
 *
 * Any other arguments are passed to `tauri android dev`.
 */

import { argv, spawn } from "bun";
import { adb } from "./lib/adb";

const SCRIPT = "android-dev";
// Vite dev server and its HMR websocket; see apps/native/vite.config.ts.
const PORTS = [1420, 1421];

const args = argv.slice(2);
const wifi = args.includes("--wifi");
const passthrough = args.filter((a) => a !== "--wifi");

const tauriArgs = ["tauri", "android", "dev", ...passthrough];
if (!wifi) {
  for (const port of PORTS) {
    adb(SCRIPT, ["reverse", `tcp:${port}`, `tcp:${port}`]);
  }
  tauriArgs.push("--host", "127.0.0.1");
}

const child = spawn(["bun", "run", "--cwd", "apps/native", ...tauriArgs], {
  cwd: `${import.meta.dir}/..`,
  stdio: ["inherit", "inherit", "inherit"],
});

process.exit(await child.exited);
