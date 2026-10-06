# 0008: Merging collections: change records, the merge and superseded values

Status: Accepted
Date: 2026-10-06
Accepted: 2026-10-06, with the changes under "Decisions on review".

Anthony's review answers are under "Decisions on review". In short: the superseded-values log
(part 7) is postponed, so the later edit wins and nothing else is kept; purge and per-entity
`requires` are built later; one PR.

## Context

Step 1.11's acceptance criteria:

- Every user-visible change produces the tracking data ADR 0006 defines (1.1b built the foundation;
  this step checks every table against it).
- A merge function applies remote registers and card events, advances the HLC and recomputes
  caches.
- Tests make changes in two collections independently, merge them locally and cover the edge cases
  of step 0.7 (ADR 0006, section 11).

`docs/plans/0.7-data-model-sync.md` also gives 1.11 a design pass on three details ADR 0006 left
open: wire-neutral change records, the superseded-values log, and purge.

ADR 0006 already decides a lot: field-level registers that merge by the higher `(hlc, device)`, card
events merged by union, soft deletes, read-time rules for what the database cannot enforce, the
unknown-data store, and a merge function in `fc-core` that step 1.11 tests and Phase 4 reuses
(section 13, item 9). This ADR does not reopen any of that.

What steps 1.1b to 1.10 built, which the merge has to fit:

- **Three kinds of synced table.**
  - `SyncedTable`: one column per register, `id` primary key. There are 11, from `requirement` to
    `media_file`.
  - `DynamicTable`: one row per `(owner, key)`. These are `note_field_value` (the register name is the
    field ID) and `note_tag` (the register name is the tag text).
  - `AppendOnlyTable`: immutable rows merged by ID. These are `card_event` and `fsrs_parameter_set`.
- **`register_clock`** holds `(hlc, device, pushed)` per register. `unknown_register` keeps registers
  this build does not know. `unpushed_row` lists append-only rows not pushed yet.
- **Guard triggers** refuse any write outside `Collection::write`, and refuse every DELETE.
- **Two derived things.** The schedule cache is folded from events (`study::fold::rebuild_card`). The
  cards of a note come from its fields and note type (`note::generate::reconcile`).
- **Tests already fake remote writes** with raw SQL that opens the guard and writes a clock row
  (`deck/tests.rs`, `study/queue_tests.rs`, `study/fold_tests.rs`). 1.1b's build notes said the merge
  needs its own write method that opens the guard. It does not exist yet.

## Findings

Checked 2026-10-06. Experiments were throwaway, in the session scratchpad outside the repo, with
Python 3 and its `sqlite3` module on Linux.

1. **ADR 0006's rule for the superseded-values log misses about half the losing values.** Section 3
   says: when a remote register beats a local value that was never pushed, keep the local value. A
   simulation of the section 7 protocol tested this rule. Three devices edit four registers offline
   and sync at random. A sync pulls above its cursor, applies, then pushes. The server keeps only the
   latest value. Clocks are skewed by up to ±5 s. There were 500 runs and 120,088 writes.
   - The ground truth: a value is a **conflict loser** if it is not the final value and no write was
     made on top of it, directly or through later writes.
   - Results:

     | Rule: log the local value when a remote value replaces it, and | Losers missed (of 30,942) | Values logged that the winner had built on |
     | --- | --- | --- |
     | the local value was never pushed (ADR 0006) | 14,960 | 0 |
     | the local value was written on this device | 0 | 11,067 |
     | the local value was written here, and the remote value was not built on it (`base`, part 7) | 0 | 325 |

   - **The hole in the ADR 0006 rule.** Device A edits a field and syncs, so its value is pushed.
     Device B edits the same field later, offline, then syncs. B pulls A's value, which loses to B's,
     so B has nothing to log. Then B pushes. A pulls B's value, which replaces A's, but A's was already
     pushed, so nothing is logged anywhere. A's text is gone from every device. With 2,000 runs the
     proportions were the same (59,498 missed by the ADR 0006 rule, 0 by `base`).
   - **The 325.** These come from a chain through two other devices: C replaced A's value, D replaced
     C's, and the server kept only D's. A sees that D did not build on its value, and logs it. The
     user did see that value replaced, so the log entry is true but stale. One `base` clock cannot
     see two hops back.
