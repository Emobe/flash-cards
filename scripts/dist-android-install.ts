/**
 * Installs the release APK from `bun run dist:android` on the connected Android device and
 * launches it (`bun run dist:android:install`). It is a different app from the dev build
 * (`bun run android:install`): the dev build has the ID with `.dev` added.
 */

import { existsSync } from "node:fs";
import { join } from "node:path";
import { ACTIVITY, adb, RELEASE_APP_ID } from "./lib/adb";
import { fail, workspaceVersion } from "./lib/dist";

const SCRIPT = "dist-android-install";
const apk = join(
  import.meta.dir,
  `../release/flash-cards-${workspaceVersion(SCRIPT)}-android-arm64.apk`,
);

if (!existsSync(apk)) {
  fail(SCRIPT, `no APK at ${apk}. Run \`bun run dist:android\` first.`);
}

adb(SCRIPT, ["install", "-r", apk]);
adb(SCRIPT, ["shell", "am", "start", "-n", `${RELEASE_APP_ID}/${ACTIVITY}`]);
