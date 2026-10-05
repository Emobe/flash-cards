# Status

Kept current by every session. A new session reads this first.

## Current step

0.4 Web client spike: design done, awaiting Anthony's review. ADR 0003 (Proposed) and `docs/plans/0.4-web-spike.md`. Once accepted: Sonnet build, `/step 0.4`.

## Branch

`step/0.4-web-spike` (from `master`). Not pushed yet.

## Done

- Planning docs (`docs/`).
- 0.1 Repository and workspace boilerplate. Verified on Manjaro only.
- 0.2 Android build. Debug APK and USB live reload verified on the phone, workflow runs through Bun. Includes a 16 KB page alignment fix.
- 0.3 UI-to-core bridge. 0.3a (call path, PR #3) and 0.3b (notices, progress, cancellation, events and attachments, PR #4) reviewed by Anthony and merged. Verified on desktop and the phone. Attachment round trip 1 MB: 40 ms desktop, 75 ms phone. 5 MB: 181 ms desktop, 250 ms phone.

## Remaining in this step

- Anthony: review ADR 0003 and the plan, then accept or adjust. Nothing is built until it is Accepted.
- Build session: everything in the plan.

## Open items

- Installed for the 0.4 experiment (per-user, approved): the `wasm32-unknown-unknown` target for toolchain 1.98.1 and `wasm-bindgen-cli` 0.2.129 in `~/.cargo/bin`. `~/.cargo/bin` is not on the PATH of Claude's shell, so call it by full path or add it to PATH (not done).
- Web (0.4 design): verified only in headless Brave (Chromium) on Linux. Firefox, Safari and mobile browsers are unverified.
- Android: call path, progress, cancel, events and attachments verified (0.3a, 0.3b). Reload behaviour of the notice channel on the phone is not.
- The phone may still have the APK with the temporary 0.3b check harness. Run `bun run android:build` then `bun run android:install` to replace it.
- Windows: desktop launch and `cargo xtask check` from step 0.1 are unverified and deferred to a manual check by Anthony.
- Step 2.1 must handle edge-to-edge drawing and safe areas (status bar, navigation bar, cutout, keyboard) once in the app shell.
- App/bundle ID is still the placeholder `dev.placeholder.flashcards`. Pick it before step 2.7.
