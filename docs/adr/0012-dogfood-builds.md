# 0012: Dogfood builds: app ID, signing, installers and the version

Status: Accepted
Date: 2026-10-07
Accepted: 2026-10-07 (with the answers in Decisions on review)

## Context

Step 2.7 (`docs/phases/02-study-loop.md`) has two acceptance criteria:

- A documented process to produce an Android APK (sideloaded, no Play Store) and unsigned Linux and
  Windows installers, all at zero cost.
- The app version visible in Settings.

Dogfood builds are the first builds that hold real study data. That changes several things that were
fine for development:

1. **The app ID.** `PRODUCT.md` says the final app ID must be picked before 2.7: Android treats a new
   ID as a different app, so a later change means a new install and moving the data by hand. Today it
   is `dev.placeholder.flashcards`. The ID also names the desktop data folder (Tauri's
   `app_data_dir`, used for `collection.db` and `backups/`), so changing it moves desktop data too.
2. **Android signing.** Every APK so far is a debug build signed with the machine's debug key
   (`~/.android/debug.keystore`). Android only installs an update signed with the same key as the
   installed app. A debug build is also slow (unoptimised Rust) and debuggable.
3. **Dev builds and dogfood builds on one device.** `bun run dev` and `bun run android:dev` use the
   same ID as a dogfood build would. On desktop they would open the real collection, so a dev build
   with a new migration would upgrade real data. On the phone, a debug build would refuse to install
   over a release build (different key), and the obvious fix, uninstalling, deletes the real
   collection.
4. **The blank window on Linux.** The README says how release builds deal with the WebKitGTK
   DMA-BUF problem "is not decided yet". `scripts/dev.ts` sets `WEBKIT_DISABLE_DMABUF_RENDERER=1`
   only for `bun run dev`.
5. **Windows has never been built.** Phase 0 left Windows to a manual check by Anthony, which has not
   happened yet.
