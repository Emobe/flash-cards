# Flash cards (working name)

A spaced repetition flashcard app: Rust core, Tauri apps for desktop and mobile, a shared React UI.
Planning docs are in [`docs/`](docs/README.md); the layout is explained in
[ADR 0001](docs/adr/0001-workspace-layout.md).

## Layout

```
crates/fc-core     Rust core library (domain logic)
crates/fc-api      The core's public API: one dispatcher for every host, generates the TS bindings
crates/fc-cli      Developer CLI (`fc`): drives the core from a terminal
crates/fc-wasm     Web host: the core compiled to wasm for the browser worker (empty on native targets)
apps/native        Tauri v2 host for desktop and Android (src-tauri/) and its Vite entry
apps/web           Web client: Vite entry, the core worker and the web transport
packages/ui        Shared React UI
packages/core-client  Typed TypeScript client for the core API (no React, no Tauri)
xtask              Repository tasks (`cargo xtask ...`)
scripts/           Bun scripts (e.g. the dev launcher)
docs/              Product, process, roadmap, phases and ADRs
```

## Prerequisites

All free. Bun is used for everything JavaScript: do not use npm, yarn or pnpm, and Node is not needed.

### Manjaro / Arch

```sh
# System libraries for Tauri
sudo pacman -S --needed webkit2gtk-4.1 base-devel curl wget file openssl appmenu-gtk-module libappindicator-gtk3 librsvg

# Rust (the exact toolchain is pinned in rust-toolchain.toml and installed automatically)
sudo pacman -S --needed rustup && rustup default stable

# Bun (version pinned in .bun-version)
curl -fsSL https://bun.sh/install | bash

# Licence and advisory checker
cargo install --locked cargo-deny

# Web client (step 0.4): clang compiles SQLite to wasm32, and the wasm-bindgen CLI must match the
# version in Cargo.lock exactly (`cargo xtask wasm` tells you the command if it does not)
sudo pacman -S --needed clang
cargo install wasm-bindgen-cli --version 0.2.129 --locked
```

The `wasm32-unknown-unknown` Rust target is installed automatically from `rust-toolchain.toml`.
`cargo xtask check` needs the web prerequisites, since it builds `fc-wasm`.

### Windows

1. [Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with
   "Desktop development with C++" (free).
2. WebView2: preinstalled on Windows 10 (recent) and 11.
3. Rust via [rustup](https://rustup.rs/) (MSVC toolchain).
4. Bun: `powershell -c "irm bun.sh/install.ps1 | iex"`
5. `cargo install --locked cargo-deny`
6. Web client: LLVM's `clang` with the wasm32 target, and
   `cargo install wasm-bindgen-cli --version 0.2.129 --locked`. Not verified on Windows.

Not yet verified on Windows: deferred to a manual check by Anthony (see `docs/phases/00-foundations.md`).

## Android (Manjaro)

Android builds are optional: desktop work and `cargo xtask check` do not need any of this. Only
Manjaro is documented and tested. Everything is free; debug builds need no signing setup.

### One-time setup

1. **Android Studio** (provides the SDK manager and a bundled JDK):
   the `android-studio` package from the AUR (e.g. `pamac build android-studio`).
   Open it once and, in *Settings > Languages & Frameworks > Android SDK*, install:
   - *SDK Platforms*: the platform matching `compileSdk` in
     `apps/native/src-tauri/gen/android/app/build.gradle.kts` (currently Android API 37).
   - *SDK Tools*: Android SDK Platform-Tools, Android SDK Build-Tools, NDK (Side by side).
2. **Environment variables.** Add to your shell profile (e.g. `~/.zshrc`), adjusting the NDK version
   to the one in `~/Android/Sdk/ndk/`:
   ```sh
   export ANDROID_HOME="$HOME/Android/Sdk"
   export NDK_HOME="$ANDROID_HOME/ndk/27.1.12297006"
   export JAVA_HOME=/opt/android-studio/jbr   # JDK bundled with Android Studio
   ```
3. **Rust Android target** (run inside the repo so it lands on the pinned toolchain):
   `rustup target add aarch64-linux-android`
4. **Phone:** enable *Developer options* and *USB debugging*, connect by USB, and accept the
   "Allow USB debugging" prompt. On Samsung phones, turn off *Auto Blocker*
   (Settings > Security and privacy) or app installs over USB are blocked.
5. **Verify:** `cargo xtask doctor-android` checks all of the above and prints the fix for anything
   missing.

The first Android build downloads Gradle and the Android build plugins into `~/.gradle`
(several hundred MB, one time).

### Commands

| Command | What it does |
| --- | --- |
| `cargo xtask doctor-android` | Check the Android setup |
| `bun run android:dev` | Build, install and run on the connected phone with live reload |
| `bun run android:build` | Build a standalone debug APK (frontend bundled, no dev server needed) |
| `bun run android:install` | Install the debug APK on the connected phone and launch it |

### Live reload

`bun run android:dev` uses **USB** by default (`scripts/android-dev.ts`): it forwards ports 1420
and 1421 from the phone to this machine with `adb reverse`, and the dev server listens on
`127.0.0.1` only, so it is not exposed to your network. Edits to the UI appear on the phone
immediately. Tauri also watches `apps/native/src-tauri` and rebuilds on Rust changes.

`bun run android:dev --wifi` uses **Wi-Fi** instead (Tauri's own default for physical devices): the
dev server listens on this machine's LAN address. The phone must be on the same network, the
firewall must allow ports 1420 and 1421, and anyone on the network can reach the dev server while it
runs. Not yet tested on this project; USB is the supported path.

### Troubleshooting

- **`INSTALL_FAILED_...` or "blocked" on install (Samsung):** turn off *Auto Blocker*.
- **App shows "connection refused" in dev:** the USB forwarding was lost (for example after
  reconnecting the cable). Stop and rerun `bun run android:dev`.
- **Build says `NDK_HOME`/`JAVA_HOME` not set:** run `cargo xtask doctor-android`.

## Commands

Run from the repository root.

| Command | What it does |
| --- | --- |
| `bun install` | Install JS dependencies |
| `cargo xtask check` | Run every check (Rust fmt, clippy, tests, cargo-deny; Biome, TypeScript, Vitest). Must pass before a PR. |
| `cargo xtask fmt` | Format all Rust and JS/TS code |
| `cargo xtask bindings` | Regenerate the TypeScript bindings after changing an API type in `crates/fc-api` |
| `cargo run -p fc-cli -- new <file>` | Create an empty collection with the developer CLI (`info <file>` opens it and prints facts; `help` lists commands) |
| `bun run dev` | Launch the desktop app with hot reload (via `scripts/dev.ts`) |
| `cargo xtask wasm [--release]` | Build `fc-wasm` and generate its JS glue into `apps/web/src/wasm` (gitignored) |
| `bun run web:dev` | Build the wasm core, then run the web client with the Vite dev server |
| `bun run web:build` | Release wasm build, then a production web build in `apps/web/dist` |
| `bun run web:preview` | Serve the production web build locally |

Android commands are listed under [Android](#android-manjaro).

## Troubleshooting

- **Blank window on Linux (notably NVIDIA GPUs).** A known WebKitGTK issue with its DMA-BUF
  renderer. The app sets `WEBKIT_DISABLE_DMABUF_RENDERER=1` itself on Linux (in
  `apps/native/src-tauri/src/main.rs`), unless the variable is already set in your environment, so
  dev and release builds both work (a value you set yourself is left alone). Android is
  unaffected: it uses the phone's own WebView.
