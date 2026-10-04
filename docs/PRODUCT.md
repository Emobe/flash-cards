# Product brief

## What this is

A modern spaced repetition flashcard app and ecosystem, in the same space as Anki. A Rust core, Tauri apps for desktop and mobile with a React UI, a web client, and a self-hostable sync server.

## Who it is for

- **Now:** Anthony, learning Polish.
- **Later:** anyone who uses flashcards, for any subject.

Features should be general-purpose. Language learning is the first use case, not the only one.

## Why someone would switch from Anki

1. **Easy to use.** Modern, clear, not clunky. A new user can study within minutes without learning jargon first.
2. **Real parity between mobile and desktop.** Anything you can do on desktop, you can do comfortably on a phone, including finding, filtering and editing cards.
3. **Sync that just works.** No "keep this side or that side" prompts, no website detours.

## Problems with Anki we are designing against

These are feature inputs, not code references.

- Dated, confusing UI and steep onboarding.
- Sync sometimes forces a one-way choice where one device's changes are lost. Error messages are vague.
- Mobile apps are mainly reviewers. Managing, filtering and editing cards on a phone is painful.
- Add-ons are desktop only and often break on updates.
- Card creation is slow, especially for language learners mining sentences.
- Large review backlogs cause people to quit.
- Shared decks are upload-and-forget. No collaboration or versioning without a paid third party.

## Requirements

### Platforms

- Desktop: Linux and Windows first. macOS when available.
- Mobile: Android first. iOS when a Mac is available.
- Web: a full client that works offline once loaded, not a cut-down companion.
- One React UI shared across all platforms. How the web client shares the core with the Tauri apps is decided by ADR.

### Mobile and desktop parity

- Every user-facing feature works on mobile from the start, including browsing, filtering, bulk editing, note type editing and template editing.
- Mobile layouts are designed for one-handed phone use, not shrunk desktop screens.
- Desktop gets keyboard shortcuts. Mobile gets large tap targets and sensible gestures.

### Sync

- Every device works fully offline.
- Changes from multiple devices merge automatically. No "choose a side" prompts in normal use.
- Review history is never lost in a merge.
- Devices running different app versions must not corrupt each other's data.
- Sync happens automatically in the background, with a clear status indicator.
- When something does go wrong, the error says exactly what and what to do.
- Media syncs efficiently and never re-uploads unchanged files.
- The sync server is self-hostable. Anthony hosts the main instance initially, at zero cost (for example on his own machine or a free tier) until further notice.

### Accounts

- Accounts are required for sync.
- The app must be usable locally without an account (useful for testing, and for people who never sync).
- Account deletion and full data export are supported (UK GDPR).

### Notes, cards and templates

- Notes hold fields. Note types define fields and card templates. One note can generate several cards.
- Built-in note types at minimum: Basic, Basic and reversed, Cloze.
- Templates support HTML and CSS, field substitution, conditionals and cloze.
- Images and audio in cards.
- Card content is untrusted (it may come from shared decks). It must render in a sandbox with no access to app data or app commands.

### Scheduling

- FSRS, with per-user parameter optimisation from review history.
- Deck option presets (daily limits, desired retention, learning steps).
- Undo for the last answer.
- Review debt features are a later phase, but nothing in the design should block them.

### Search and browse

- A powerful search and filter system (by deck, tag, note type, field content, card state, due date, review history).
- Saved searches.
- Bulk actions: move, tag, suspend, delete, reschedule.

### Fast capture

- Get material into a rough card in seconds from anywhere (share from another app, quick-add, later a browser extension).
- Rough cards land in an inbox and can be completed later.

### Data ownership

- Full export and backup in an open, documented format.
- Import from Anki `.apkg`, including review history if feasible.
- Automatic local backups.

### Extensibility

- Add-ons are a later phase, but the core exposes stable extension points (events and hooks) from the start so add-ons do not depend on internals.
- Add-ons must work on all platforms, not just desktop, and run sandboxed.

### Later

- Shared, versioned, collaborative decks and a deck library.
- Possible marketplace for decks and add-ons.
- Review debt features (catch-up modes, load smoothing).

## Tooling constraints

- **Zero running cost for the foreseeable future.** Apart from Anthony's Claude subscription, nothing may require paying for a service: no paid CI minutes, hosting, storage, email sending, domains, app store fees or code signing certificates. Every tool and service must have a free path, and any choice that would start costing money must be flagged to Anthony before it is made. Paid options can be noted in ADRs as "later, when funded".

- **Bun for everything Node-related.** Use Bun as the package manager and for running JavaScript and TypeScript scripts, dev servers and tests wherever practical. Do not use npm, yarn or pnpm, and do not commit their lockfiles.
- If a specific tool or step does not work with Bun, stop and tell Anthony what the problem is and the options before falling back to Node or another tool.
- This applies to the React UI, the web client, build scripts and any browser extension. The Rust core and sync server are not affected.

## Non-goals for now

- Paid features or monetisation.
- Paid infrastructure of any kind (see Tooling constraints).
- Google Play or app store publishing. Android builds are sideloaded APKs.
- Code-signed desktop installers. Unsigned builds are fine for personal use.
- iOS and macOS builds until a Mac is available.
- AI card generation.
- Real-time collaborative editing.

## Rules about Anki

- We write our own core. No code from Anki (AGPL) is read, copied or adapted.
- Anki is a source of feature ideas only. Feature requirements come from these docs, not from inspecting Anki's source.
- Reading the `.apkg` file format for import is fine, implemented from format documentation and observed files.

## Open product questions

- Name.
- Licence (open source at first, may go private later; keep the repo private or avoid outside contributions until decided).
- Final app/bundle ID (currently the placeholder `dev.placeholder.flashcards`). Pick it before step 2.7 (dogfood builds). Android treats a new ID as a different app, so changing it after real study data is on the phone means a separate install, and the old app's local data is lost when it is uninstalled. It does not have to match the final name.