2. **The cost of storing `base`.** A `register_clock` table of 1,500,000 rows (the size ADR 0006
   estimates for 50,000 notes) was 112 MB in SQLite. With `base_hlc` and `base_device` filled on every
   row it was 152 MB (+36%). A NULL column costs about a byte, so filling `base` only for the
   registers the log covers (part 7) keeps the cost to those registers.
3. **From reading the code** (no experiment):
   - **NOT NULL columns.** Every register column is `NOT NULL` with a default. So a remote NULL would
     abort the whole merge transaction. Readers use typed `row.get::<T>`, so a value of the wrong
     type would make later reads fail.
   - **Registers with no clock.** `card.deck` (v5), `card.suspended`, `card.buried_until`,
     `deck.limits_include_subdecks` and `options_preset.space_siblings` (v8) were added with column
     defaults and no clock rows. Rows made before those migrations hold the default with no clock.
     Every device holds the same default, so not sending them loses nothing.
   - **The merge must reconcile cards itself.** Note N has field F = "x", so its optional reverse
     card R exists. Device A clears F, which deletes R. Device B changes F to "y" at a later HLC, which
     writes nothing on R. After the merge, F = "y" (B wins) but R is deleted (only A wrote it). Neither
     device wrote that combination. Only reconciling N after the merge brings R back.

**Not verified:** any of this in Rust, on the phone or in a browser. The simulation models the protocol
of ADR 0006 section 7, not code that exists. The `base` storage cost was measured in Python's SQLite
on Linux, not in the app's schema. Merge speed was not measured: the build measures it (see the plan).

## Decisions and options

### 1. Change records

How a batch of changes looks in `fc-core`, without a wire encoding (step 4.3 picks the encoding).

- **A. Plain Rust structs, no encoding (chosen).**

  ```rust
  pub struct Changes { pub registers: Vec<RegisterChange>, pub rows: Vec<RowChange> }
  pub struct RegisterChange {
      pub entity: String,      // "note", "card", "note_tag", or a type this build does not know
      pub entity_id: Id,
      pub field: String,       // column name, field ID text, or tag text
      pub value: Value,        // rusqlite's Value: Null, Integer, Real, Text, Blob
      pub clock: Clock,        // (hlc, device)
      // A `base: Option<Clock>` field comes with part 7, which is postponed.
  }
  pub struct RowChange { pub entity: String, pub id: Id, pub columns: Vec<(String, Value)> }
  ```

  - These are the self-describing changes of ADR 0006 section 7: entity type and field names are
    strings, so unknown ones pass through.
  - `Value` is the type `UnknownRegister` already uses.
  - The two-collection tests pass a `Changes` from one collection to another in memory.
- **B. Choose a serialisation now (JSON or CBOR).** It would need a dependency, and the encoding,
  compression and paging are step 4.3's decisions, made with the server.
- **C. A SQLite file as the batch.** Attaching another database ties the batch to our local schema,
  which ADR 0006 section 10 keeps separate from the sync format.

### 2. Reading changes from a collection

- `Collection::changes(Selection::All | Selection::Unpushed)` returns a `Changes`.
  - **`All`:**
    - every register that has a clock: synced tables, both dynamic tables, and stored unknown
      registers;
    - every append-only row, including stored unknown rows and columns (part 6).

    This is what a new device needs (a pull from cursor 0), and what the two-collection tests use.
  - **`Unpushed`:** registers with `pushed = 0` and rows listed in `unpushed_row`. Phase 4 pushes
    these.
- **A register with no clock is not sent.** It holds the column default on every device (finding 3).
  The audit (part 10) checks this on every table.
- **Seeded registers** (`hlc 0`, nil device) are sent like any other. They are equal everywhere, so
  applying them does nothing.
- Marking changes as pushed, cursors and paging are Phase 4.

### 3. Applying a batch: `Collection::merge`

`Collection::merge(&Changes) -> Result<MergeReport, CollectionError>` applies one batch in one write
transaction. A failure rolls back all of it. The batch is idempotent, so a retry is safe.

