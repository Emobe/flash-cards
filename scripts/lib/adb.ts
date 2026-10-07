/** Shared helpers for the Android scripts. */

import { join } from "node:path";
import { spawnSync } from "bun";

/** The release build's ID, `identifier` in tauri.conf.json. */
export const RELEASE_APP_ID = "io.github.emobe.flashcards";
/** The debug build's ID: `bundle > android > debugApplicationIdSuffix` in tauri.conf.json adds `.dev`. */
export const APP_ID = "io.github.emobe.flashcards.dev";
/** The Kotlin package (`namespace`): the activity class keeps it when the debug ID gains `.dev`. */
export const ACTIVITY = "io.github.emobe.flashcards.MainActivity";

export function fail(script: string, message: string): never {
  console.error(`${script}: ${message}`);
  process.exit(1);
}

/** adb from $ANDROID_HOME/platform-tools, the one `cargo xtask doctor-android` checks. */
export function adbPath(script: string): string {
  const sdk = process.env.ANDROID_HOME;
  if (!sdk) {
    fail(script, "ANDROID_HOME is not set. Run `cargo xtask doctor-android`.");
  }
  return join(sdk, "platform-tools", process.platform === "win32" ? "adb.exe" : "adb");
}

/** Runs adb with the given arguments, echoing the command; exits on failure. */
export function adb(script: string, args: string[]): void {
  console.log(`$ adb ${args.join(" ")}`);
  const { exitCode } = spawnSync([adbPath(script), ...args], {
    stdio: ["inherit", "inherit", "inherit"],
  });
  if (exitCode !== 0) {
    fail(script, `adb ${args[0]} failed (exit code ${exitCode})`);
  }
}