6. **The version.** Every crate and package is `0.0.0`. `tauri.conf.json` says `0.0.1`. The UI
   already gets `getCoreInfo().coreVersion` (fc-core's `CARGO_PKG_VERSION`), shown nowhere.

Constraints: zero cost (`PRODUCT.md`), Bun for JS work, no Play Store, no code signing certificates,
no system-wide installs without asking, `gh` for everything on GitHub. The repository is public
(`gh repo view`: `PUBLIC`).

## Findings

Research on 2026-10-07, and throwaway experiments in a git worktree in the session scratchpad (not
in the repo), on Manjaro (X11, i3, NVIDIA RTX 3070, driver 580, `webkit2gtk-4.1` 2.52.6, glibc 2.44)
and Anthony's phone (Samsung SM-S928B). The desktop runs used empty data folders
(`XDG_DATA_HOME` and `XDG_CONFIG_HOME` in the scratchpad), so the real collection was not touched.

1. **Linux release bundles build.** `tauri build --bundles appimage,deb` produced an AppImage
   (101 MiB) and a `.deb` (5.1 MiB) in one run. Tauri downloaded `linuxdeploy` and its plugins from
   GitHub into its cache the first time. **Verified.**
2. **A release build has the blank window on this machine.** The plain release binary and the
   AppImage, started without the variable, both showed a blank window (a screenshot of one colour)
   and logged `Failed to create GBM buffer of size 1000x700: Invalid argument`. With
   `WEBKIT_DISABLE_DMABUF_RENDERER=1` both showed the deck screen. **Verified.** Anyone starting the
   app from a menu has no way to set the variable, so the app must set it itself.
3. **Setting the variable in `main` before Tauri starts is enough.** A patched `main.rs` that sets
   it (Linux only, when it is not already set) showed the deck screen with no variable in the
   environment and no GBM errors. **Verified.** In Rust 2024, `std::env::set_var` is `unsafe`, and
   the workspace has `unsafe_code = "forbid"`, so this needs a narrow exception (decision 4).
4. **The AppImage carries its own WebKitGTK and the GStreamer core, but no GStreamer plugins**
   (`bundleMediaFramework` is off) and its start-up hook sets no plugin path. Card audio would then
   depend on the host's plugins matching the bundled core. **Not verified** whether sound plays. The
   Tauri docs also say an AppImage needs the glibc it was built on or newer, so one built on Manjaro
   (2.44) will not run on older distributions.
5. **The `.deb` repackages cleanly as a pacman package.** A four-line `PKGBUILD` that unpacks the
   `.deb`'s `data.tar.gz` built `flash-cards-0.0.1-1-x86_64.pkg.tar.xz` with `makepkg` (not
   installed). It contains `/usr/bin/fc-native`, a `.desktop` file and icons, and uses the system's
   WebKitGTK and GStreamer. **Verified that it builds; not installed or run.** Two things to fix:
   the binary is called `fc-native` and the menu comment is the crate description ("Tauri host for
   the desktop and mobile apps…").
6. **A signed release APK builds and runs.** The Tauri signing recipe (a `keystore.properties` and
   a `signingConfigs` block in `app/build.gradle.kts`) with a throwaway key built
   `app-universal-release.apk` (16 MB, arm64). To install it next to the existing app, the
   experiment gave the release build type `applicationIdSuffix = ".relexp"`. It installed, started
   and showed the deck screen; `apksigner` shows the throwaway certificate; the package is not
   `DEBUGGABLE`. R8 minification is on in the template and nothing failed at start-up. **Verified**
   on the phone. **Not verified:** the `AppearancePlugin` call under R8 (it would need a tap on the
   phone to change the theme), and everything past the first screen.
7. **The installed app today** is `dev.placeholder.flashcards`, versionCode 1, `DEBUGGABLE`, debug
   key, first installed 2026-10-04.
8. **Tauri derives the Android versionCode from the version:** `major * 1000000 + minor * 1000 +
   patch` (`crates/tauri-cli/src/mobile/android/mod.rs`; `0.0.1` gives versionCode 1, as the
   generated `tauri.properties` shows). An update with a lower versionCode is refused.
9. **Tauri can suffix the debug app ID by itself:** `bundle > android > debugApplicationIdSuffix`
   in `tauri.conf.json`. The CLI writes `applicationIdSuffix` into the debug build type of
   `build.gradle.kts`; 2.12.0 fixed it writing to the wrong block. The repo's CLI is 2.12.1.
   **Not verified** with `tauri android dev` (the CLI starts the activity with `am start -n`, so it
   has to use the suffixed ID; the build checks this).
10. **Windows from Linux:** Tauri documents cross-compiling an NSIS installer with `cargo-xwin`,
    `llvm`, `lld` and `nsis`, as "not as straight forward as compiling on Windows directly and … not
    tested as much", to use "as a last resort". MSI cannot be cross-compiled. On Manjaro, `lld` is
    installed; `llvm` (for `llvm-rc`) is not; `nsis` is only in the AUR (its build needs
    `mingw-w64-gcc`); `cargo-xwin` downloads the Microsoft CRT and Windows SDK, which needs accepting
    Microsoft's licence. **Not tried**: each needs a system-wide install.
11. **GitHub Actions:** GitHub's December 2025 pricing change keeps standard GitHub-hosted runners
    free on public repositories, Windows included. Private repositories get a monthly free quota.
    From GitHub's docs, not tested: with no payment method on the account, going over the quota
    blocks jobs rather than charging.
12. **Unsigned installers on Windows** trigger SmartScreen ("Windows protected your PC", then
    More info, then Run anyway). The NSIS installer installs per user by default and gets WebView2
    with the bootstrapper if it is missing (needs internet; preinstalled on Windows 10 and 11).

## Decision 1: the app ID, and keeping dev builds apart

**Anthony picks the ID** (a product decision, `PRODUCT.md`). It does not need to match the final
name. A suggestion: `io.github.emobe.flashcards`. It is a reverse domain he controls (his GitHub
account) without buying a domain.

The dogfood build uses that ID. **Dev and debug builds use the same ID plus `.dev`**:

- Android: `bundle > android > debugApplicationIdSuffix: ".dev"`. The Kotlin package (the
  `namespace`) stays the base ID. A `src/debug/res/values/strings.xml` names the debug app
  "Flash cards dev" so the phone shows two different labels.
- Desktop: `bun run dev` merges a config that sets `identifier` to the `.dev` ID and the window
  title to "Flash cards dev". Its data folder is therefore separate from the dogfood app's.
- `scripts/lib/adb.ts` uses the `.dev` ID for `android:dev` and `android:install`.

Doing the ID change in the same build as the new signing key (decision 2) means one move of data,
not two. Both need a new install anyway.

**Moving existing data** (once): the old `dev.placeholder.flashcards` app and its desktop folder are
left as they are. In the old app, Settings, Export; in the new one, Settings, Restore from a file
(both from step 2.6). The old phone app can be uninstalled afterwards, by Anthony.

Options considered:

- **A. New ID, with `.dev` for dev builds (chosen).**
- **B. Keep `dev.placeholder.flashcards`.** No work, but `PRODUCT.md` asks for the change before real
  data exists, and the placeholder would stay for good.
- **C. Change the ID but let dev builds share it.** Simpler config, but a dev build upgrades real
  data, and on Android it cannot install over the dogfood build (context point 3).

## Decision 2: Android: a release build signed with our own key

`bun run dist:android` builds `tauri android build --apk --target aarch64` (the release build
type: optimised, R8 on, not debuggable), signed with a release key:

- Anthony makes the key once with `keytool` (bundled with Android Studio). It lives outside the
  repository, for example `~/.android/flash-cards-release.jks`. `keystore.properties` in
  `apps/native/src-tauri/gen/android/` (already gitignored) points to it.
- `app/build.gradle.kts` gets Tauri's `signingConfigs` block. If `keystore.properties` is missing,
  the release build stops with a message saying how to make one, rather than producing an unsigned
  APK.
- **The key must be backed up.** If it is lost, a later APK cannot update the installed app; the
  only way on is uninstalling, which deletes the collection unless it was exported first.
- The script copies the APK to `release/` with the version in its name. Installing: `adb install -r`
  (a script), or copying the file to the phone and opening it (Android asks to allow installs from
  that app).

Options considered:

- **A. Release build, own key (chosen).** Fast, not debuggable. One key to look after.
- **B. Keep debug builds** signed with the machine's debug key. No set-up, but slow, debuggable (anyone
  with `adb` access can read its data with `run-as`), and the debug key is per machine and
  shared with every other debug app on it. It also collides with dev builds unless the dogfood build
  gets a suffix instead.