- **Registers** can be applied in any order.
  - **Known register** (a `SyncedTable` register, or a dynamic one: a field-ID key for `note`, any key
    for `note_tag`):
    - **No local clock:** the remote value wins. A row that does not exist yet is inserted with this
      one value, and every other column takes its default. This is what "a card can arrive before
      its note" needs.
    - **Remote `(hlc, device)` higher:** the remote value wins. The value is written and the clock row
      gets the remote `hlc` and device, `pushed = 1` (it came from elsewhere) and its `base`.
    - **Equal or lower:** nothing changes.
  - **Unknown register:** goes to `unknown_register` with its existing rule, in the same transaction.
  - **A value that does not fit** is rejected (part 4).
- **Rows** go in with `INSERT OR IGNORE`. Rows that came from elsewhere are not added to
  `unpushed_row`. Unknown entity types and unknown columns go to the unknown-row store (part 6).
- **The HLC** moves up to the highest received register clock (`hlc_last = max(hlc_last, received)`),
  including clocks that lost. Event times are not fed into the HLC: they have their own rule (ADR
  0007, part 6).
- **Derived state** is updated last, in the same transaction (part 5).
- **The method is crate-private.** `WriteTx` gets crate-only `apply_register` and `apply_row`, used
  only by `merge` (and by tests that fake another device). The public surface is `changes` and
  `merge`.
- **`MergeReport`** counts registers applied, registers ignored (older or equal), unknown registers
  and rows stored, rows added, notes reconciled and cards rebuilt. It also lists the rejected changes, each with its reason.

### 4. Values that do not fit

A value's SQLite type must match the declared type of its column. Integer and real are both accepted
for a `REAL` column. Otherwise it is **rejected**: not applied, not stored, listed in the report. The
same check applies to the known columns of a row.

- Why: a NULL in a NOT NULL column would abort the merge, and a wrong type would break later reads
  (finding 3). ADR 0006 says merges never fail, and earlier steps made reading never fail.
