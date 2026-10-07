/**
 * Builds the Android release APK (`bun run dist:android`, ADR 0012, decision 2).
 *
 * A release build (optimised, R8, not debuggable) signed with the machine's debug key (see
 * `app/build.gradle.kts`). The APK is copied to `release/`.
 */

import { cpSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { fail, run, workspaceVersion } from "./lib/dist";

const SCRIPT = "dist-android";
const ROOT = join(import.meta.dir, "..");
const APK = join(
  ROOT,
  "apps/native/src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release.apk",
);

const version = workspaceVersion(SCRIPT);

// Remove an old APK so the one copied below is this build's.
rmSync(APK, { force: true });
run(
  SCRIPT,
  [
    "bun",
    "run",
    "--cwd",
    "apps/native",
    "tauri",
    "android",
    "build",
    "--apk",
    "--target",
    "aarch64",
  ],
  ROOT,
);

if (!existsSync(APK)) {
  fail(SCRIPT, `no release APK at ${APK} after the Tauri build.`);
}
const outDir = join(ROOT, "release");
mkdirSync(outDir, { recursive: true });
const out = join(outDir, `flash-cards-${version}-android-arm64.apk`);
cpSync(APK, out);
console.log(`\nBuilt ${out}\nInstall on the connected phone: bun run dist:android:install`);
