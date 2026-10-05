# Status

Kept current by every session. A new session reads this first.

## Current step

0.3b UI-to-core bridge, notices, long operations, attachments: in progress. Plan: `docs/plans/0.3-bridge.md`, ADR 0002 (Accepted). Follows 0.3a (merged).

## Branch

`step/0.3b-bridge-notices` (from `master`).

## Done

- Planning docs (`docs/`).
- 0.1 Repository and workspace boilerplate. Verified on Manjaro only.
- 0.2 Android build. Debug APK and USB live reload verified on the phone, workflow runs through Bun. Includes a 16 KB page alignment fix.
- 0.3a UI-to-core bridge, call path. Merged (PR #3). Verified on desktop and on the phone (value and the divide-by-zero error, checked by Anthony).

## Remaining in this step

0.3b done so far: `fc-api` notices, operation context, attachments and debug methods (commit 2); `fc-native` `subscribe`, `cancel`, operations map, throttled notice hub, base64 attachments in and framed attachments out (commit 3, Rust tests pass).

Still to do: `core-client` (operation IDs, `signal`, `onProgress`, `onEvent`, attachment typing, fake transport), Tauri transport (`subscribe`, `cancel`, base64), TS tests, gating check on desktop, Android timings for 1 MB and 5 MB attachments into ADR 0002, ADR and README notes, then `cargo xtask check` and the PR report.

## Open items

- Android: the call path works (0.3a). Attachments and events are still unverified there (0.3b).
- Windows: desktop launch and `cargo xtask check` from step 0.1 are unverified and deferred to a manual check by Anthony.
- Step 2.1 must handle edge-to-edge drawing and safe areas (status bar, navigation bar, cutout, keyboard) once in the app shell.
- App/bundle ID is still the placeholder `dev.placeholder.flashcards`. Pick it before step 2.7.