- Every device rejects the same changes, so all devices still converge. Only a buggy or hostile client
  can send one. Phase 4 decides what the user sees ("3 changes from another device could not be
  applied"). It must say what and why.
- **Alternatives:**
  - Store it in `unknown_register`: never read, because the register is known, so it would only hide
    the problem.
  - Convert it (NULL to the default, text to a number): devices would then hold a value the server
    does not have, silently.

### 5. Derived state after a merge

- **Cards of notes.** The merge reconciles, after all registers are applied:
  - every live note whose `note` registers or field values the merge changed;
  - every live note of a note type whose `note_type`, `note_type_field` or `template` registers it
    changed (`reconcile_note_type`).

  A change to a card register does not trigger a reconcile. A card that went to the trash with its
  deck elsewhere must stay there, and the existing dead-deck rule covers the case of a card arriving
  before its deck's delete.
  - Reconcile writes are ordinary local writes (this device, a new HLC, unpushed). Every device
    computes the same wanted set from the same merged values, so all devices write the same values.
    Their clocks differ, but no value flips, and receiving an equal value does not reconcile again.
  - **Alternatives:**
    - Never reconcile on merge: a note can be left with a wanted card deleted (finding 3) until its
      next edit.
    - Make card existence derived instead of registers: this changes ADR 0006's card registers and
      the history kept for cards.
- **Schedule cache.** `rebuild_card` runs for every card that got a new event (reviews, voids and
  unknown kinds), and for every card with an event that names a parameter set that just arrived.
  Before the set arrived, the fold could only take that event's recorded result.
- **Nothing else.** The deck tree, "still referenced, so still alive", tag spelling, duplicate names
  and live media are read-time rules, so they need no update.

### 6. Unknown rows and columns

ADR 0007 left to 1.11 how a newer app's event fields are kept.

- **Chosen:** a local table `unknown_row_value (entity_type, row_id, column, value)`.
  - For a known table, the known columns go into the table, and any extra column a newer app added
    goes here.
  - For an unknown table, every column goes here.
  - `changes(All)` reassembles the row, so it is relayed unchanged.
- **Alternatives:**
  - An `extra` JSON column on `card_event`: needs a migration on an immutable table, and only covers
    that table.
  - Dropping the extra columns: breaks ADR 0006 section 10 ("unknown data is kept").
- **Adopting after an upgrade.** A migration that adds a register or column calls a helper,
  `adopt_unknown`. It moves the matching stored values into the real table (with their clocks, for
  registers) and removes them from the store. 1.1b's notes said "a migration applies what was
  stored". This is that code.
- **Rule for later migrations:**
  - A new column on an append-only table is nullable, because older apps send rows without it.
  - A new register has a column default that matches what an older device assumes.

### 7. The superseded-values log (postponed)

Postponed on review (see "Decisions on review"). Kept here as the design to build from later.

ADR 0006 section 3 and its "Decisions on review" item 2 promise that a value lost to a concurrent
edit is kept on the device that lost it. Finding 1 shows that its condition ("never pushed") misses
about half of those values.

- **A. Log a local value that was never pushed (ADR 0006).** Misses 48% of losers (finding 1).
- **B. Log any value this device wrote when a remote value replaces it.** Misses none, but also logs
  every ordinary "edited on the phone, edited again later on the desktop" (11,067 such values against
  30,942 real losers). A screen showing "this was also edited on another device" would be wrong most
  of the time.
- **C. A `base` clock on each write (chosen).**
  - A write records the clock of the value it replaced, its `base`. If the value it replaced was
    written by this same device, the write keeps that value's `base` instead, so a run of edits on one
    device still points at what the device started from.
  - When a remote value wins over a local one, the local value is logged if:
    - the local value was written by this device, and
    - the remote change's `base` is not the local value's clock (the other device did not build on
      it).
  - It missed none and logged 325 stale values (finding 1). This is the same idea as an event's
    `previous` (ADR 0007, part 6).
- **D. Version vectors per register.** Exact, but each register would carry a vector that grows with
  the number of devices, in the database, on the wire and on the server.

**Details of C:**

- **Kept registers.** These are the text people type:
  - note field values;
  - note type name and CSS;
  - field name;
  - template name, front and back;
  - deck name;
  - preset name;
  - saved search name and query.

  They are declared on the table (`SyncedTable::kept`, and a flag on `DynamicTable`, on for note
  values and off for tags). Other registers (flags, numbers, positions, parents, tag presence) are not
  logged: losing one of them loses no typing. ADR 0006 named "note fields, templates and names".
- **Storage.**
  - `register_clock` gets nullable `base_hlc` and `base_device`, filled only for kept registers
    (finding 2).
  - A local table `superseded_value` holds:
    - the entity, ID and register;
    - the lost value and its clock;
    - the winner's clock.

    Its key includes the lost value's clock, so merging the same batch twice logs once.
- **On the wire.** `RegisterChange.base` is set for kept registers only.
- **API.**
  - `superseded_values()`, optionally for one entity.
  - `dismiss_superseded(...)`.
  - Putting a lost value back is an ordinary edit through the existing API. There is no automatic
    pruning (losers are rare). Revisit if the table grows.
- **Values written before this migration** have no `base`. A missing base never matches, so the first
  conflict on such a register can log a value that was replaced knowingly. That happens once per
  register, and only on collections that synced before this step, of which there are none (Phase 4).

On acceptance, ADR 0006 section 3 gets a pointer here, because this replaces its "never pushed"
condition.

### 8. Purge: shape only, built with "Empty trash"

ADR 0006 section 5 has "Empty trash" write a purged state that nothing can undo. Nothing in Phase 1
needs it, and no step builds a trash screen yet. The shape is decided now, so the merge and the
format leave room for it:

- **A purge is an append-only row** of a new entity `purge`, with columns `entity_type` and
  `entity_id`. Its ID is `UUIDv5(namespace, entity_type ‖ entity_id)`, so two devices purging the
  same thing make one row. Rows merge by union, so nothing can undo a purge.
- **Effect, on every device that has the row:**
  - the entity's row, its dynamic rows (field values, tags) and its clock rows are deleted;
  - for a media file, its blob is deleted too;
  - any later register for that entity is dropped on arrival.

  These are the only hard deletes, through a guard opened for this purpose. Card events are never
  purged (ADR 0006 section 5).
- **Older apps:** an app without purge would re-create content from a later register. So the first
  purge writes the collection requirement `purge`, which pauses sync on older apps (section 10).
- **The server** (Phase 4) knows the `purge` entity by name and drops the stored registers of that
  entity. This is generic, not schema knowledge.
- **Alternative:** a sticky `purged` register that beats every other. It needs a merge rule that
  differs from every other register's, and the server would still need to know it.

### 9. Per-entity `requires`: shape only

ADR 0006 section 10 has an entity carry a `requires` register, and 1.1b left it "to the steps that
need them". None in Phase 1 does. But an app without the check would treat `requires` as unknown
data and keep editing the entity. So the check must ship in the first app version that syncs, not in
the first that writes `requires`.

- **Shape:** a register named `requires` on any entity type, holding feature names separated by
  spaces. The name is reserved for every entity.
- **Check:** `WriteTx` refuses `set` and `insert` on an entity whose `requires` names a feature this
  build does not support, with "Update the app to edit this".
- **When to build:** in Phase 4 before step 4.3 ships. Added to `STATUS.md` so it is not lost.
- **"Edit anyway" (Anthony, on review).** The read-only message also offers to make the item plain,
  so a person who cannot update the app now is not stuck:
  - It asks first, and names what will be lost: "This note uses a feature this version doesn't
    have. Edit it anyway? The parts this version can't show will be removed on all your devices."
  - If they agree, the older app writes ordinary changes: `requires` set to empty, and every
    register of that entity it holds only as unknown data set to empty. It can do this without
    understanding them, because the unknown store has their names. Then the entity is editable.
  - These are normal later edits. They sync and win like any other, and a device that later adds
    the feature again wins in turn.
  - **Rule for every future feature that writes `requires`:** it must define what its entity means
    with its own registers empty, and that must be a valid plain item (for example, a note with
    hidden image areas becomes an ordinary note with the image). The ADR for that feature says so.

### 10. Checking every table (the first acceptance criterion)

- **The table list.** ADR 0006 section 3's table is compared with the registry, and the differences
  are written in the build notes. Field `options`, card `flag` and the new-card position were never
  built. `collection_setting` has no `deleted` (ADR 0007). `requirement` was added. Section 10 allows
  registers to be added later.
- **Invariant checks**, run at the end of the tests below:
  - every register of every synced row either has a clock or holds its column default;
  - every append-only row written on this device is in `unpushed_row`.
- **The tour.** One test calls every public operation that writes. After it, `changes(All)` merged
  into a new, empty collection must reproduce:
  - every synced table, row for row;
  - the clocks;
  - the events;
  - the schedule cache;
  - what the reading APIs return (deck tree, notes, cards, tags, presets, searches, media list).

  Any user-visible state that is not tracked, or not applied by the merge, fails this. The plan lists
  the operations. A new operation that writes must be added to the tour, and the plan says so in the
  review checklist.

### 11. Tests for ADR 0006 section 11

Each row that does not need a server gets a two-collection test, in a new `sync/merge_tests.rs`. The
plan lists them.

- **Rows that need the server**, tested here only for the local part:
  - **A wrong device clock:** the HLC part only. A device hours ahead wins, and a local write after a
    merge beats everything received. The warning and the server's refusal are Phase 4.
  - **A corrupted database:** rebuilding a collection from another's `changes(All)` is tested. Reading
    unpushed changes out of a damaged file is Phase 4.
  - **A restored backup:** step 1.13.
- **A convergence test:**
  - three collections with skewed `ManualClock`s;
  - random operations through the public API;
  - random merges between pairs, in random directions;
  - then all merges until nothing changes.

  All three must then be identical (synced tables, clocks, events, schedule cache, read results), and
  re-applying any batch, or applying batches in another order, must change nothing.
- **An "older app" test.** A collection opened with a smaller test schema:
  - receives a register and a column it does not know;
  - relays them unchanged to a third collection;
  - adopts them after it is upgraded.

### 12. CLI

- `fc merge <from> <into>`: applies `changes(All)` of one collection file to another and prints the
  report.
- `fc superseded <file>`: lists the log, when part 7 is built.

Two files merged both ways is the manual test of this step.

## Consequences

- **The merge function ADR 0006 asked for exists**, with no server. Phase 4 adds transport, cursors,
  pushed marks, paging and the skew check around it, not merge logic.
- **The sync format reserves** the register name `requires` and the entity `purge`. A `base` clock
  on kept registers is added only if the superseded-values log is built.
- **ADR 0006 changes in one place:** its promise to keep a value lost to a concurrent edit of the same
  field is postponed. For now the later edit wins and the other is gone. Every other rule is
  unchanged.
- **New local table:** `unknown_row_value`. No synced table changes.
- **A merge can write.** Reconciling cards produces local, unpushed writes that every device makes
  identically. They cost some sync traffic, not correctness.
- **Rejected changes are possible** and must be shown by Phase 4.
- **Not built:** purge (with "Empty trash"), per-entity `requires` (before 4.3 ships), blob transfer
  (4.4). Two collections merged locally share registers but not media bytes, so `check_media` shows
  the other device's files as "without bytes".
- **No new dependencies.**

## Revisit if

- **Merge speed.** Merging a large batch (a new device, 50,000 notes) is too slow on the phone or the
  web. Then:
  - chunk the batch (it is idempotent);
  - skip the reconcile for notes whose cards all arrived in the same batch;
  - or keep clocks per entity (ADR 0006 already lists this).
- **Lost edits.** People report edits disappearing after a sync, or several people start sharing one
  account and editing the same notes. Then build part 7. If it is built and its stale entries (the
  325 case) mislead, prune it by age or use version vectors (option D) for kept registers only.
- **Rejected changes** occur in practice from our own clients. That is a bug to fix at the source,
  and a reason to look at the check.
- **Purge semantics** turn out to need undo (a trash with a grace period on the server).

## Decisions on review

Anthony, 2026-10-06.

1. **The superseded-values log is postponed.** The same field edited on two devices before they sync
   is rare for one person, and not expected in the MVP. For now the later edit wins and the other text
   is not kept. Part 7 and finding 1 stay as the design to build from. Adding it later is additive: a
   register with no `base` never matches, so older data can at worst log a stale value once.
   - **Shared accounts.** Several people sharing one account follow the same rule: the server keeps the
     latest value of each register, which is the one source of truth, and the later edit wins. If
     shared accounts become common, that is a reason to build part 7 (see "Revisit if"). Shared decks
     between different accounts are Phase 9's spaces (ADR 0006 section 12), not this.
2. **One PR, 1.11a.** With the log postponed, nothing is left for a 1.11b.
3. **Purge and per-entity `requires`** are not built now. Parts 8 and 9 record their shape. Purge
   comes with "Empty trash". Per-entity `requires` must still ship before step 4.3 releases sync.
4. **Rejected values** (part 4): Claude Code's call, as a technical decision. They are dropped and
   reported, not stored.

## Build notes (step 1.11a)

Built to the plan in `docs/plans/1.11-merge.md`, one PR. No new dependency.

- **What exists.**
  - Migration v11: `unknown_row_value`, a local table (in `LOCAL_TABLES`).
  - `sync/changes.rs`: `Changes`, `RegisterChange`, `RowChange`, `Clock`, `Selection` and
    `Collection::changes`.
  - `sync/merge.rs`: `Collection::merge`, `MergeReport`, `Rejected`, and the crate-only
    `WriteTx::apply_register` and `apply_row`.
  - `sync/unknown.rs`: `adopt_unknown`, and one shared rule for storing an unknown register.
  - `note/generate.rs`: `reconcile_notes` (some notes of one note type) and
    `reconcile_note_type_counting`.
  - `fc merge <from> <into>`.
- **How a merge runs**, in one `Collection::write`:
  1. The HLC moves up to the highest received register clock, including registers that lose, before
     anything else is written.
  2. Each register is classified once: a register of a synced table, a value of a dynamic table, or
     unknown. A known one is type-checked against `pragma_table_info` (read once per merge), then
     compared with the local clock. The winner is written with `INSERT ... ON CONFLICT`, so a row that
     does not exist yet is made with that one value and the defaults. Its clock is saved as pushed.
  3. Each append-only row is inserted with `INSERT OR IGNORE`. Extra columns, and every column of an
     unknown entity, go to `unknown_row_value`. Nothing is added to `unpushed_row`.
  4. Cards of notes: every note type with a changed `note_type`, `note_type_field` or `template`
     register is reconciled whole, and every other live note with a changed `note` register or field
     value is reconciled once. A card register triggers nothing, as part 5 says.
  5. Schedule: every card with a new event, and every card with an event that names a parameter set
     that arrived in this batch, is folded again.
- **What is rejected** (part 4, "Claude Code's call"): a NULL for a NOT NULL column, a type that does
  not fit the declared one (an integer fits REAL), NaN (SQLite would store NULL), a row missing a NOT
  NULL column, and a row that gives a column twice. Rejected changes are listed in
  `MergeReport::rejected` with entity, ID, register or column, and the reason. Nothing else in a batch
  can make a merge fail except the database itself (the failure rolls everything back, tested).
- **`MergeReport`.** `registers_applied`, `registers_ignored`, `unknown_registers` (newly kept),
  `unknown_rows` (rows that arrived with columns or an entity type this build keeps as unknown, newly
  kept), `rows_added`, `notes_reconciled` (notes whose cards the merge changed, not notes looked at),
  `cards_rebuilt` and `rejected`.

**Findings during the build:**

1. **A new device would have brought back cards that went to the trash with their deck.** Found by the
   convergence test (seed 12). A deck is deleted on A while B adds a note to it. The deck then reads as
   alive because of that one card (ADR 0006's "still referenced, so still alive"), and the other cards
   stay deleted on the devices that merged it. A new device reconciles every note, and the existing
   dead-deck rule no longer saw the deck as dead, so it restored all of those cards and then would have
   pushed the revival to everyone. The merge now treats any deck whose `deleted` register is set as dead
   when reconciling, whatever still refers to it (`reconcile_after_merge`). A local edit of a note
   still uses the old rule (an edit of one note in such a deck can bring its own card back). Test:
   `a_new_device_does_not_bring_back_cards_that_went_to_the_trash_with_their_deck`.
2. **Two answers to a new card are two steps.** The plan's row "Same card reviewed on two devices ...
   interval about the single-Good one" holds for a review card (a second answer minutes after the first
   is a same-day review, and the interval moves by about a day). For a new card, Good on two devices
   replays as two answers and the card graduates, as ADR 0007 part 6 says ("a concurrent event is
   recomputed from the state the fold has reached"). Both are tested.
3. **The media bytes are not synced** (4.4), so `has_bytes` differs between two converged collections.
   The test digest leaves it out. Every other field of `media_files()` is compared.

**Audit of ADR 0006 section 3 against the registry** (part 10). The registry has these tables and no
others. `.write(|` outside tests was searched, and every public operation that calls it is in the tour.

| ADR 0006 section 3 | Built |
| --- | --- |
| Field `options` (the note type's options) | Not built. Nothing uses it. |
| Card `flag` and a new-card position | Not built. No feature uses them yet. |
| `collection_setting` | Built without `deleted` (ADR 0007). |
| `requirement` | Added in 1.1b (a collection-level `requires`). |
| Dynamic tables `note_field_value`, `note_tag` | Built (1.3, 1.6). |
| Append-only `card_event`, `fsrs_parameter_set` | Built (1.7a). |
| Note type, template, note, note tag, card, deck, options preset, media file, saved search | Built with the registers listed, and these were added by later steps: `relearning_steps`, `fsrs_parameters` and `space_siblings` on the preset, `limits_include_subdecks` on the deck. The preset's "limits" are two registers (`new_per_day`, `reviews_per_day`). |

Registers added after their table was created (`card.deck`, `card.suspended`, `card.buried_until`,
`deck.limits_include_subdecks` and `options_preset.space_siblings`) have no clock on rows older than
their migration, and hold the column default everywhere. (The seeded Default deck and preset got seed
clocks for theirs in the migrations, so those are sent.) `changes` does not send a register with no clock, and
`a_register_with_no_clock_is_not_sent` checks it. The invariant check in the tests (every register has
a clock or holds its default) runs on the tour and on the three collections of the convergence test.

**Tests.** 44 new in `fc-core` (593 in all) and 2 new in `fc-cli`.

- `sync/merge_tests.rs` (40): the helpers (`pair`, `sync`, `settle`, `digest`), changes reading,
  the merge rule (higher, equal, lower, ties by device, a missing row, the HLC, a failed merge),
  rejected values, every row of ADR 0006 section 11 that needs no server, the "restore" of a template,
  a field and a deck after a merge, a version-10 upgrade, and the finding above.
- `sync/tour_tests.rs` (4):
  - **The tour:** every public operation that writes (the list in the plan), then
    `changes(All)` into an empty collection, whose `digest` equals the original, with the invariant
    checks on both.
  - **The convergence test:** three collections with clocks 7 minutes ahead and 3 hours behind, 2,000
    random operations from 46 kinds, a random pairwise merge every 20 operations, then a full mesh. It
    ends with identical digests, then re-applies every batch (nothing applied, nothing rejected), then
    merges into two empty collections in opposite orders (identical). Three more seeds of 700
    operations each run in a second test. Seeds are fixed (`id::seeded`).
  - **The "older app" test:** an old schema (version 7 plus the `unknown_row_value` table) receives
    registers of a card, a deck and a preset that it lacks, a whole table (`saved_search`), `media_file`
    and an event column it lacks. It keeps them, relays them to a full collection (identical digest), and
    after reopening with migrations that call `adopt_unknown` (including adding the event column), every
    value is in its table, nothing is left in the store, and the append-only update trigger is back.
- `fc-cli/tests/merge.rs` (2): a note on each of two files is on both after merging both ways, and the
  second merge applies nothing.

**Measured** (Linux, release, a throwaway test that was not kept, because `fc-core` cannot read the
clock). A collection of 50,000 Basic notes (550,000 registers, 10,000 events on 10,000 cards), made
as a batch because adding notes one by one slows down with each note (the duplicate check of 1.3):

| What | Time |
| --- | --- |
| `changes(All)` | 0.62 s |
| `merge` into an empty collection (a new device) | 3.4 s (11.3 s before the statements were cached) |
| `merge` of a batch touching 1% of notes into a copy | 42 ms |
| `merge` of the whole batch again (nothing new) | 0.74 s |

Most of the time is one read and two writes per register. There is no target (the plan says so).
`notes_reconciled` was 0 in all of these: every card was there already, so the reconcile only reads.
The first number to look at on the phone and in wasm is the new-device merge: ADR 0008 "Revisit if" says
to chunk the batch (it is idempotent) if it is too slow.

**Verified.**

- Linux: `cargo xtask check` passes (it includes the wasm build). The CLI on two scratch files and on a
  copy of the real desktop collection (version 11 on open): a note added to each, merged both ways, both
  notes on both, and a second merge applied nothing.
- Not verified: the phone and the web (nothing calls `merge` there, and the migration to 11 runs the next
  time they open a collection), Windows, merge speed on the phone or in wasm, a batch with media bytes
  (4.4).

**Deviations from the plan:**

1. **`apply_register` and `apply_row` take a `&Targets`** (what this build and database can apply,
   read once per merge: tables present, declared column types) as well as the change. The ADR wrote
   `apply_register(change)`.
2. **`reconcile_note` became `reconcile_notes(w, note_type, notes, dead)`.** It takes the notes of one
   note type together, so the templates of a type are parsed once for a batch of 50,000 notes and not
   once each. `reconcile_note_type` is kept and now calls `reconcile_note_type_counting(w, type, dead)`.
3. **The merge reconciles with a stricter set of dead decks** (finding 1). This is a change to the text
   of part 5, which said "the existing dead-deck rule covers the case".
4. **`adopt_unknown(tx, entity, table, names)`** has a `table` argument (a migration works with frozen
   table names) and handles both registers and event columns. For columns it drops the append-only
   update trigger for the statements and puts it back. It is `pub(crate)`, used only by tests (no
   migration needs it yet), and carries a `dead_code` allowance outside tests.
5. **`WriteTx::hlc` is `pub(super)`** and `register_clock` reads use a cached statement
   (`prepare_cached`), which cut the new-device merge to a third.
6. **`study` exports `ParameterSets` and `rebuild_card` inside the crate** (`pub(crate)`).
7. **`MergeReport::unknown_rows` also counts rows with an unknown column**, and a row that is rejected
   for a duplicate column is a rejection, not a failure (the plan listed three kinds of rejection; NaN
   and a duplicated column are two more).
8. **Not done: the optional switch of the hand-written remote helpers** in `deck/tests.rs`,
   `study/queue_tests.rs` and `study/fold_tests.rs`. They still write raw SQL. The plan said to do it
   only if small.
9. **The tests that assert the storage version** were changed from 10 to 11 (`deck`, `note`, `tag`,
   `study`, `queue`, `search` and `media` tests, and the CLI `collection` test).
10. **The "older app" test leaves out note registers** when it feeds the old schema. The code under
    test reads the newest deck columns while reconciling, which an old schema cannot answer. A real
    older build would read its own columns. What the old side keeps as unknown does not depend on notes.
11. **The plan's two-device "same card reviewed" row** is tested for a review card, and the new-card
    case is a second test (finding 2).
12. **Not done:** `fc superseded` (part 7 is postponed), purge, per-entity `requires`, and any `fc-api`
    or web method, as the plan said.