- **C. Release build signed with the debug key.** Fast without a new key, but the key's lifetime is
  that of `~/.android`, and its name says "debug".

## Decision 3: Linux: a pacman package for Manjaro, and the `.deb`

`bun run dist:linux` runs `tauri build --bundles deb`, then `makepkg` on a `PKGBUILD` kept in
`packaging/arch/`, which unpacks the `.deb` (finding 5). Both files go to `release/`. Anthony installs
with `sudo pacman -U release/<file>.pkg.tar.*` and removes with `sudo pacman -R flash-cards`. The
`.deb` is the installer for Debian and Ubuntu (not tested on them).

Also in this decision: `mainBinaryName` `flash-cards` (not `fc-native`), and a proper short
description for the menu entry and packages.

Options considered:

- **A. pacman package from the `.deb` (chosen).** Uses the system's WebKitGTK and GStreamer, so it
  gets their security updates and card audio has the same plugins as dev builds. Small (5 MB).
  `makepkg` ships with every Arch-based system. Only installs on Arch-based systems.
- **B. AppImage.** No install step and runs on most distributions, but it is 100 MB, freezes a copy
  of WebKitGTK (no security updates until the next build), has no GStreamer plugins (finding 4) and
  needs the glibc of the build machine or newer (finding 4). It can be added later for other people
  if Linux users outside Arch appear; the bundler already supports it.
- **C. Only the `.deb`.** Not installable on Manjaro without `debtap` from the AUR.

## Decision 4: the Linux blank window, fixed in the app

`apps/native/src-tauri/src/main.rs`, on Linux only, sets `WEBKIT_DISABLE_DMABUF_RENDERER=1` before
anything else runs, unless the variable is already set (so a user can still override it). It runs
before Tauri, while the process has one thread, which is what `set_var`'s safety rule asks.
`scripts/dev.ts` stops setting it, since the binary does.

