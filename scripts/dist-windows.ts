/**
 * Builds the Windows installer (`bun run dist:windows`, ADR 0012, decision 5). Run it on Windows.
 *
 * `tauri build --bundles nsis` makes an unsigned per-user installer (SmartScreen warns about it).
 * It is copied to `release/`. No shell syntax here, so it runs without a POSIX shell.
 */

import { cpSync, existsSync, mkdirSync, readdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { fail, run, workspaceVersion } from "./lib/dist";

const SCRIPT = "dist-windows";
const ROOT = join(import.meta.dir, "..");
const NSIS_DIR = join(ROOT, "target", "release", "bundle", "nsis");

if (process.platform !== "win32") {
  fail(SCRIPT, "this builds the Windows installer and runs on Windows only.");
}

const version = workspaceVersion(SCRIPT);

// Remove old installers so the one found below is this build's.
rmSync(NSIS_DIR, { recursive: true, force: true });
run(SCRIPT, ["bun", "run", "--cwd", "apps/native", "tauri", "build", "--bundles", "nsis"], ROOT);

const installer = existsSync(NSIS_DIR)
  ? readdirSync(NSIS_DIR).find((f) => f.endsWith("-setup.exe"))
  : undefined;
if (!installer) {
  fail(SCRIPT, `no *-setup.exe in ${NSIS_DIR} after the Tauri build.`);
}

const outDir = join(ROOT, "release");
mkdirSync(outDir, { recursive: true });
const out = join(outDir, `flash-cards-${version}-windows-x64-setup.exe`);
cpSync(join(NSIS_DIR, installer), out);
console.log(`\nBuilt ${out}`);
