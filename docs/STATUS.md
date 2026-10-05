# Status

Kept current by every session. A new session reads this first.

## Current step

0.5 Scheduling spike: built and verified on desktop and in the browser. ADR 0004 accepted, build notes added. `cargo xtask check` passes. Not verified on the phone (locked, and rules forbid unlocking or tapping), so the PR is waiting on Anthony's review and a phone check. 0.4 was reviewed and merged (PR #5).

## Branch

`step/0.5-scheduling-spike`.

## Done

- Planning docs (`docs/`).
- 0.1 Repository and workspace boilerplate. Verified on Manjaro only.
- 0.2 Android build. Debug APK and USB live reload verified on the phone, workflow runs through Bun. Includes a 16 KB page alignment fix.
- 0.4 Web client spike: core runs in wasm in a worker, SQLite persists in OPFS across reload and restart, cancel by worker restart (about 2 s), panic recovery. Reviewed and merged (PR #5).
- 0.5 Scheduling spike: `fsrs` 6.6.2 chosen (ADR 0004). Intervals 2, 11, 46, 163, 497 on desktop, native and wasm. Optimiser works on wasm (66 ms for 2,000 cards). Phone unchecked. Awaiting review.
- 0.3 UI-to-core bridge. 0.3a (call path, PR #3) and 0.3b (notices, progress, cancellation, events and attachments, PR #4) reviewed by Anthony and merged. Verified on desktop and the phone. Attachment round trip 1 MB: 40 ms desktop, 75 ms phone. 5 MB: 181 ms desktop, 250 ms phone.

## Remaining in this step

- Anthony: review the branch. On the phone (installed, debug APK), unlock it, open the app and tap "Schedule 5 Good reviews" (expect 2, 11, 46, 163, 497) and "Optimise on 200 cards".
- Then merge. Next: 0.6 Sandboxed card rendering spike (Opus, /adr 0.6).

## Open items

- Installed for the 0.4 experiment (per-user, approved): the `wasm32-unknown-unknown` target for toolchain 1.98.1 and `wasm-bindgen-cli` 0.2.129 in `~/.cargo/bin`. `~/.cargo/bin` is not on the PATH of Claude's shell, so call it by full path or add it to PATH (not done).
- Web (0.4): verified only in headless Brave (Chromium) on Linux, built and run through Bun. Firefox, Safari, mobile browsers and Windows are unverified. Measurements are in ADR 0003 build notes.
- Android: call path, progress, cancel, events and attachments verified (0.3a, 0.3b). Reload behaviour of the notice channel on the phone is not.
- The phone may still have the APK with the temporary 0.3b check harness. Run `bun run android:build` then `bun run android:install` to replace it.
- Windows: desktop launch and `cargo xtask check` from step 0.1 are unverified and deferred to a manual check by Anthony.
- Step 2.1 must handle edge-to-edge drawing and safe areas (status bar, navigation bar, cutout, keyboard) once in the app shell.
- App/bundle ID is still the placeholder `dev.placeholder.flashcards`. Pick it before step 2.7.
