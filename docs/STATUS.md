# Status

Kept current by every session. A new session reads this first.

## Current step

1.2 Note types and fields: in progress on `step/1.2-note-types`. It is the first step that adds synced tables, so follow the rules at the top of `crates/fc-core/src/sync/mod.rs`. Plan approved by Anthony (fractional-index positions, built-ins seeded with the lowest clock, template syntax Mustache-style, field rename does not rewrite templates until 1.4).

## Branch

`step/1.2-note-types` (from master after PR #10).

## Done

- 1.1b Sync foundation (ADR 0006 build notes). Reviewed and merged (PR #10). Anthony ran the APK on the phone: it shows "Collection storage version 2", so the phone line of 1.1a and 1.1b at runtime are verified. `fc-core`: `Id` (UUIDv7 from host time and `getrandom`, blob in SQLite, UUID string in the API), `Clock` and `Host`, the HLC saved in `meta`, device ID regenerated for a different installation or on request, schema v2 (`register_clock`, `unknown_register`, `write_guard`, `requirement`), `Collection::write` / `WriteTx` as the only way to write a synced table (guard triggers make the database refuse anything else), the unknown-data store, collection `requires` (reported in `info()`, does not refuse to open). Hosts: `fc-native`, `fc-cli` (system clock via `chrono`, installation ID outside the collection), `fc-wasm` and the web worker (IndexedDB). 64 core tests. Verified on Linux (CLI, desktop app migrated the real collection from v1 to v2) and in headless Brave (`getrandom` works, device ID stable across restart, changes when the installation ID is wiped). Android: APK built and run on the phone by Anthony.

- 1.1a Collection storage and migrations (ADR 0003 build notes). `fc_core::collection`: create, open, close, migrations in one transaction, a newer or foreign file refused untouched. Notes spike removed, `getCollectionInfo` and error kinds `updateRequired` and `unavailable` added, web worker surfaces open errors, minimal `fc` CLI (`fc new`, `fc info`). Verified on Linux (tests, CLI, desktop app created its collection) and the web (headless Brave: create, reload, new browser process). Android: APK builds with bundled SQLite; Anthony saw the collection line on the phone (checked with 1.1b).
- 0.6 Sandboxed card rendering (ADR 0005 accepted). Cards render in `<iframe sandbox="allow-scripts">` from a trusted `frame.html` with its own CSP (a `card` URI scheme on native, `/card-frame.html` on web), media as frame-local blob URLs. Every bridge command needs a session token from `handshake`. The 54-attempt malicious card: 0 SUCCEEDED on desktop, the phone and web, and a negative control on the phone (token check removed) did reach the core. Sample card (image, audio, JS) works on all three. Timings and limits are in ADR 0005 build notes. Reviewed and merged (PR #7).
- Planning docs (`docs/`).
- 0.1 Repository and workspace boilerplate. Verified on Manjaro only.
- 0.2 Android build. Debug APK and USB live reload verified on the phone, workflow runs through Bun. Includes a 16 KB page alignment fix.
- 0.4 Web client spike: core runs in wasm in a worker, SQLite persists in OPFS across reload and restart, cancel by worker restart (about 2 s), panic recovery. Reviewed and merged (PR #5).
- 0.5 Scheduling spike: `fsrs` 6.6.2 chosen (ADR 0004). Intervals 2, 11, 46, 163, 497 on desktop, native and wasm. Optimiser works on wasm (66 ms for 2,000 cards). Phone: same intervals, optimiser 77 ms for 200 cards. Reviewed and merged (PR #6).
- 0.3 UI-to-core bridge. 0.3a (call path, PR #3) and 0.3b (notices, progress, cancellation, events and attachments, PR #4) reviewed by Anthony and merged. Verified on desktop and the phone. Attachment round trip 1 MB: 40 ms desktop, 75 ms phone. 5 MB: 181 ms desktop, 250 ms phone.

## Remaining in this step

- 1.2: see the plan in the session; chunks are committed in order (tables and built-ins, operations, CLI, docs).

## Open items

- 1.1b not verified: Windows, Firefox, Safari, `register_clock` size and speed at 50,000 notes, any real two-device sync (Phase 4).
- 1.1b leaves for later steps: the merge write method that opens the guard with a remote clock (1.11), a write path for migrations that rewrite synced rows (first migration that needs it), per-entity `requires` registers (steps that need them), unknown events (1.7, 1.11). Phase 4 must read `unsupported_features` before syncing.
- A restore or import must call `Collection::regenerate_device_id` (1.13): a copy opened by the same installation keeps its device ID.
- New dependencies in 1.1b: `uuid` `v5` feature (pulls `sha1_smol`), `getrandom` direct in `fc-core`, `chrono` in `fc-native` and `fc-cli` (clock feature only).

- 1.1a not verified: Windows, Firefox and Safari, and a newer-collection error in a real browser (core and transport tests cover it).
- Native startup logs a collection open failure and carries on, so every collection method then answers "No collection is open." The real error screen (newer collection, file in use) comes with the app shell in step 2.1.
- One connection behind a mutex: reads wait for a long write. A read connection is added by the first step that needs it (ADR 0003 build notes, 1.1a).
- Harness trap for browser checks: Brave restores the previous launch's tabs, which hold the OPFS handles. Delete `Default/Sessions` and the `Current/Last Session|Tabs` files between launches.
- Installed for the 0.4 experiment (per-user, approved): the `wasm32-unknown-unknown` target for toolchain 1.98.1 and `wasm-bindgen-cli` 0.2.129 in `~/.cargo/bin`. `~/.cargo/bin` is not on the PATH of Claude's shell, so call it by full path or add it to PATH (not done).
- Web (0.4): verified only in headless Brave (Chromium) on Linux, built and run through Bun. Firefox, Safari, mobile browsers and Windows are unverified. Measurements are in ADR 0003 build notes.
- Android: call path, progress, cancel, events and attachments verified (0.3a, 0.3b). Reload behaviour of the notice channel on the phone is not.
- The phone has the 1.1b debug APK (storage version 2).
- 0.6 not verified: Windows (Tauri may expose IPC to card frames there, the token covers it), Firefox, Safari, a main-frame reload on Android. Freeze recovery for looping cards is Phase 2 (a looping card freezes the app, checked on desktop).
- Temporary code to delete (the notes spike is already gone): `CardSandboxSpike`, its cards file, `spikeCardMedia` and the sample media (1.10). Lasting: the gate, `handshake`, the `card` scheme, `frame.html`, `CardFrame`, `PlatformContext`.
- Windows: desktop launch and `cargo xtask check` from step 0.1 are unverified and deferred to a manual check by Anthony.
- Step 2.1 must handle edge-to-edge drawing and safe areas (status bar, navigation bar, cutout, keyboard) once in the app shell.
- Design experiment for 0.6 shared the repo's `target/` dir, so `fc-native` was cleaned afterwards (`cargo clean -p fc-native`, host and Android). The next desktop and Android builds recompile more than usual.
- Standing rule from ADR 0005 (Accepted): never grant Tauri plugin permissions to the main window, and every bridge command must check the session token. On Android card frames can call anything the main window can.
- 0.7 experiments (scratch, outside the repo): register merge, event union, FSRS replay and IDs verified natively on Linux; IDs and `getrandom` on wasm under Bun only, not in a browser. Step 1.1 checks `getrandom` in the browser.
- App/bundle ID is still the placeholder `dev.placeholder.flashcards`. Pick it before step 2.7.
