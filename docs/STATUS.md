# Status

Kept current by every session. A new session reads this first.

## Current step

0.3 UI-to-core bridge. Model: Opus design session (`/adr 0.3`), then Sonnet build (`/step 0.3`).

## Branch

None started for 0.3. Step 0.2 is built and recorded as done on `step/0.2-android` (not yet merged to `master` at the time of writing).

## Done

- Planning docs (`docs/`).
- 0.1 Repository and workspace boilerplate. Verified on Manjaro only.
- 0.2 Android build. Debug APK and USB live reload verified on the phone, workflow runs through Bun. Includes a 16 KB page alignment fix.

## Remaining in this step

- 0.3: ADR for the UI-to-core bridge (Proposed, then Accepted by Anthony), then the build.

## Open items

- Windows: desktop launch and `cargo xtask check` from step 0.1 are unverified and deferred to a manual check by Anthony.
- Step 2.1 must handle edge-to-edge drawing and safe areas (status bar, navigation bar, cutout, keyboard) once in the app shell.
- App/bundle ID is still the placeholder `dev.placeholder.flashcards`. Pick it before step 2.7.
