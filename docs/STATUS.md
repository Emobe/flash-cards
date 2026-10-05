# Status

Kept current by every session. A new session reads this first.

## Current step

0.3 UI-to-core bridge. Model: Opus design session (`/adr 0.3`), then Sonnet build (`/step 0.3`).

## Branch

`step/0.3-bridge`: ADR 0002 (Proposed) and `docs/plans/0.3-bridge.md`, waiting for Anthony's review. Step 0.2 is merged to `master` (PR #1).

## Done

- Planning docs (`docs/`).
- 0.1 Repository and workspace boilerplate. Verified on Manjaro only.
- 0.2 Android build. Debug APK and USB live reload verified on the phone, workflow runs through Bun. Includes a 16 KB page alignment fix.

## Remaining in this step

- 0.3: Anthony reviews ADR 0002 and the plan (proposed split into PRs 0.3a and 0.3b). After acceptance, Sonnet builds from the plan (`/step 0.3`).

## Open items

- The CSP from step 0.1 blocks Tauri's fast IPC path (found in the 0.3 design session). To be fixed in 0.3a.
- Android behaviour of the bridge is unverified (read from Tauri's source only). To be checked on the phone in 0.3a and 0.3b.
- Windows: desktop launch and `cargo xtask check` from step 0.1 are unverified and deferred to a manual check by Anthony.
- Step 2.1 must handle edge-to-edge drawing and safe areas (status bar, navigation bar, cutout, keyboard) once in the app shell.
- App/bundle ID is still the placeholder `dev.placeholder.flashcards`. Pick it before step 2.7.