`fc-native` keeps every workspace lint but `unsafe_code` goes from `forbid` to `deny` for that crate
only, with one `#[allow(unsafe_code)]` on the call and a `SAFETY` comment. Every other crate keeps
`forbid`. (Cargo's `[lints] workspace = true` cannot override one lint, so `fc-native` copies the
workspace lint table with that one change.)

Turning the DMA-BUF renderer off for everyone on Linux costs some rendering speed on machines that
would have been fine. Flash cards are light pages; the dev builds have run this way since Phase 0.

Options considered:

- **A. Set it in `main` (chosen).** Works however the app is started (finding 3).
- **B. A wrapper script or an `Exec=env …` line in the `.desktop` file.** No `unsafe`, but starting
  the binary any other way gives a blank window, and the AppImage has no place for it.
- **C. Only set it when an NVIDIA driver is found.** Keeps DMA-BUF elsewhere, but adds a detection
  rule we cannot test on other hardware. Revisit if speed matters.

## Decision 5: Windows: a manual GitHub Actions run

A workflow, `.github/workflows/windows-installer.yml`, that **only runs when started by hand**
(`workflow_dispatch`; no push or pull request trigger, so it is not CI). On `windows-2025` it checks
out the chosen ref, installs Bun at the pinned version and the toolchain from `rust-toolchain.toml`,
runs `bun install --frozen-lockfile` and `tauri build --bundles nsis`, and uploads the
`*-setup.exe` as a workflow artifact (kept 30 days). `permissions: contents: read`; no secrets;
third-party actions pinned by commit SHA. `bun run dist:windows` starts it with
`gh workflow run` on the current pushed branch, waits with `gh run watch` and downloads the installer
into `release/` with `gh run download`.

Cost: free while the repository is public (finding 11). If it is made private, the free monthly
quota applies (Windows minutes count more than Linux), and an account with no payment method is
blocked, not charged. **This is a new use of a GitHub service; Anthony approves it or picks B.**

Options considered:

- **A. GitHub Actions, by hand (chosen).** Builds on real Windows with the documented toolchain,
  nothing to install on Anthony's Windows machine to try a build, and Claude Code can produce it.
- **B. Build on Anthony's Windows machine.** Nothing new on GitHub, and it is the first real check of
  the Windows dev set-up (Phase 0's open item). But it needs the full toolchain there (C++ Build
  Tools, Rust, Bun) and Anthony at that machine for every build. Documented as the fallback.
- **C. Cross-compile on Manjaro with `cargo-xwin`.** Everything on one machine, but four system-wide
  installs (one from the AUR), a Microsoft licence to accept, and Tauri calls it a last resort
  (finding 10). NSIS only.

Either way only NSIS is built (no MSI): one installer is enough, and it installs per user without
admin rights. The installer is unsigned; SmartScreen's warning is expected (finding 12).

## Decision 6: one version, shown in Settings

- **One version for the whole app:** `[workspace.package] version` in `Cargo.toml`, with every crate
  on `version.workspace = true`. `tauri.conf.json` drops its own `version`, so Tauri takes
  `fc-native`'s (the build confirms the fallback). The JS packages stay `0.0.0` (private, never
  published). `fc_core::version()`, and so `getCoreInfo().coreVersion`, is then the app version on
  desktop, Android and the web, with no API change.
- **Numbering:** start at `0.1.0` with the first dogfood build. Raise the patch number for every
  build that goes onto a device, and the minor number at the end of each phase. The versionCode then
  always goes up (finding 8). Raising it is a one-line change plus `Cargo.lock`, in its own commit
  ("Version 0.1.1").
- **The build** (a short git commit, with `-dirty` if the tree had uncommitted changes, or `dev` under
  the dev server) is worked out by each app's Vite config and passed to `<App>` as a prop, so two
  builds with the same version can still be told apart.
- **Settings** gets an "About" group at the end: "Version 0.1.0 (a1b2c3d)". Same on every platform
  and at every width.

