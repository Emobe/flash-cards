# Status

Kept current by every session. A new session reads this first.

## Current step

0.3b UI-to-core bridge, notices, long operations, attachments: done, awaiting review. Plan: `docs/plans/0.3-bridge.md`, ADR 0002 (Accepted, build notes for 0.3b added). With 0.3a merged, step 0.3 is complete once this merges. Next: 0.4 web client spike (Opus, `/adr 0.4`).

## Branch

`step/0.3b-bridge-notices` (from `master`). Not pushed yet.

## Done

- Planning docs (`docs/`).
- 0.1 Repository and workspace boilerplate. Verified on Manjaro only.
- 0.2 Android build. Debug APK and USB live reload verified on the phone, workflow runs through Bun. Includes a 16 KB page alignment fix.
- 0.3a UI-to-core bridge, call path. Merged (PR #3). Verified on desktop and on the phone (value and the divide-by-zero error, checked by Anthony).
- 0.3b notices, progress, cancellation, events and attachments. `cargo xtask check` passes. Verified in a built desktop app and the phone APK. Attachment round trip 1 MB: 40 ms desktop, 75 ms phone. 5 MB: 181 ms desktop, 250 ms phone.

## Remaining in this step

- Anthony: review 0.3b and merge. The phone currently has an APK built with a temporary check harness that has since been removed: run `bun run android:build` then `bun run android:install` to replace it.

## Open items

- Android: call path, progress, cancel, events and attachments verified (0.3a, 0.3b). Reload behaviour of the notice channel on the phone is not.
- Windows: desktop launch and `cargo xtask check` from step 0.1 are unverified and deferred to a manual check by Anthony.
- Step 2.1 must handle edge-to-edge drawing and safe areas (status bar, navigation bar, cutout, keyboard) once in the app shell.
- App/bundle ID is still the placeholder `dev.placeholder.flashcards`. Pick it before step 2.7.
