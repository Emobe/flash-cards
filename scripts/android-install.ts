/**
 * Installs the debug APK from `bun run android:build` on the connected
 * Android device and launches it (`bun run android:install`).
 */

import { existsSync } from "node:fs";
import { join } from "node:path";
import { ACTIVITY, APP_ID, adb, fail } from "./lib/adb";

const SCRIPT = "android-install";
const APK = join(
  import.meta.dir,
  "../apps/native/src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk",
);

if (!existsSync(APK)) {
  fail(SCRIPT, `no debug APK at ${APK}. Run \`bun run android:build\` first.`);
}

adb(SCRIPT, ["install", "-r", APK]);
adb(SCRIPT, ["shell", "am", "start", "-n", `${APP_ID}/${ACTIVITY}`]);
