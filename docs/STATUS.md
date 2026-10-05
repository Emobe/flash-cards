# Status

Kept current by every session. A new session reads this first.

## Current step

0.3a UI-to-core bridge, call path: done, awaiting review. Plan: `docs/plans/0.3-bridge.md`, ADR 0002 (Accepted). 0.3b (notices, long operations, attachments) follows as a separate PR.

## Branch

`step/0.3a-bridge` (from `step/0.3-bridge`).

## Done

- Planning docs (`docs/`).
- 0.1 Repository and workspace boilerplate. Verified on Manjaro only.
- 0.2 Android build. Debug APK and USB live reload verified on the phone, workflow runs through Bun. Includes a 16 KB page alignment fix.
- 0.3a UI-to-core bridge, call path: code complete, `cargo xtask check` passes. Verified on desktop (value, error, no IPC warning, gating, type sync) and on the phone (value). Phone error path needs Anthony to tap Divide with divisor 0 (CLAUDE.md forbids tapping the phone). Ready for PR review.

## Remaining in this step

- Anthony: review 0.3a, check the divide error on the phone, merge. Then 0.3b (notices, long operations, attachments), per `docs/plans/0.3-bridge.md`.

## Open items

- Android: the call path works (0.3a). Attachments and events are still unverified there (0.3b).
- Windows: desktop launch and `cargo xtask check` from step 0.1 are unverified and deferred to a manual check by Anthony.
- Step 2.1 must handle edge-to-edge drawing and safe areas (status bar, navigation bar, cutout, keyboard) once in the app shell.
- App/bundle ID is still the placeholder `dev.placeholder.flashcards`. Pick it before step 2.7.