Options considered for the source of the version: the workspace version (chosen; one number, already
read by the core); `tauri.conf.json` read by a new native command (would not cover the web client);
the root `package.json` (Rust would have to read a JS file at build time).

## Decision 7: the documented process

A "Dogfood builds" section in `README.md`: one-time set-up (the release key and its backup, `makepkg`
being present, the GitHub workflow), the three `dist:` commands, raising the version, installing and
updating on each platform (with the SmartScreen and "install unknown apps" prompts), where the data
lives, and moving data from the placeholder app. Outputs go to `release/` (gitignored), named
`flash-cards-<version>-<platform>.<ext>`.

## Consequences

- One manual data move, now, before M1. After that, updates keep their data as long as the key is
  kept and the ID does not change.
- Dev and dogfood builds can sit on the same phone and desktop without touching each other's data.
- A lost release key means uninstalling and restoring from an export on the next update.
- One `unsafe` call in the codebase, in `fc-native` only.
- A first use of GitHub Actions (if decision 5 A is accepted), triggered by hand only.
- Every dogfood build needs a version bump commit, or Android refuses an update with a lower
  versionCode (an equal one installs).
- Windows gets its first build here; whatever is broken on Windows shows up in this step.
- Linux installs are Arch-only in practice until someone wants the AppImage.

## Revisit if

- The repository goes private, or GitHub changes free-runner terms: re-check decision 5's cost.
- A Mac arrives: macOS and iOS need signing and the Apple tooling, a new ADR.
- Linux users outside Arch appear: add the AppImage (decision 3 B), built on an older base.
- WebKitGTK fixes DMA-BUF with NVIDIA, or rendering is slow on Linux: drop or narrow decision 4.
- Auto-update is wanted: Tauri's updater needs its own signing key and somewhere to host files.
- Play Store publishing becomes a goal (a non-goal now): app bundles and Play App Signing.
- R8 breaks a plugin or a feature in release: add keep rules, or turn minification off.

## Questions for Anthony

1. **The app ID.** Your choice. Suggested: `io.github.emobe.flashcards`.
2. **Moving data:** leave the placeholder app and desktop folder as they are, and move data once
   with Export and Restore from a file (step 2.6). Is there study data on the phone or desktop worth
   moving?
3. **Windows:** GitHub Actions started by hand (A, recommended), building on your Windows machine (B)
   or cross-compiling on Manjaro (C)?
4. **Linux:** a pacman package plus the `.deb`, no AppImage.
5. **Android key:** you make the key with `keytool` and keep a backup of it. Where should the backup
   go?
6. **The one `unsafe` exception** in `fc-native` for the Linux fix.
7. **Version numbering:** `0.1.0` first, patch for every device build, minor at the end of a phase;
   the commit shown next to the version in Settings.
8. The split into two PRs in the plan (`docs/plans/2.7-dogfood-builds.md`).

## Decisions on review

Anthony's answers on 2026-10-07. Where they differ from the decisions above, these win.

1. **App ID:** Anthony left the choice to Claude Code: `io.github.emobe.flashcards`.
2. **No data to move.** There is no study data worth keeping on the phone or desktop, so the build
   skips the Export and Restore move. The placeholder app is uninstalled by Anthony.
3. **Windows: option B.** No GitHub Actions for now ("future, definitely"). Anthony builds and tests
   the installer on his Windows machine with `bun run dist:windows`, which runs there. No workflow
   file in this step.
4. **Linux: the pacman package only.** The `.deb` is still built, as the input to the `PKGBUILD`,
   but it is not documented or named as an installer. Other formats come later.
5. **Android key: option C of decision 2.** The release build is signed with the machine's existing
   debug key (`~/.android/debug.keystore`), not a new key. No `keytool`, no `keystore.properties`,
   no passwords. A separate release key waits until it is needed (a store, or building on several
   machines). Anthony keeps a copy of `~/.android/debug.keystore`: if it is lost, the next update
   means uninstalling. The dev builds use the same key, which is fine because their ID differs
   (`.dev`).
6. **The `unsafe` exception:** accepted.
7. **Version numbering and the commit in Settings:** accepted.
8. **The split** is for the build PRs (2.7a, 2.7b), as in the plan.

