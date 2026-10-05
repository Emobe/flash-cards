# Status

Kept current by every session. A new session reads this first.

## Current step

0.6 Sandboxed card rendering spike: built and verified, waiting for Anthony's review (PR not opened yet). Next: 0.7 Data model and sync strategy (Opus design session, `/adr 0.7`).

## Branch

`step/0.6-card-sandbox`.

## Done

- 0.6 Sandboxed card rendering (ADR 0005 accepted). Cards render in `<iframe sandbox="allow-scripts">` from a trusted `frame.html` with its own CSP (a `card` URI scheme on native, `/card-frame.html` on web), media as frame-local blob URLs. Every bridge command needs a session token from `handshake`. The 54-attempt malicious card: 0 SUCCEEDED on desktop, the phone and web, and a negative control on the phone (token check removed) did reach the core. Sample card (image, audio, JS) works on all three. Timings and limits are in ADR 0005 build notes.
- Planning docs (`docs/`).
- 0.1 Repository and workspace boilerplate. Verified on Manjaro only.
- 0.2 Android build. Debug APK and USB live reload verified on the phone, workflow runs through Bun. Includes a 16 KB page alignment fix.
- 0.4 Web client spike: core runs in wasm in a worker, SQLite persists in OPFS across reload and restart, cancel by worker restart (about 2 s), panic recovery. Reviewed and merged (PR #5).
- 0.5 Scheduling spike: `fsrs` 6.6.2 chosen (ADR 0004). Intervals 2, 11, 46, 163, 497 on desktop, native and wasm. Optimiser works on wasm (66 ms for 2,000 cards). Phone: same intervals, optimiser 77 ms for 200 cards. Reviewed and merged (PR #6).
- 0.3 UI-to-core bridge. 0.3a (call path, PR #3) and 0.3b (notices, progress, cancellation, events and attachments, PR #4) reviewed by Anthony and merged. Verified on desktop and the phone. Attachment round trip 1 MB: 40 ms desktop, 75 ms phone. 5 MB: 181 ms desktop, 250 ms phone.

## Remaining in this step

- Anthony: review the branch `step/0.6-card-sandbox`, especially the token gate (`apps/native/src-tauri/src/gate.rs`), `frame.html` and the malicious card. Try to break it with your own card. Open the spike panel on the phone and in a desktop browser if you want.
- Then open the PR (not done: ask or run `gh pr create`).

## Open items

- Installed for the 0.4 experiment (per-user, approved): the `wasm32-unknown-unknown` target for toolchain 1.98.1 and `wasm-bindgen-cli` 0.2.129 in `~/.cargo/bin`. `~/.cargo/bin` is not on the PATH of Claude's shell, so call it by full path or add it to PATH (not done).
- Web (0.4): verified only in headless Brave (Chromium) on Linux, built and run through Bun. Firefox, Safari, mobile browsers and Windows are unverified. Measurements are in ADR 0003 build notes.
- Android: call path, progress, cancel, events and attachments verified (0.3a, 0.3b). Reload behaviour of the notice channel on the phone is not.
- The phone has the 0.6 debug APK (real build, with the spike panel). It replaced the 0.3b harness build.
- 0.6 not verified: Windows (Tauri may expose IPC to card frames there, the token covers it), Firefox, Safari, a main-frame reload on Android. Freeze recovery for looping cards is Phase 2 (a looping card freezes the app, checked on desktop).
- Temporary code to delete: `CardSandboxSpike`, its cards file, `spikeCardMedia` and the sample media (1.10). Lasting: the gate, `handshake`, the `card` scheme, `frame.html`, `CardFrame`, `PlatformContext`.
- Windows: desktop launch and `cargo xtask check` from step 0.1 are unverified and deferred to a manual check by Anthony.
- Step 2.1 must handle edge-to-edge drawing and safe areas (status bar, navigation bar, cutout, keyboard) once in the app shell.
- Design experiment for 0.6 shared the repo's `target/` dir, so `fc-native` was cleaned afterwards (`cargo clean -p fc-native`, host and Android). The next desktop and Android builds recompile more than usual.
- Standing rule from ADR 0005 (Accepted): never grant Tauri plugin permissions to the main window, and every bridge command must check the session token. On Android card frames can call anything the main window can.
- App/bundle ID is still the placeholder `dev.placeholder.flashcards`. Pick it before step 2.7.
