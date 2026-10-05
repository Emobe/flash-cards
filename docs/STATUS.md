# Status

Kept current by every session. A new session reads this first.

## Current step

0.4 Web client spike: design session in progress (Opus, `/adr 0.4`). Writing ADR 0003 and `docs/plans/0.4-web-spike.md`.

## Branch

`step/0.4-web-spike` (from `master`). Not pushed yet.

## Done

- Planning docs (`docs/`).
- 0.1 Repository and workspace boilerplate. Verified on Manjaro only.
- 0.2 Android build. Debug APK and USB live reload verified on the phone, workflow runs through Bun. Includes a 16 KB page alignment fix.
- 0.3 UI-to-core bridge. 0.3a (call path, PR #3) and 0.3b (notices, progress, cancellation, events and attachments, PR #4) reviewed by Anthony and merged. Verified on desktop and the phone. Attachment round trip 1 MB: 40 ms desktop, 75 ms phone. 5 MB: 181 ms desktop, 250 ms phone.

## Remaining in this step

- Write ADR 0003 (Proposed) and the build plan, then stop for Anthony's review.

## Open items

- Android: call path, progress, cancel, events and attachments verified (0.3a, 0.3b). Reload behaviour of the notice channel on the phone is not.
- The phone may still have the APK with the temporary 0.3b check harness. Run `bun run android:build` then `bun run android:install` to replace it.
- Windows: desktop launch and `cargo xtask check` from step 0.1 are unverified and deferred to a manual check by Anthony.
- Step 2.1 must handle edge-to-edge drawing and safe areas (status bar, navigation bar, cutout, keyboard) once in the app shell.
- App/bundle ID is still the placeholder `dev.placeholder.flashcards`. Pick it before step 2.7.
