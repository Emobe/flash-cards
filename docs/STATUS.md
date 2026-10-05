# Status

Kept current by every session. A new session reads this first.

## Current step

0.3a UI-to-core bridge, call path (Sonnet build, `/step 0.3a`). Plan: `docs/plans/0.3-bridge.md`, ADR 0002 (Accepted). 0.3b (notices, long operations, attachments) follows as a separate PR.

## Branch

`step/0.3a-bridge` (from `step/0.3-bridge`).

## Done

- Planning docs (`docs/`).
- 0.1 Repository and workspace boilerplate. Verified on Manjaro only.
- 0.2 Android build. Debug APK and USB live reload verified on the phone, workflow runs through Bun. Includes a 16 KB page alignment fix.
- 0.3a plan items 1 to 10 (code): `crates/fc-api`, `cargo xtask bindings`, `packages/core-client`, `fc-native` `call` command with `AppManifest` gating and `allow-call`, CSP, Tauri transport, UI (`CoreProvider`, divide form). `cargo xtask check` passes. Not yet run in the real app.

## Remaining in this step

- 0.3a: manual checks on desktop (value, error, no IPC warning, ungranted command rejected) and phone, type-sync check, ADR 0001 update, PR report.

## Open items

- The CSP from step 0.1 blocks Tauri's fast IPC path (found in the 0.3 design session). To be fixed in 0.3a.
- Android behaviour of the bridge is unverified (read from Tauri's source only). To be checked on the phone in 0.3a and 0.3b.
- Windows: desktop launch and `cargo xtask check` from step 0.1 are unverified and deferred to a manual check by Anthony.
- Step 2.1 must handle edge-to-edge drawing and safe areas (status bar, navigation bar, cutout, keyboard) once in the app shell.
- App/bundle ID is still the placeholder `dev.placeholder.flashcards`. Pick it before step 2.7.
