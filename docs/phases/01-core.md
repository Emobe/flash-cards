# Phase 1: Core library

## Goal

The Rust core implements the product's domain with thorough tests, driven from a dev CLI. No UI work in this phase.

Everything is built according to the accepted ADRs from Phase 0, especially the data model and sync strategy.

## Exit criteria

- Through the CLI alone: create note types, decks and notes, render cards, study a simulated day with correct scheduling, search, attach media, back up and restore.
- Change tracking for sync is in place as designed in the sync ADR.
- Tests cover scheduling, templates, search and change tracking thoroughly.
- Core runs on desktop, Android and in the browser (build check at minimum).

---

## 1.1a Collection storage and migrations

**Model:** Sonnet build (/step)

**Goal:** a collection can be created, opened and closed, with a migration system for future schema changes.

**Acceptance criteria:**
- Create, open and close a collection from the CLI.
- A test migration upgrades an older collection.
- Opening a collection from a newer app version fails safely with a clear error.
- Storage works on all targets per the web ADR.

**Review focus:** how painful will future schema changes be?

**Status:** done (2026-10-05), pending Anthony's check of the phone (see `docs/STATUS.md`). Built to ADR 0003 (build notes, step 1.1a). The CLI criterion is met by a minimal `crates/fc-cli` (`fc new`, `fc info`) that 1.14 extends.

---

## 1.1b Sync foundation

**Model:** Sonnet build (/step)

**Goal:** everything later steps need so that every synced table is tracked from the start (ADR 0006, section 13, items 1 to 3 and 7).

**Acceptance criteria:**
- 128-bit IDs (UUIDv7 from host time and `getrandom`), stored as 16-byte blobs and exposed as UUID strings. `getrandom` checked in a browser.
- Time comes from a host-supplied `Clock` on native, web and in tests; `fc-core` never reads the clock.
- A hybrid logical clock saved in the collection, and a device ID that is regenerated when a collection is copied, restored or imported.
- One write path that records `(hlc, device, pushed?)` per register in the same transaction, with a test that fails if a synced table can be written without it.
- A generic store for unknown entity types and registers, and collection-level `requires` checked on open.

**Review focus:** can any later step write a synced table without the clock being recorded?

**Status:** done (2026-10-05), pending Anthony's review. Built to ADR 0006 (build notes, step 1.1b). One deviation: a collection that needs an unknown feature still opens, and `info()` lists the features (ADR 0006 section 10: sync pauses, study continues).

**Notes:** see `docs/plans/0.7-data-model-sync.md`.

---

## 1.2 Note types and fields

**Model:** Sonnet build (/step)

**Goal:** note types with fields and card templates, including the built-in types.

**Acceptance criteria:**
- Built-ins exist: Basic, Basic and reversed, Cloze.
- Create, rename and delete note types; add, rename, reorder and remove fields; add and remove templates.
- Changes that would affect existing notes are handled predictably and tested.

---

## 1.3 Notes and card generation

**Model:** Sonnet build (/step)

**Goal:** adding a note generates the right cards; editing a note keeps cards in step.

**Acceptance criteria:**
- Notes generate one card per applicable template; cloze notes generate one card per cloze number.
- Editing fields adds or removes cards correctly (for example, filling a previously empty field used by a template).
- Duplicate detection on the first field, as a warning not a block.

---

## 1.4 Template rendering

**Model:** Sonnet build (/step)

**Goal:** turn a card into front and back HTML ready for the sandbox.

**Acceptance criteria:**
- Field substitution, conditional sections, front side inclusion on the back, cloze rendering for front and back, references to media.
- Malformed templates produce a clear error, not a crash.
- Output is safe to hand to the sandbox from 0.6.

---

## 1.5 Decks and deck options

**Model:** Sonnet build (/step)

**Goal:** nested decks and option presets.

**Acceptance criteria:**
- Create, rename, move (nest) and delete decks, with a defined behaviour for cards in a deleted deck.
- Option presets with daily new and review limits, learning steps, desired retention. Presets can be shared by several decks.

---

## 1.6 Tags

**Model:** Sonnet build (/step)

**Goal:** tags on notes, including hierarchical tags.

**Acceptance criteria:**
- Add, remove, rename tags, including renaming a parent tag.
- Tags are usable in search (1.9).

---

## 1.7 Study queues and answering

**Model:** Opus design session (/adr), then Sonnet build (/step)

**Goal:** the core decides what to study next and records answers.

**Acceptance criteria:**
- Queues respect deck limits, nested decks and learning steps.
- Answering a card updates scheduling per the scheduling ADR and records a review.
- Undo of the last answer.
- Suspend, bury and unbury.
- Day boundary behaviour is configurable and tested across time zones.
- A simulated multi-day test produces expected due counts.

**Review focus:** this is the heart of the app. Read the tests.

---

## 1.8 Review history and basic stats

**Model:** Sonnet build (/step)

**Goal:** queryable review history and the numbers the UI will need.

**Acceptance criteria:**
- Per-card history.
- Daily counts, retention, due forecast for the next 30 days.
- FSRS parameter optimisation can run from history.

---

## 1.9 Search and filtering

**Model:** Sonnet build (/step)

**Goal:** a search system powerful enough to make browsing on a phone painless.

**Acceptance criteria:**
- Filter by deck (including subdecks), tag, note type, field content, card state (new, learning, review, suspended), due date ranges, added and reviewed date ranges, ease or difficulty.
- Combine with and, or, not.
- Sorting options.
- Saved searches.
- Fast enough on a 50,000-card collection (target agreed in the plan for this step).

**Review focus:** is the query syntax something a normal person could use, with the UI also offering filters as buttons?

---

## 1.10 Media

**Model:** Sonnet build (/step)

**Goal:** store and reference images and audio.

**Acceptance criteria:**
- Add media, reference it from fields, find unused media, find missing media.
- Identical files are stored once.
- Works with the sync strategy from the ADR.

---

## 1.11 Change tracking for sync

**Model:** Opus design session (/adr), then Sonnet build (/step)

**Goal:** everything the sync ADR says Phase 1 must record is recorded, and two collections merge as ADR 0006 describes.

**Acceptance criteria:**
- Every user-visible change produces the tracking data defined in the ADR (the foundation is from 1.1b; this step checks every table against it).
- A merge function applies remote registers and card events, advances the HLC and recomputes caches.
- Tests simulate two collections making changes independently and merging them locally, covering the edge cases listed in step 0.7.

**Review focus:** compare against the ADR line by line.

---

## 1.12 Extension points

**Model:** Sonnet build (/step)

**Goal:** stable events and hooks for future add-ons and the UI.

**Acceptance criteria:**
- Events for at least: note added, edited, deleted; card answered; study session started and ended; sync completed (stub).
- Documented as a public surface, separate from internals.

---

## 1.13 Backup, export and restore

**Model:** Sonnet build (/step)

**Goal:** a full export in our own documented format, and automatic backups.

**Acceptance criteria:**
- Export a whole collection or a single deck, including media and optionally history.
- Restore from backup.
- Format documented in `docs/`.

---

## 1.14 Developer CLI

**Model:** Sonnet build (/step)

**Goal:** a CLI that exercises the core for manual testing and debugging.

**Acceptance criteria:**
- Covers everything above.
- Can generate a large fake collection for performance testing.

**Notes:** this can grow alongside 1.1 to 1.13 rather than being built last. Claude Code decides.
