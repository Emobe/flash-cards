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

**Status:** done (2026-10-05), merged. The phone showed the collection line (storage version 2). Built to ADR 0003 (build notes, step 1.1a). The CLI criterion is met by a minimal `crates/fc-cli` (`fc new`, `fc info`) that 1.14 extends.

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

**Status:** done (2026-10-05), merged (PR #10). Built to ADR 0006 (build notes, step 1.1b). One deviation: a collection that needs an unknown feature still opens, and `info()` lists the features (ADR 0006 section 10: sync pauses, study continues).

**Notes:** see `docs/plans/0.7-data-model-sync.md`.

---

## 1.2 Note types and fields

**Model:** Sonnet build (/step)

**Goal:** note types with fields and card templates, including the built-in types.

**Acceptance criteria:**
- Built-ins exist: Basic, Basic and reversed, Cloze.
- Create, rename and delete note types; add, rename, reorder and remove fields; add and remove templates.
- Changes that would affect existing notes are handled predictably and tested.

**Status:** done (2026-10-05), merged. Built to ADR 0006 (build notes, step 1.2). Two things were left for later steps: renaming a field does not rewrite `{{Field}}` references in templates (needs the 1.4 parser), and the notes-level tests run against a stand-in table until 1.3 adds notes. The CLI has `fc notetypes` to list; the commands that change note types are 1.14.

---

## 1.3 Notes and card generation

**Model:** Sonnet build (/step)

**Goal:** adding a note generates the right cards; editing a note keeps cards in step.

**Acceptance criteria:**
- Notes generate one card per applicable template; cloze notes generate one card per cloze number.
- Editing fields adds or removes cards correctly (for example, filling a previously empty field used by a template).
- Duplicate detection on the first field, as a warning not a block.

**Status:** done (2026-10-05), merged (PR #12). Built to ADR 0006 (build notes, step 1.3). The reading of templates that decides which cards exist was a small stand-in (`note::scan`), replaced in 1.4. Renaming a field now rewrites `{{OldName}}` in templates. The CLI has `fc notes` and `fc add-note`.

---

## 1.4 Template rendering

**Model:** Sonnet build (/step)

**Goal:** turn a card into front and back HTML ready for the sandbox.

**Acceptance criteria:**
- Field substitution, conditional sections, front side inclusion on the back, cloze rendering for front and back, references to media.
- Malformed templates produce a clear error, not a crash.
- Output is safe to hand to the sandbox from 0.6.

**Carried over from 1.2 and 1.3:** the built-in templates already use `{{Field}}`, `{{FrontSide}}` and `{{cloze:Field}}`, so define the grammar to accept them (or migrate them). Step 1.3 added `crate::note::scan`, a lenient scanner that decides which cards a note has (fields, filters, `{{#F}}` and `{{^F}}` sections, cloze numbers) and rewrites `{{OldName}}` when a field is renamed. Replace it with this step's parser so there is one reading of the language, and keep the tests in `note/tests.rs` passing.


**Status:** done (2026-10-05), merged (PR #13). Built to ADR 0006 (build notes, step 1.4). `fc_core::template` (lexer, parser with errors, cloze, renderer) replaced `note::scan`; `Collection::render_card` returns the front and back as full HTML documents plus the media names; saving a template with a syntax error is refused, a merge-made bad template still keeps its cards and gives a clear error when rendered. `fc render <file> <note ID>`. Left for 1.10: media in CSS `url()` and scripts (needs a `frame.html` change and 1.10's names). Not wired to the card frame until Phase 2.
---

## 1.5 Decks and deck options

**Model:** Sonnet build (/step)

**Goal:** nested decks and option presets.

**Acceptance criteria:**
- Create, rename, move (nest) and delete decks, with a defined behaviour for cards in a deleted deck.
- Option presets with daily new and review limits, learning steps, desired retention. Presets can be shared by several decks.

**Status:** done (2026-10-05), merged (PR #14). Built to ADR 0006 (build notes, step 1.5). `fc_core::deck`: decks nest through a `parent` register, cycles and missing parents are settled when reading, deleting a deck deletes its sub-decks, cards and notes left empty (restorable), the Default deck and preset cannot be deleted. Presets: daily limits, learning steps, desired retention, shared by several decks, deleting one sends its decks to the Default preset. `fc decks`, `fc add-deck`, `fc add-note --deck`. Left for 1.7 and 1.8: the FSRS parameters and relearning steps registers, and what the limits mean in queues. Not in any UI or the web API.

---

## 1.6 Tags

**Model:** Sonnet build (/step)

**Goal:** tags on notes, including hierarchical tags.

**Acceptance criteria:**
- Add, remove, rename tags, including renaming a parent tag.
- Tags are usable in search (1.9).

**Status:** done (2026-10-06), reviewed and merged (PR #15). Built to ADR 0006 (build notes, step 1.6). `fc_core::tag`, migration v6 (`note_tag`, a text-keyed register table): a tag's identity is its name, matched ignoring case, `::` makes a child, add, remove, set, rename (a parent renames what is inside it) and delete, `tags` (tree with counts), `notes_with_tag` for 1.9. `fc tags`, `fc tag`, `fc untag`, `fc rename-tag`, `fc add-note --tag`. Not in any UI or the web API.

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

**Status:** done (2026-10-06), awaiting review. ADR 0007 Accepted and `docs/plans/1.7-study-queues.md`, built as two PRs. **1.7a** (merged, PR #18): answering, card events, the schedule cache and its fold, the day boundary, undo, `fc answer`, `fc undo`, `fc schedule`. **1.7b** (migration v8): `fc_core::study` queue (`study_counts`, `deck_counts`, `next_card`) with limits over nested decks, siblings, order and spreading; suspend, bury and unbury; `fc due`, `fc next`, `fc suspend`, `fc unsuspend`, `fc bury`, `fc unbury`; the pinned 30-day simulation, also run in other time zones and across a daylight saving change. ADR 0007 build notes list the choices. Not in any UI or the web API.

---

## 1.8 Review history and basic stats

**Model:** Sonnet build (/step)

**Goal:** queryable review history and the numbers the UI will need.

**Acceptance criteria:**
- Per-card history.
- Daily counts, retention, due forecast for the next 30 days.
- FSRS parameter optimisation can run from history.

**Status:** done (2026-10-06), reviewed and merged (PR #20). Built to ADR 0007 (build notes, step 1.8), no migration and no new dependency. `fc_core::stats`: `card_history`, `daily_counts` (by study day and state, with time spent), `retention`, `due_forecast` (overdue, today and the next N days), `optimisation_data` / `optimise_preset` and `PresetChange.fsrs_parameters`. `fc history`, `fc stats`, `fc forecast`, `fc optimise [--apply]`. Found: `fsrs` panics on a training item with only same-day reviews, now filtered; training needs at least 64 items. Applying parameters does not recompute existing cards' memory (a later reschedule event). Not in any UI or the web API.

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

**Status:** done (2026-10-06), reviewed and merged (PR #21). Built to the plan approved in chat; the syntax, rules, timings and deviations are in ADR 0006 (build notes, step 1.9). `fc_core::search`: a query language (text, `field:`, `deck:`, `tag:`, `note:`, `card:`, `is:`, `due:`, `added:`, `rated:`, `introduced:`, `difficulty:`, `stability:`, `lapses:`, `reviews:`, with `or`, `-` and brackets), ten sorts, paging, notes or cards, saved searches (migration v9, a synced table), Unicode-aware matching through SQL functions (`rusqlite` `functions` feature). `fc search`, `fc searches`, `fc save-search`, `fc run-search`, `fc delete-search`. At 50,000 cards most queries take 40 to 160 ms, a text word about 290 ms. Not in any UI or the web API (3.1).

---

## 1.10 Media

**Model:** Sonnet build (/step)

**Goal:** store and reference images and audio.

**Acceptance criteria:**
- Add media, reference it from fields, find unused media, find missing media.
- Identical files are stored once.
- Works with the sync strategy from the ADR.

**Status:** done (2026-10-06), awaiting review. Split in two as agreed in chat: this part is the core, the second part is 1.10b below. Built to ADR 0006 (build notes, step 1.10). Migration v10 (`media_file` synced, `media_blob` local, the bytes in the collection file). `fc_core::media`: names `<stem>-<16 hex of the SHA-256>.<ext>`, a media-file ID made from the hash (so two devices that add the same bytes write one entity), add, read, delete and restore, references from fields, templates and CSS (`url()` too), `check_media` (unused, missing, without bytes), a deleted file that a note names stays alive. `fc add-media`, `fc media`, `fc media-get`, `fc media-check`, `fc delete-unused-media`. `sha2` is a direct dependency now (already in the lockfile). Not in any UI or the web API.

---

## 1.10b Media in the card frame

**Model:** Sonnet build (/step)

**Goal:** a card can use media from CSS `url()` and from scripts, and the core lists those names.

**Acceptance criteria:**
- `frame.html` rewrites `url(name)` in the card's CSS and gives scripts a way to get a blob URL for a media file the card was given, still inside the sandbox (ADR 0005, the "Media" and "Card scripts" notes).
- `render_card`'s media list includes the names used in CSS `url()` (core: `html::css_media_names` exists since 1.10).
- The 54-attempt malicious card from 0.6 still gets 0 successes on desktop, the phone and the web, and the sample card still works on all three.

**Status:** done (2026-10-06), awaiting review. Built to the plan approved in chat; results, the script API (`fcMedia.url(name)`) and the deviations are in ADR 0005 (build notes, step 1.10b). `render_card`'s media list has the note type CSS `url()` names, `frame.html` rewrites `url()` in `<style>` and `style=""`. The 54-attempt card: 38 blocked, 0 SUCCEEDED, 16 other on desktop, the phone and the web (dev and production), same as 0.6; the sample card and the new Media card work on all three. A font from a blob URL is not verified.

**Notes:** this changes the trusted `frame.html`, so it needs Anthony's close review and the sandbox test re-run. The card sandbox spike code (`CardSandboxSpike`, its cards file, `spikeCardMedia`, the sample media) goes when `fc-api` has a media method (3.1) or Phase 2 shows a real card, not here.

---

## 1.11 Change tracking for sync

**Model:** Opus design session (/adr), then Sonnet build (/step)

**Goal:** everything the sync ADR says Phase 1 must record is recorded, and two collections merge as ADR 0006 describes.

**Acceptance criteria:**
- Every user-visible change produces the tracking data defined in the ADR (the foundation is from 1.1b; this step checks every table against it).
- A merge function applies remote registers and card events, advances the HLC and recomputes caches.
- Tests simulate two collections making changes independently and merging them locally, covering the edge cases listed in step 0.7.

**Review focus:** compare against the ADR line by line.

**Status:** design accepted (2026-10-06), ready to build.
- ADR 0008 (Proposed) covers:
  - the change records;
  - the merge, and what it recomputes;
  - values that do not fit;
  - unknown rows;
  - the superseded-values log, postponed on review (the later edit wins and nothing else is kept);
  - the shape of purge and per-entity `requires`.
- The plan is `docs/plans/1.11-merge.md`: one PR, **1.11a** (merge, audit, edge-case tests,
  `fc merge`). ADR 0008 is Accepted, with Anthony's answers under its "Decisions on review".

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