## Build notes (step 2.7a)

Built on `step/2.7a-app-identity`, from `master` (the ADR is merged as PR #45). First of two PRs.
No new dependency. Nothing here makes a release build; that is 2.7b.

### What was built

- **One version, `0.1.0`.** `[workspace.package] version` in the root `Cargo.toml`; every crate has
  `version.workspace = true`; `tauri.conf.json` has no `version`. Two tests that pinned `0.0.0`
  (`fc-core`'s `version_matches_manifest`, the `fc-cli` info test) read `CARGO_PKG_VERSION`.
- **The Android version** is read by `app/build.gradle.kts` from the root `Cargo.toml`
  (`[workspace.package]`), with `versionCode = major * 1000000 + minor * 1000 + patch`. See the first
  deviation: decision 6 assumed Tauri would do this. The phone's debug app now reports versionName
  `0.1.0`, versionCode `1000`. The build stops with a message if the version line is missing.
- **Build ID.** `scripts/lib/build-id.ts` (`dev` under the Vite server, otherwise the short commit with
  `-dirty` if the tree has changes, `unknown` outside git). Both Vite configs `define` `__BUILD_ID__`
  from it; both `main.tsx` pass it to `<App build>`, which hands it to `SettingsScreen`. The helper uses
  plain Node APIs because Vite loads its config under whichever runtime runs it. Each app's `tsconfig`
  includes the file, sets `rootDir` to the repository and allows the `.ts` import (Vite warned about an
  import with no extension).
- **Settings, About.** A group at the end ("Version 0.1.0 (a1b2c3d)"; the version alone if no build is
  given). It is empty while `getCoreInfo` is pending and if it fails. `SettingsScreen.test.tsx`: the
  version and build, the version alone, pending, and failing (4 tests).
- **App ID `io.github.emobe.flashcards`** in `tauri.conf.json`, `namespace` and `applicationId`,
  `ANDROID_PACKAGE` in `appearance.rs`, and the Kotlin package line and folder of both `app/` and
  `buildSrc/` (`git mv`). `git grep -i placeholder` now finds only `manifestPlaceholders`, an SQL comment in
  `search/compile.rs`, the Browse placeholder screen import and the tag input's placeholder text, and docs.
- **Dev builds.** `bundle > android > debugApplicationIdSuffix: ".dev"` (the CLI wrote
  `applicationIdSuffix = ".dev"` into the debug build type of `build.gradle.kts`, which is committed);
  `src/debug/res/values/strings.xml` names the debug app "Flash cards dev"; `bun run dev` merges
  `apps/native/src-tauri/tauri.dev.conf.json` (identifier `.dev`, window title "Flash cards dev");
  `scripts/lib/adb.ts` `APP_ID` is the `.dev` ID.

### Findings

- **Finding 9 is now verified:** `tauri android build --debug` honours `debugApplicationIdSuffix`, and
  `android:install` launched the `.dev` app.
- **The CLI does not write `tauri.properties` on `tauri android build`** when the config has no
  `version` (checked by deleting the file and rebuilding: none came back, and the APK was `1.0` / `1`).
  The old file in the working tree was a leftover from 2026-10-04.
- **`am start -n <id>/.MainActivity` breaks with a suffix:** the leading dot is relative to the
  package before the slash, which is now `...flashcards.dev`. `android-install.ts` uses the full class
  name (`ACTIVITY` in `scripts/lib/adb.ts`).

### Verified

- `cargo xtask check` passes (328 Vitest tests, 4 of them new).
- Desktop: `bun run dev` opens "Flash cards dev", creates `~/.local/share/io.github.emobe.flashcards.dev`
  (new, empty); the old `dev.placeholder.flashcards` folder was not touched; Settings shows "Version
  0.1.0 (dev)".
- Phone (Samsung SM-S928B): `android:build` and `android:install` put `io.github.emobe.flashcards.dev`
  next to `dev.placeholder.flashcards` and its `.relexp` test app, and start it; `dumpsys package`
  shows versionName `0.1.0`, versionCode `1000`.

### Not verified

- Settings on the phone (I may not tap the phone; Anthony looks at the About line).
- `bun run android:dev` (live reload under the `.dev` ID); only `android:build` and `android:install` ran.
- The web client in a browser (`web:dev`): typecheck, tests and the build config pass, nothing was loaded.
- That a desktop release build takes its version from Cargo (no `version` in `tauri.conf.json`); that
  is checked with the `.deb` in 2.7b.
- The Settings screenshot at phone width. The desktop window was 805 px wide.

### Deviations from the plan

- **The Android version is read in Gradle,** not taken from Tauri. Plan step 1 said to stop if Tauri did
  not fall back to Cargo's version; it does not for Android. Anthony left the choice to Claude Code, which
  chose Gradle reading the workspace `Cargo.toml` over putting `version` back in `tauri.conf.json` (two
  numbers to bump, and not shown to make the CLI write `tauri.properties`). `tauri.properties` is no
  longer used. This changes the text of decision 6: "Tauri takes `fc-native`'s version" holds for
  desktop bundles only (to be confirmed in 2.7b).
- **`tauri.dev.conf.json`** is a file next to `tauri.conf.json`; the plan said `scripts/dev.ts` passes
  `--config` with the values, without saying where they live.
- **The `buildSrc` Kotlin package folder moved too,** not just the `app/` sources the plan named.
- **The activity is launched by its full class name** (finding above), a change to `android-install.ts`
  the plan did not list.
- **`tsconfig` changes in both apps** (`rootDir`, the build-id include, `allowImportingTsExtensions`) and
  a `build-id.d.ts` in each, to let a Vite config import the helper from `scripts/lib/`.
- **A CSS rule** (`.group .about-version`) so the About group has no empty line under the text.

## Build notes (step 2.7b)

Built on `step/2.7b-release-builds`, from `master` (2.7a is merged as PR #46). Second of two PRs. No new
dependency. Anthony's Decisions on review apply: the Android release build uses the debug key, Linux is
the pacman package only, Windows is built on Anthony's machine, no GitHub Actions workflow.

### What was built

- **Linux fix.** `main.rs` sets `WEBKIT_DISABLE_DMABUF_RENDERER=1` on Linux when it is not set, first
  thing in `main`, in one `#[allow(unsafe_code)]` block with a `SAFETY` comment. `fc-native`'s
  `Cargo.toml` has its own copy of the workspace lint table with `unsafe_code = "deny"` (a comment
  there says to keep it in step with the root). `scripts/dev.ts` no longer sets the variable. README
  troubleshooting updated.
- **Bundle metadata.** `mainBinaryName` `flash-cards`, `category` `Education`, `shortDescription`
  "Spaced-repetition flash cards", `longDescription`.
- **`bun run dist:linux`** (`scripts/dist-linux.ts`, `scripts/lib/dist.ts`, `packaging/arch/PKGBUILD`):
  `tauri build --bundles deb`, then `makepkg` in a temporary copy of `packaging/arch/` with the `.deb`
  next to it, then the package is copied to `release/flash-cards-<version>-linux-x86_64.pkg.tar.<ext>`
  (the extension comes from `makepkg`; it is `.xz` here). `release/` is in `.gitignore`.
- **Android.** The release build type uses `signingConfigs.getByName("debug")`. `bun run dist:android`
  (`scripts/dist-android.ts`) builds with `tauri android build --apk --target aarch64` and copies the
  APK to `release/flash-cards-<version>-android-arm64.apk`; `bun run dist:android:install` installs it
  with `adb install -r` and starts it by the release ID (`RELEASE_APP_ID` in `scripts/lib/adb.ts`).
- **`bun run dist:windows`** (`scripts/dist-windows.ts`): refuses on other systems, then
  `tauri build --bundles nsis` and copies the installer to
  `release/flash-cards-<version>-windows-x64-setup.exe`. Plain Bun and Node APIs only.
- **README:** "Dogfood builds" (what the IDs mean, the commands, raising the version, installing and
  updating on each platform, the keystore backup, SmartScreen).

### Findings

- **Decision 6 holds for desktop:** with no `version` in `tauri.conf.json`, the `.deb` is
  `Flash cards_0.1.0_amd64.deb` and Settings shows 0.1.0. (Android reads Cargo in Gradle, see 2.7a.)
- **R8 does not break `AppearancePlugin`** (the open risk in finding 6): on the phone, choosing Light
  in Settings changed the page and the system bars (logcat: `APPEARANCE_LIGHT_STATUS_BARS` for the
  release package), and there is no `AndroidRuntime` error. Choosing System put it back.
- **The release APK is 16 MB** and is signed with the certificate in `~/.android/debug.keystore`
  (`apksigner` shows `CN=Android Debug`, same SHA-256 as `keytool`). `apksigner` needs a `java` on the
  PATH; Android Studio's is `/opt/android-studio/jbr/bin`.
- **The desktop file is `Flash cards.desktop`** (Tauri names it after `productName`), with
  `Exec=flash-cards`, `Icon=flash-cards`, `Categories=Education;`.
- The package is 3.8 MB, its dependencies are `webkit2gtk-4.1 gtk3 gst-plugins-base gst-plugins-good`.

### Verified

- `cargo xtask check` passes (328 Vitest tests; no test is new, the changes are build scripts and
  config).
- Linux: `bun run dist:linux` builds the package; its contents and `.desktop` file were read. The built
  `target/release/flash-cards`, started with no `WEBKIT_DISABLE_DMABUF_RENDERER` and empty `XDG_*`
  folders, showed the deck screen (no blank window, no GBM error) and Settings showed Version 0.1.0.
  `bun run dev` (variable also unset) showed "Flash cards dev" and no GBM error. Both used scratch data
  folders, so the real collections were not touched.
- Android (Samsung SM-S928B): `dist:android` and `dist:android:install` put
  `io.github.emobe.flashcards` on the phone next to the `.dev` app; `dumpsys package` shows versionName
  `0.1.0`, versionCode `1000` and no `DEBUGGABLE` flag; it started; About shows 0.1.0; the theme switch
  works (finding above).
- `bun run dist:windows` on Linux stops with its message.

### Not verified

- Installing the pacman package, starting it from the menu, and sound on a card: Anthony.
- Everything on Windows (the build, the installer, SmartScreen, WebView2, the data folder
  `%APPDATA%\io.github.emobe.flashcards`, which is from Tauri's documentation): Anthony.
- The builds Claude made say `-dirty` after the version (the changes were not committed yet). Rebuild
  from a clean tree for a clean build ID.
- Updating an installed release app with a newer one (same key, higher version code), and `bun run
  android:dev` under the `.dev` ID (also open from 2.7a).
- Another distribution or GPU for the Linux fix; the Linux package on a clean Arch install (it was built
  with `--nodeps`).

### Deviations from the plan

- **Claude Code tapped the phone** (the theme switch, scrolling to About). The plan said to ask Anthony
  to switch the theme, and `CLAUDE.md` says never touch the phone beyond installing and screenshots.
  Anthony said in chat that Claude Code may do taps and gestures; he did not mention unlocking, and the
  phone was already unlocked. `CLAUDE.md` was not changed.
- **`makepkg -f --nodeps --skipinteg`** instead of `makepkg -f`, so it never asks for `sudo` to install
  dependencies. The `PKGBUILD` also sets `options=('!strip' '!debug')`, `license=('custom')` and reads
  the version from the `FC_VERSION` variable the script sets; the plan only said `pkgver` from the
  version.
- **Extra files:** `scripts/lib/dist.ts` (shared version reader, `run`, `fail`) and
  `RELEASE_APP_ID` in `scripts/lib/adb.ts`; `dist:android:install` is its own script file.
- **Bundle text chosen without being specified:** category `Education` and the two descriptions.
- **One commit did not pass `cargo xtask check`** (`f3108eb`, a type error in `scripts/lib/dist.ts`); the
  next commit (`af3730e`) fixes it. Every other commit passes.
- **The Android build was not redone from a clean tree** (see Not verified), and the version was not
  raised: the release app has a new ID, so 0.1.0 installs as it is.
- **No change to the ADR text** of decisions 2 and 5 (the Decisions on review already override them).
