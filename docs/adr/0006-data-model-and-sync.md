# 0006: Data model and sync strategy

Status: Accepted
Date: 2026-10-05
Accepted: 2026-10-05. Anthony delegated the open product decisions to Claude Code; they are recorded under "Decisions on review".

## Context

Phase 1 builds storage. Phase 4 builds sync. If the storage from Phase 1 cannot record what sync
needs, Phase 4 becomes a storage rewrite. This ADR decides the data model's sync-relevant shape and
the sync strategy now, against the Sync requirements in `PRODUCT.md`:

1. Every device works fully offline.
2. Changes from several devices merge automatically. No "choose a side" prompts in normal use.
3. Review history is never lost in a merge.
4. Devices on different app versions must not corrupt each other's data.
5. Sync runs in the background with a clear status indicator.
6. When something goes wrong, the error says exactly what and what to do.
7. Media syncs efficiently and never re-uploads unchanged files.
8. The server is self-hostable, and Anthony hosts the main instance at zero cost.

Also from `PRODUCT.md`: the app is usable without an account, and signing in later attaches the
local collection (step 4.2), so the first sync of a device must merge, not overwrite (step 4.3).
Export must be open and documented. Undo of the last answer must work. Review debt features (later)
must not be blocked.

The step brief asks for explicit handling of: offline for weeks, the same card reviewed on two
devices, edit versus delete, clock differences, app version differences, schema changes and media.
It also asks how shared decks (Phase 9) and add-on data (Phase 10) fit later.

Constraints from accepted ADRs:

- SQLite through `rusqlite` on every target. On the web it uses the OPFS SAH pool VFS (ADR 0003).
- `fc-core` must not read the clock, sleep or start threads. Time comes from the host, and this ADR
  says how (ADR 0003).
- Randomness for IDs uses `getrandom` with its `wasm_js` backend on the web. This ADR picks the ID
  scheme and checks it (ADR 0003).
- API types contain no `i64` or `u64`. This ADR says how IDs are represented (ADR 0002).
- Scheduling is FSRS through the `fsrs` crate, behind our own module (ADR 0004).
- Cached core state must be rebuildable from the database (ADR 0003, cancel on the web).

### Findings

**Experiments.** These were throwaway, run in a scratch crate outside the repo on Linux. They used the
repo's toolchain (1.98.1), `rusqlite` 0.40.2 (bundled SQLite), `fsrs` 6.6.2 and `uuid` 1.27.0 (all
already in our lockfile), in a release build.

1. **Field-level last-writer-wins in SQLite converges.** The register table was
   `(entity, field) -> (value, hlc, actor)`, and each change was applied with one upsert:
   `ON CONFLICT DO UPDATE ... WHERE (excluded.hlc, excluded.actor) > (reg.hlc, reg.actor)`.
   - The test applied 1,000,000 changes from 3 devices to 50,000 entities × 5 fields. Many changes
     deliberately had equal timestamps from different devices.
   - The changes were applied in three different orders, in 10 batches each like separate syncs.
     All three orders gave the same final state (same digest of all 245,323 registers).
   - Each order took 1.0 to 1.1 s.
   - Re-applying half the changes afterwards (an interrupted sync retried) changed nothing.
2. **Append-only events merge by ID.** 1,000,000 inserts, 400,000 of them duplicate IDs, gave
   600,000 rows with `INSERT OR IGNORE`, in 476 ms.
3. **FSRS replay of one card reviewed on two devices.** This used default parameters, retention 0.9,
   and a card at S = 10.96 days, due on day 11.
   - Device A reviews Good. On its own that gives S = 46.28 and interval 46.3 days.
   - Device B reviews Good from the same old state, 5 minutes later. The merge replays B on top of
     A with 0 elapsed days.
   - Result: S = 46.28, D = 2.097, interval 46.3 days. A second Good on the same day barely moves
     the schedule, which is what a user would expect.
   - If B is Again instead: S = 12.77, interval 12.8 days. A lapse still counts after the merge.
   - Replaying twice from scratch gave bit-identical `f32` values.
   - `fsrs`'s `memory_state(history)` gave the same S and D as step-by-step replay. So the memory
     state can be derived from the review history alone.
   - Replaying 50,000 cards × 20 reviews took 219 ms.
4. **IDs.**
   - `uuid::Builder::from_unix_timestamp_millis(ms, &random10)` makes a version 7 UUID from a time
     and random bytes we pass in, so `uuid` never reads the clock or the RNG itself.
   - `Uuid::new_v5(note, template ‖ ord)` gives the same card ID every time for the same inputs, and
     a different one for a different ordinal.
   - Built for `wasm32-unknown-unknown` with `getrandom` 0.4.3 (`wasm_js`), bound with
     `wasm-bindgen` 0.2.129 and run under Bun 1.4.2: random v7 IDs and v5 card IDs were produced.
     This answers ADR 0003's open point for Bun's runtime, but not yet in a browser.

**Library status.** Checked on GitHub, crates.io and vendor pages on 2026-10-05.

5. **cr-sqlite** (CRDTs inside SQLite) is MIT. Its last release is v0.16.3 from January 2024, with
   no release since.
6. **SQLite Sync** (sqlite.ai) is under the Elastic License 2.0. That is not open source, and the
   product is built around their SQLite Cloud service.
7. **Automerge** (MIT, Rust crate 0.12.0, Sep 2026) and **Loro** (MIT, 1.16.4, Sep 2026) are active
   document CRDT libraries. A third-party comparison gives about 180 KB (Loro) and 320 KB (Automerge)
   of gzipped wasm. Not checked by building.
8. **PowerSync.** The self-hosted service is under the Functional Source License (source-available)
   and needs Postgres, MongoDB, MySQL or SQL Server behind it. Client SDKs are Apache-2.0. Writes go
   through the app's own backend, so the conflict rules would still be ours to write.

**Not verified:**
- Any of the experiments in a browser, on Android or on Windows.
- `getrandom` in a real browser (only under Bun, which uses the same `crypto.getRandomValues`).
- Bit-identical FSRS results across platforms. ADR 0004 showed the same intervals to the digits
  displayed on Linux, wasm and the phone. See the "Derived scheduling state" section for why
  bit-identity is not required.
- The server-side behaviour described below (no server exists yet). Step 4.5's tests cover it.
- Wire size and speed of a first sync of a large collection.

## Options considered

### A. Whole-object last-modified sync with a full-sync fallback

Each row has a modified time and a "needs sync" marker. The server keeps the newest version of each
object. When the schema changes or the two sides cannot be reconciled, one side's whole collection
replaces the other's.

- Bad: whole-row last-writer-wins loses concurrent edits to different parts of the same object. For
  example, a field edited on the phone and a tag added on the desktop: one is lost.
- Bad: the full-sync fallback is exactly the "choose a side" prompt `PRODUCT.md` rules out.
- Bad: modified times come from device clocks, so a wrong clock decides who wins.

### B. Field-level registers with hybrid logical clocks, plus append-only card events (chosen)

All mutable data is stored as ordinary tables. Alongside them, each sync-relevant field (a
"register") records the hybrid logical clock (HLC) timestamp and the device of its last write.

- **Merging registers.** The higher `(hlc, device)` wins. This is commutative, idempotent and
  order-independent (finding 1).
- **Reviews and manual scheduling actions** are immutable events with their own IDs, merged by set
  union (finding 2).
- **A card's scheduling state** (due, stability, difficulty, learning step) is a local cache
  derived from its events, as planning suggested (finding 3).
- **Deletes** are soft, so nothing a merge touches is ever gone.
- **The server** stores and relays changes without knowing the app's schema.

- Good:
  - No merge ever needs input, and none can fail.
  - The same merge runs on two local collections, so step 1.11 can test it with no server.
  - Concurrent edits to different fields both survive. Reviews are never lost.
  - Clock skew is bounded by the HLC and checked against the server.
  - Old app versions store and relay data they do not understand.
- Bad:
  - Two edits to the *same* field still pick one winner. This ADR keeps the losing text locally so
    it can be recovered, without a prompt.
  - Invariants that span rows (unique deck names, no cycles in the deck tree, cards in deleted decks)
    cannot be enforced by the database. They are resolved by deterministic rules when data is read.
  - Every write must go through one code path that also records the clock. That is a Phase 1
    discipline, and a test enforces it.

### C. Server-reconciled mutations (Replicache or Linear style)

Clients send named mutations ("answer card", "rename deck"). The server runs the core's logic on them
in arrival order and is authoritative. Clients undo their pending mutations and replay them on top of
the server's state.

- Good: arbitrary invariants can be enforced centrally, and the server knows the intent of each
  change.
- Bad: a phone offline for weeks has its old mutations applied after newer edits from other devices,
  unless mutations carry timestamps. Once they do, this is option B plus a server that must run the
  core.
- Bad: the server must run every old client's mutations with the semantics of that client's version,
  forever. Self-hosters must upgrade the server in lockstep with the apps.
- Bad: two local collections cannot merge without a server, so the first-sync merge needs the
  server too, and step 1.11 cannot test merges locally.

### D. Full event sourcing

Every user action is an immutable operation in a log, and all state is a replay of the log.

- Good: complete history, and merging is a union of logs.
- Bad: the log grows with every edit. A new device replays years of operations, so snapshots are
  needed, which turns it back into option B for everything except the parts where history matters.
- Bad: every operation's meaning must be kept forever, so migrations cannot rewrite the past.
- Option B takes event sourcing only where it pays: card events, where the history *is* the data.

### E. Off-the-shelf sync engines and CRDT libraries

- **Automerge, Loro** (finding 7):
  - Each collection would be a CRDT document held in memory, with millions of review entries.
  - Search needs SQL (ADR 0003), so everything would be stored twice, in the document and mirrored
    into SQLite.
  - The document format becomes our storage and export format.
  - The domain rules (deterministic card IDs, deck tree cycles, scheduling replay) would still be
    ours to write.
  - Real-time collaborative text editing, their main strength, is a non-goal.
- **cr-sqlite** (finding 5): close to option B's design, but no release in over two years, and
  untested with `sqlite-wasm-rs`. Writing our own version for a known schema is small (finding 1).
- **SQLite Sync** (finding 6): not open source, and built around a paid service.
- **PowerSync** (finding 8): needs a database server and a source-available service to self-host.
  Its write path is our own backend anyway.

### Sub-choices within B

- **IDs.**
  - **128-bit UUIDv7 (chosen):** time-ordered, so new rows append to SQLite's B-trees. Random enough
    that IDs from different users never collide, which shared decks need.
  - 64-bit random integers: smaller, but collisions become plausible once shared decks mix IDs from
    many users. The API needs strings either way (ADR 0002).
  - Server-assigned IDs: ruled out by offline work.
- **Ordering.**
  - **Hybrid logical clock (chosen):** wall time plus a counter, never behind anything the device
    has seen.
  - Wall time alone: a slow clock loses to older edits.
  - Server arrival order: a phone offline for weeks would overwrite newer edits made elsewhere
    when it finally syncs.
- **Register granularity:** one register per field of an entity. Values that only make sense together
  are one register (for example the 21 FSRS parameters).
- **Server role.**
  - **A schema-agnostic relay that keeps only the latest value per register plus all events
    (chosen).** It is simple to self-host and does not need upgrading for most app changes.
  - A server that runs the core: possible later (it is Rust), but not needed for merging.

## Decision

Option B. The rules below are what Phase 1 builds in and Phase 4 relies on.

### 1. Identity

- **Random IDs.** Every synced entity has a 128-bit ID: a UUIDv7 made from the host's time and 10
  random bytes from `getrandom` (finding 4).
  - Stored as a 16-byte `BLOB` in SQLite.
  - In API types, a canonical lowercase UUID string (ADR 0002 forbids 64-bit numbers).
- **Deterministic IDs** where two devices could create "the same" thing independently:
  - **Cards:** `UUIDv5(namespace = note ID, name = template ID ‖ ordinal)`. The ordinal is the cloze
    number, or 0 for non-cloze templates.
    - Two devices that generate cards for the same note never create duplicates.
    - Re-filling a field that brings back a deleted card restores the same card, with its history.
  - **Built-in content:** fixed IDs written into the code for the built-in note types (Basic, Basic
    and reversed, Cloze) and their fields and templates, the Default deck and the default options
    preset. Two collections created separately (first-sync merge) share them instead of having two
    of each.
- **Field and template IDs.** Note type fields and templates have their own IDs. Note field values
  are keyed by field ID, never by position or name. So renaming or reordering a field on one device
  while notes are edited on another is safe.
- **Device ID.** Each copy of a collection has a random 128-bit device ID (an "actor"). It is stored
  in the collection, together with the app installation ID, which is stored outside the collection.
  - If a collection is opened by a different installation (a copied file), the device ID is
    regenerated.
  - Restoring a backup or importing also regenerates it.
  - Two copies must never write with the same device ID. They could produce two different values
    with identical `(hlc, device)`, and the copies would never converge.

### 2. Time

- **The host supplies time.** `fc-core` takes a `Clock` from the host when a collection is opened.
  It provides the current Unix time in milliseconds and the local UTC offset.
  - `fc-native` uses the system clock.
  - `fc-wasm` uses `Date.now()` and `getTimezoneOffset()`.
  - Tests use a fake clock.
  - This settles ADR 0003's open point.
- **The hybrid logical clock (HLC)** is 64 bits: 48 bits of Unix milliseconds and a 16-bit counter.
  The device ID breaks ties.
  - On a local write: `hlc = max(now, last + 1)`.
  - On receiving a change: `last = max(last, received)`.
  - `last` is saved in the collection, so the HLC never runs backwards across restarts or when the
    user changes the clock.
  - A device that has seen a change always writes after it. This is what lets the server and other
    devices order changes correctly.

### 3. Two kinds of synced data

**Registers** (mutable state, field-level last-writer-wins). Every synced entity type lists its
registers:

| Entity | Registers (indicative; Phase 1 steps finalise) |
| --- | --- |
| Note type | name, kind (standard or cloze), CSS, sort field, `deleted` |
| Note type field | note type, name, position, options, `deleted` |
| Template | note type, name, position, front, back, `deleted` |
| Note | note type, one register per field value (by field ID), `deleted` |
| Note tag | (note, tag) present or absent |
| Card | note, template, ordinal, deck, suspended, buried until, flag, new-card position, `deleted` |
| Deck | name, parent, options preset, `deleted` |
| Options preset | name, limits, learning steps, desired retention, FSRS parameters (one register), `deleted` |
| Media file | content hash, size, `deleted` |
| Saved search, collection setting | value, `deleted` |

- **One write path.** Each write records `(hlc, device)` for every register it changes, in the same
  SQLite transaction, and marks it as not yet pushed.
- **How a merge applies a remote register:**
  - It takes the higher `(hlc, device)`, as in finding 1.
  - It advances the HLC.
  - It updates the domain table.
  - It recomputes any affected cache.
- **The losing value is kept, without a prompt.** If a register wins over a local value that was
  never pushed, the losing value goes into a local "superseded values" log. A later UI can offer
  "this field was also edited on another device: show the other version". This covers note fields,
  templates and names.

**Card events** (append-only, merged by union). Reviews and manual scheduling actions are immutable
records with a UUIDv7 ID:
- review (rating, duration);
- set due date;
- reset to new;
- reschedule;
- void (undo).

Each event records:
- the card;
- the event's time and the device's UTC offset at that moment;
- the inputs that produced its result: the options preset ID, desired retention and the FSRS
  parameters in effect (a hash or an inline copy, 1.7 decides);
- the result: memory state, scheduling state, learning step, due time;
- the ID of the previous event it built on (none for the first).

The fuzz for the interval is seeded from the event's own ID, so the result can be recomputed exactly.

### 4. Derived scheduling state

A card's due date, memory state, queue and learning step are a local cache. They are never synced.
They are computed from the card's events:

1. Sort the events that are not voided by (time, ID).
2. Fold over them. If an event's "previous event" is the one just applied, use its recorded result.
   This is the normal case, and it gives the same answer on every device and app version. Otherwise
   two devices reviewed concurrently, so recompute the event from the current state with the event's
   own rating, time and recorded parameters (finding 3).

This is a pure function of the event set, so every device that has the same events computes the same
cache. Recomputation happens only on concurrent branches, where tiny float differences between
platforms could at worst move a due time slightly on one device. That does not spread, because the
cache is not synced.

### 5. Deletes

- **Soft deletes.** Deleting sets the `deleted` register. The content stays, so deleted items are
  in a trash and can be restored, which sets `deleted` back with a newer HLC.
- **Permanent deletion ("Empty trash")** writes a `purged` state that no later change can undo, and
  clears the content.
  - It is the only destructive operation, and it is explicit.
  - Card events are never purged by it. They are kept for statistics and the optimiser.
  - Account deletion is the exception (step 4.8).
- **Deleting a container writes explicit tombstones.** Deleting a deck or note type also soft-deletes
  each card or note it contained on that device at that moment.
- **Rule: anything still referenced by live data stays alive.** A deck or note type that is deleted
  but still has a live card or note (added concurrently on another device) is treated as live. It
  shows up again with only those items. This is decided at read time, so nothing is written and
  nothing can loop.

### 6. Invariants the database does not enforce

Merges must never fail. So on synced tables there are:

- no `UNIQUE` constraints on user-visible names;
- no enforced foreign keys (a card can arrive before its note);
- no shared counters (a "next position" counter cannot merge, so positions come from IDs or HLCs).

Deterministic read-time rules cover what is left:

- **Duplicate deck names** after a merge are shown as two decks with the same name, and the UI
  disambiguates them (for example "Polish" and "Polish (2)", by ID order). A "merge decks" action
  (Phase 3) combines them on request. No prompt.
- **Deck tree cycles** (A moved under B on one device, B under A on another) are broken by ignoring
  the newest parent change in the cycle (highest HLC). That deck shows at the top level.
- **References to missing rows** (a card whose note has not arrived yet) are hidden until it
  arrives.
- **Ordering ties** in positions are broken by ID.

### 7. Sync protocol (shape; step 4.3 designs the wire format)

- **The server keeps, per account,**
  - one row per register: its latest `(value, hlc, device)` and a server sequence number, assigned
    each time a newer value wins;
  - all events, each with a sequence number;
  - media blobs by hash.
- **Changes are self-describing.** Each change carries the entity type and field name as strings, so
  the server stores and relays entity types it does not know.
- **A sync** pulls everything with a sequence number above the device's cursor, applies it, pushes
  the device's unpushed registers and events, then advances the cursor.
  - Changes are idempotent (finding 1), so an interrupted sync is simply retried.
  - A losing push changes nothing on the server, and the device learns the winner on its pull.
- **No compaction step is needed.** The server only keeps the latest value per register, so a device
  offline for any length of time pulls only what is newest. Events are never compacted. A new device
  bootstraps with a pull from cursor 0.
- **The first sync of a collection that existed before signing in** is an ordinary sync with every
  register unpushed. Two separately created collections merge. Built-in content shares IDs, and
  same-named user decks follow rule 6.
- **The wire format includes a "space" ID from the start** (always the user's own collection until
  Phase 9). See "Shared decks".
- **The server does no merging logic beyond the comparison.** It validates sizes, timestamps
  (section 9) and version requirements (section 10), and nothing else.

### 8. Media

- **Content-addressed.** A file's identity is the SHA-256 of its bytes.
  - The file name used in note fields includes enough of the hash to be unique, so a name always
    means the same bytes. Two devices adding different files called `image.jpg` produce two
    different names.
  - Anki imports (Phase 5) rename on import and rewrite references in the imported fields and
    templates. Step 1.10 picks the exact name format.
- **Synced like other data.** Each file is a "media file" entity (registers: hash, size, `deleted`).
  The bytes are a blob stored once per hash.
- **Upload.**
  - The device asks the server which of its hashes it lacks, and uploads only those. Unchanged files
    are never re-uploaded.
  - Blobs go up before the registers that reference them. The server rejects a media register whose
    blob it does not have, so no device ever sees a reference it cannot download.
- **Download** is lazy or in the background (step 4.4), so study is never blocked.
- **Deleting unused media** sets `deleted`. If, after a merge, a live note still references the file,
  the reference wins and the file stays (rule 5).
  - The server deletes a blob only once it has been marked deleted for a grace period (proposed: 30
    days) and nothing references it.
  - A device that later needs a deleted blob and still has the file locally re-uploads it.
- **Deduplication is per account only.** It is never across accounts, so the server cannot reveal
  whether another user has a file.

### 9. Clock differences between devices

- **Ordering does not trust wall clocks alone.** The HLC never goes behind anything a device has
  seen, so a device with a slow clock still orders its edits after what it synced.
- **Skew check.** Every sync response carries the server's time. The device estimates its offset
  (allowing for half the round trip).
  - If the offset is over 10 minutes, the app warns: "This device's clock is 3 hours fast. Fix the
    date and time in your device settings."
  - The device then corrects its unpushed registers and events by that offset before pushing. That
    covers both the HLC and the event times used for scheduling, and the device recomputes its
    caches. Pushed data is never rewritten.
- **The server rejects changes with an HLC more than 10 minutes ahead of its own time**, after
  correction, with an error that names the problem. This stops a badly wrong clock from making its
  edits win for days.
- **Without an account** there is no reference time. A wrong clock then affects only that device's
  own scheduling, as in any offline app.

### 10. App versions and schema changes

- **The local SQLite schema and the sync format are separate.**
  - Each device migrates its own database on upgrade (step 1.1).
  - A local migration never needs a full sync, a re-upload or a "choose a side".
  - A collection from a newer app version refuses to open with a clear error (step 1.1).
- **The sync format only changes additively.** New entity types and new registers get new names.
  Old names keep their meaning. Merge rules for an existing register never change.
- **Unknown data is kept, not dropped.** An app that receives an entity type or register it does not
  know stores it unchanged in a generic local table. It never writes to it, so it cannot overwrite
  it. After an upgrade, a migration applies what was stored.
- **Required features.** When an older app would misinterpret data, the data says so:
  - **Per entity:** an entity can carry a `requires` register, a set of feature names. An app that
    does not know one of them shows that entity read-only: "Update the app to edit this note."
  - **Per collection:** a collection-level `requires` set covers changes to sync itself. An app that
    does not know a feature in it pauses sync: "This collection uses features from a newer app
    version. Update the app on this device to keep syncing. Changes made here are kept and will sync
    after the update." Local study continues.
  - The server checks both on push and returns the same message, so an old app cannot write past
    them.
- **Protocol version.** The client sends its protocol version and features. The server accepts a
  range and rejects others: "This app is too old for this sync server. Update to version X."
- **Derived state is safe across versions.** The scheduling cache is recomputed locally by each
  version, and events record their results (section 4). So two versions agree on every card that was
  not reviewed concurrently.
- **Note type changes by the user** (fields added, removed, renamed or reordered, templates changed)
  are register changes like any other. A removed field's values stay stored and hidden, and come
  back if the field is restored.

### 11. Edge cases

What happens in each case. No case prompts the user.

| Case | Result |
| --- | --- |
| A device offline for weeks | Everything it did is pushed and merged when it reconnects. It pulls only the latest value of each register changed meanwhile. Nothing expires on the server, except deleted media blobs after the grace period, which it re-uploads if it still has them. Its HLC is behind other devices' edits made later, so stale edits do not overwrite newer ones. Daily limits may be exceeded across devices for those days. |
| The same card reviewed on two devices | Both reviews are kept. The schedule is replayed in time order (section 4). Good then Good on the same day keeps about the same interval. Good then Again on the same day counts as a lapse. Both count in statistics and in the optimiser. |
| Undo of an answer that has already synced | A void event. Other devices drop that review from the replay on their next sync. The record stays, marked voided. |
| Same field of a note edited on two devices | The later (HLC) value wins everywhere. The other value is kept in the losing device's superseded-values log. |
| Different fields or tags of one note edited on two devices | Both kept. |
| Edit on one device, delete on another | The note is deleted (in the trash), with the edit merged into it. Restoring it shows the edit. |
| Review on one device, delete on another | The card is deleted (in the trash). The review is kept, and restoring the card restores its full history. |
| Note added to a deck, or card moved into it, while another device deletes that deck | The deck stays alive because a live card references it (rule 5). It shows up with only that card. Cards it had on the deleting device stay deleted. |
| Note type deleted while notes of that type are added elsewhere | The note type stays alive for those notes (rule 5). |
| Field removed from a note type while a note's value for it is edited elsewhere | The value is kept and hidden. It comes back if the field is restored. |
| Template removed while one of its cards is reviewed elsewhere | The card is deleted with the template, and the review is kept. Re-adding the template restores the same card ID, with history. |
| Suspend on one device, review on another | Both apply. The card is suspended and the review counts. |
| Deck options changed on two devices | Field by field. Optimised FSRS parameters are one register, so the later optimisation wins whole. |
| Two devices move decks into each other | The cycle is broken at read time (rule 6). |
| Two collections created separately, then signed in to one account | They merge. Built-ins are shared. Same-named decks show twice until merged by the user. |
| A device clock wrong by hours or days | Warning, and unpushed changes corrected (section 9). The server refuses anything still far in the future. |
| Two app versions syncing | Unknown data kept and relayed. Entities or collections needing a newer version are read-only, or sync pauses with an update message (section 10). |
| A local schema migration while other devices are on an old version | Local only. The sync format is unchanged unless a feature is added. |
| An interrupted sync | Retried. Changes are idempotent and the cursor advances only after a local commit. |
| A corrupted local database | Rebuilt from the server with a pull from cursor 0. Any unpushed changes that can still be read are re-applied. Otherwise the error says which recent changes were lost. |
| A backup restored on a synced device | The restore is written as ordinary new changes (the difference to the current state), with a new device ID, so it syncs like edits. It does not get silently overwritten by the server, and it does not overwrite other devices. Card events are unioned, not rolled back. |

### 12. Shared decks (Phase 9) and add-on data (Phase 10)

**Shared decks.** A shared deck becomes its own **space**: a separate change log on the server with
members and permissions, holding the deck's note types, notes and media.

- A subscriber's cards, events, deck placement, suspensions and personal tags stay in the
  subscriber's own space. They reference the shared notes by their global IDs.
- Versions are points in the shared space's sequence, which subscribers pull or follow.
- A subscriber's local edit to a shared note is a personal override register, keyed by shared note
  and field.
- What this asks of Phase 1:
  - global random IDs;
  - per-user state on cards and events, never on notes or note types;
  - the space ID in the wire format (Phase 4).

**Add-on data.** It is stored as registers and events of namespaced entity types
(`addon.<add-on id>.<type>`).

- The generic unknown-data store from section 10 already holds and relays them for devices without
  the add-on.
- Add-ons write through their own limited API (ADR 0002, Revisit), never directly to core tables.
- The server applies a size quota per account.

### 13. What Phase 1 must build from day one

Without these, Phase 4 would need a storage rewrite:

1. 128-bit IDs as above, with deterministic card and built-in IDs, and field-ID-keyed note values.
2. The host-supplied `Clock`, the HLC, and the device ID with regeneration on copy, restore and
   import.
3. A single write path that records `(hlc, device, pushed?)` per register in the same transaction,
   plus a test that fails if a synced table can be written without it.
4. Soft deletes with explicit container tombstones, and permanent deletion only through an explicit
   purge.
5. Card events as the source of truth for scheduling. The scheduling columns on cards are a
   rebuildable cache. Events record their inputs, result, previous event and fuzz seed. Undo is a
   void event.
6. No unique name constraints, enforced foreign keys or shared counters on synced tables, and
   read-time rules for the invariants.
7. The generic store for unknown entity types and registers, and `requires` on entities and the
   collection.
8. Content-addressed media with hash-derived names.
9. A merge function in `fc-core` that applies a batch of remote registers and events. It is used by
   step 1.11's two-collection tests and later by sync.
10. Restore and import that produce ordinary changes.

Not needed until Phase 4: the server, the wire encoding, the cursor, space IDs, media upload, the
skew check.

## Consequences

- **No merge prompt exists anywhere in the design.** The only user-visible traces of concurrency are
  the following, and none blocks anything:
  - a superseded value that can be viewed;
  - a deck or note type that reappears;
  - two decks with the same name;
  - a clock warning;
  - an "update the app" message.
- **Review history is append-only.** Its only removals are a user's own undo (kept, marked voided)
  and account deletion.
- **Storage cost.** One clock row per register (about 1 to 1.5 million for 50,000 notes with their
  cards and fields), plus about 100 bytes per review event. Applying a million changes takes about
  1 s natively (finding 1). Not measured on the phone or the web.
- **Phase 1 is somewhat slower to build.** Every write goes through the tracked path, and every
  query must exclude deleted rows and follow the read-time rules.
- **The scheduling cache depends on code.** A future change to how events are folded must be
  versioned, like the merge rules.
- **Concurrent edits to the same text field lose one version from view.** It is kept on the device
  that lost and can be recovered, but there is no character-level merge. Real-time collaborative
  editing is a non-goal.
- **The server is small.** Per account, it is a register table, an event table and a blob directory,
  and it can use SQLite. It suits a free host or Anthony's machine, and it rarely needs upgrading
  with the app.
- **No new dependencies yet.** `uuid` (already in the lockfile through Tauri) becomes a direct
  dependency of `fc-core` in step 1.1, with only the `v5` feature (its v7 builder needs no feature).
  SHA-256 comes from `sha2` (already in the lockfile) in step 1.10. Each is stated in its PR.
- **Doc consequence: change tracking is not only step 1.11.** Items 1 to 8 of section 13 must exist
  from step 1.1 onwards, or each step from 1.2 to 1.10 writes untracked data that 1.11 would have to
  retrofit. So `phases/01-core.md` now has 1.1a (storage and migrations) and 1.1b (sync
  foundation), and 1.11 keeps the merge function, the two-collection tests and the details.

## Decisions on review

Anthony asked Claude Code to make these four calls. Each is the option the Proposed ADR recommended.

1. **Edit on one device, delete on another: the delete wins.** The edit is merged into the deleted
   item, so restoring it from the trash shows the edit. Reason: the delete was an explicit intent,
   and nothing is lost because the content stays in the trash. "Edit wins" would bring back items
   the user removed, with no way for them to know why.
2. **The same field edited on two devices: the later edit wins, with no prompt.** The losing text is
   kept in the losing device's superseded-values log so a later UI can offer it back. Reason: a
   prompt is what `PRODUCT.md` rules out, and a character-level merge is real-time collaborative
   editing, a non-goal. Revisit if this loses work in practice (see below).
3. **Same-named decks after a first-sync merge stay separate** until the user merges them with the
   "merge decks" action (Phase 3). Reason: merging decks automatically by name would move cards and
   mix option presets without the user asking, and two decks with the same name is visible and
   easy to fix. The UI disambiguates them.
4. **The sync foundation moves into Phase 1's first steps.** Step 1.1 is split into 1.1a (collection
   storage and migrations, as before) and 1.1b (sync foundation: IDs, clock, HLC, device ID, register
   clocks and the write path, the unknown-data store, `requires`). Reason: one PR per coherent change,
   and every table from 1.2 onwards is then tracked from the start. Step 1.11 keeps the merge
   function, the two-collection tests and the remaining details.

## Revisit if

- **Field-level last-writer-wins loses work in practice.** For example, the same note is often edited
  on two devices offline. Then show the superseded values more prominently, or add a text merge for
  note fields only.
- **Clock storage or apply speed is a problem** on the phone or the web at 50,000+ notes. Then store
  clocks per entity with a changed-fields mask instead of per register.
- **Shared decks (Phase 9) need semantics that last-writer-wins cannot give**, such as review and
  approval of edits. That is a server-side feature of the shared space, not a change to personal
  sync.
- **A maintained CRDT-for-SQLite library appears that supports wasm, and is open source and
  self-hostable.**
- **The FSRS fold gives visibly different due dates across devices** for the same card. Then sync
  the cache as a register written only by the fold.
- **The server needs to enforce semantic rules.** Then run `fc-core` on the server (option C's
  strength) for those rules only.

## Build notes (step 1.1b)

The sync foundation (section 13, items 1 to 3 and 7), built to this ADR with no change to the
decision. These are the choices step 1.1b left open.

- **IDs.** `fc_core::id::Id` is 16 bytes, a lowercase hyphenated UUID string in text, a `BLOB` in
  SQLite. `Id::new_v7(ms, random)` takes both inputs, `Id::generate(ms)` adds `getrandom`.
  `Id::new_v5` is there for deterministic IDs (cards from 1.3, requirements here). Only canonical
  lowercase text parses. `uuid` is pinned `=1.27.0` with the `v5` feature, which pulls in
  `sha1_smol` (a new, small, pure-Rust package in the lockfile).
- **Clock.** `Clock::now()` gives Unix ms and the UTC offset in minutes. A `Host` bundles it with the
  installation ID and is passed to `Collection::create` and `open`, so there is one clock per open
  collection. `fc-core` cannot read the system clock: `clippy.toml` bans the types and calls, and
  `cargo xtask check` runs clippy. `ManualClock` is the fake for tests. `fc-native` and `fc-cli`
  use `chrono` (`=0.4.45`, `clock` feature only, already in the lockfile through Tauri) for the
  local UTC offset. `fc-wasm` uses `Date.now()` and `getTimezoneOffset()`.
- **HLC.** A `u64`: 48 bits of ms, 16 bits of counter. `next(now) = max(now << 16, last + 1)`, so a
  full counter carries into the milliseconds. Stored as SQLite's signed integer (ms are limited to
  47 bits, year 6429). Saved in `meta` (`hlc_last`) in the same transaction as the write, and moved
  up by `observe_hlc` or by storing an unknown register. Each register write gets its own stamp.
- **Device and installation IDs.** Both are in `meta`. On every open, a missing device ID or an
  installation ID that differs from the host's gives a new device ID (`identify`). Where the host
  keeps the installation ID: `fc-native` in `app_config_dir/installation-id` (so copying the data
  directory does not carry it), `fc-cli` in `<collection>.installation`, the web worker in
  IndexedDB (a temporary ID if IndexedDB is unavailable, which only costs a new device ID per
  load). `regenerate_device_id()` is for restore and import (1.13). A copied file opened by the
  same installation keeps its device ID: this is why restore must call it explicitly. Old writes
  keep the old device ID, history is not rewritten.
- **Registers and the write path.**
  - A synced table is a `SyncedTable` (entity type, table, register names). The register name is
    the column name, and every table has `id BLOB PRIMARY KEY`. The list is `SYNCED_TABLES`.
  - `register_clock` is keyed by `(entity_type, entity_id, field)` with `hlc`, `device` and
    `pushed`. Entity and field are strings, as the sync format needs.
  - `Collection::write(|w| ...)` is the only way to change a synced table. `WriteTx` has `insert`
    (every register must be given, so every register has a clock), `set`, `get` and `new_id`, and no
    raw SQL. Failure rolls back the value, the clock and the saved HLC together.
  - **Guard triggers.** `install_guard` (called in the migration that creates a table) makes SQLite
    abort an INSERT or UPDATE unless a `write_guard` row exists, which only `write` creates, inside
    its transaction. A DELETE always aborts (hard delete is for the purge in 1.11). So even a
    forgotten `conn.execute` cannot write a synced table without going through `write`.
  - **Tests.** `check_schema` fails if a table is neither in `LOCAL_TABLES` nor a `SyncedTable`, if
    a synced table has a column that is not `id` or a register, or if a guard trigger is missing. It
    runs on the real schema, and its own failures are tested.
  - **For 1.11:** the merge applies remote registers with the remote `(hlc, device)`, so it needs
    its own write method that opens the guard. A migration that must rewrite synced rows needs the
    same. Neither exists yet.
- **Unknown data.** `unknown_register` has an untyped `value` column, so any SQLite value is kept as
  received. `store_unknown_register` keeps the higher `(hlc, device)` (tested: four orders and
  repeats give the same result) and refuses a register this build knows. `knows_register` is the
  classifier. Unknown events and entity-level data wait for 1.7 and 1.11.
- **`requires`.** One `requirement` row per feature, ID `UUIDv5("fc-requirement-1", feature)`, with
  registers `feature` and `active`, both defaulted so a row that arrives half filled is harmless.
  `require_feature` writes through `write` and `info()` returns `unsupported_features`. **A
  collection that needs an unknown feature still opens**: section 10 says sync pauses and local
  study continues, so the open is not refused. Phase 4 must read `unsupported_features` before
  syncing. `SUPPORTED_FEATURES` is empty. Per-entity `requires` registers are added by the steps
  that need them.
- **Migration v2** adds `register_clock`, `unknown_register`, `write_guard` and `requirement`. It
  upgraded a real 1.1a collection (a copy of the desktop one, and then the desktop app itself).
- **Verified.**
  - Linux: 64 core tests, the CLI end to end, the desktop app migrating its real collection.
  - Browser: release wasm in headless Brave 143 (Chromium) on Linux. A new collection gets a UUIDv7
    device ID, which proves `getrandom` works with `wasm_js` in a browser. The ID is the same after
    a browser restart. With only the stored installation ID wiped (a collection that moved to another
    browser profile) the device ID changed.
  - Android: the debug APK builds (with `chrono`). It was not installed or run.
- **Not verified:** Windows, Firefox, Safari, the phone at runtime, clock and device IDs under real
  multi-device sync (Phase 4), speed or size of `register_clock` at 50,000 notes.

## Build notes (step 1.2)

Note types, fields and templates, built to this ADR with no change to the decision. These are the
choices step 1.2 left open.

- **Tables.** `note_type` (name, kind, css, sort_field, deleted), `note_type_field` (note_type, name,
  position, deleted) and `template` (note_type, name, position, front, back, deleted). The sync
  entity types are `note_type`, `note_type_field` and `template`. Field `options` from the indicative
  table in section 3 is not there: a register can be added later without breaking anything (section
  10). Every column has a default, so a row that arrives with only some of its registers is harmless.
  Migration v3 creates them and seeds the built-ins.
- **Positions.** One `position` register per field or template, a fractional-index string over
  `0-9A-Za-z` (`notetype::position`, about 100 lines with tests). A list is ordered by `(position,
  id)`. Moving an item writes only its own position, so two devices that reorder at the same time
  both keep their change. If the neighbours leave no room (equal or malformed positions after a
  merge) the list is renumbered, which is a rare path and still converges by last-writer-wins.
  Positions never end in `0`, which is what keeps room before every one of them.
- **Built-ins.** Basic, Basic and reversed, Cloze, with UUIDv5 IDs from the name (`fc-builtin-ids-1`
  namespace), for the note type, each field and each template. They are written by migration v3 with
  the lowest possible clock, `(hlc 0, nil device)`, marked pushed (`sync::seed_row`). So any real edit
  on any device beats the seed, a fresh install cannot overwrite an older rename after the first
  sync, and nothing needs pushing for an untouched built-in. **The seeded content is frozen**: every
  device writes the same clock, so a version that shipped different text would differ silently. A
  change to a built-in later is a new migration that writes with real clocks, and only where the row
  still has the seed clock. They are ordinary note types afterwards: renamed, edited, deleted and
  restored like any other.
- **Delete and remove.** Deleting a note type sets its own `deleted` register. Its fields and
  templates are not touched, they are hidden with it, so restoring is one register. Removing a field
  or template sets its `deleted` register, restoring clears it, and the item keeps its ID and place.
  Tombstones for the notes and cards of a deleted note type come with step 1.3, together with the
  "still referenced, so still alive" rule (section 5).
- **Rules the API checks, and the database does not.** A note type keeps at least one live field and
  one live template, and a cloze type has exactly one template. Field and template names are unique
  within a note type, ignoring case. A removed field or template cannot be restored while a live one
  has its name. `kind` cannot change after creation. Note type names are not checked: duplicates
  are allowed, as for decks (section 6). All of these are checks for the person on this device, so a
  merge can still produce a note type with two fields of one name, and the readers cope (ordering
  by `(position, id)`).
- **Read-time rules.** The sort field is the chosen one if it is a live field, otherwise the first
  live field, and the choice is kept so restoring the field makes it the sort field again. An
  unknown `kind` (from a newer app) reads as standard. Fields and templates whose note type is
  missing or deleted are not shown.
- **Template text** is stored as written. The built-ins use `{{Field}}`, `{{FrontSide}}` and
  `{{cloze:Field}}`, which step 1.4 defines and checks. **Renaming a field does not rewrite
  `{{OldName}}` in templates** (that needs 1.4's parser), so a template that still uses the old name
  is for 1.4's validation to report.
- **Existing notes.** Notes do not exist yet. Their values will be keyed by field ID (step 1.3), so
  no note type operation touches note data. The tests for "changes that affect existing notes" use a
  stand-in table keyed the same way (rename, reorder, remove and restore a field, remove and restore
  a template, delete and restore a note type: the stored values never change, and what is shown
  follows the live fields). Step 1.3 repeats them with real notes.
- **Verified.**
  - Linux: 49 new core tests (113 in `fc-core` in all), the CLI (`fc notetypes`) on a fresh
    collection and on a copy of the real desktop collection, upgraded from v2 to v3.
  - Browser: release wasm in headless Brave 143 on Linux creates a collection at storage version 3 in
    OPFS (which includes the seed, it is in the same transaction), and reopens it after a reload and a
    new browser process.
  - Android: the debug APK builds. Anthony installed it on the phone, which showed storage version 3 (the migration, including the seed, ran on its real collection).
- **Not verified:** Windows, Firefox, Safari, the seeded rows read back in a
  browser (only the version was), merging note types from two collections (step 1.11).

## Build notes (step 1.3)

Notes and cards, built to this ADR with no change to the decision. These are the choices step 1.3
left open.

- **Tables (migration v4).** `note` (note_type, deleted), `note_field_value` and `card` (note,
  template, ordinal, deleted). The sync entity types are `note` and `card`. A card's deck,
  suspension, flag, buried-until and new-card position from the indicative table in section 3 are not
  there: the steps that need them (1.5, 1.7) add them as registers, which section 10 allows. Until
  1.5 a card has no deck.
- **Note values are dynamic registers.** Section 3 says "one register per field value (by field
  ID)", and the registry assumed one column per register. So `note_field_value` is a `DynamicTable`
  next to `SyncedTable`: one row per `(note, field)`, no `id`. In sync data the entity is `note`, the
  entity ID is the note, and the register name is the field ID as UUID text. It has the same guard
  triggers and the same clock rows (`register_clock` with `field` = the field ID text).
  `WriteTx::set_value` is its only write path: it writes nothing and stamps nothing when the value
  does not change (so edits to different fields of a note merge field by field), and an empty value
  that was never set is not stored (a missing row reads as empty, and any real write beats it).
  `check_schema` checks its columns and triggers, and `knows_register` treats a UUID-named register
  of `note` as known, so such a register is never filed as unknown data.
- **Card IDs.** `UUIDv5(namespace = note ID, name = template ID bytes ‖ ordinal as 4 bytes, big
  endian)`. The ordinal is the cloze number, or 0 for a standard template.
- **Which cards a note has.** One rule, `generate::wanted`, applied by `reconcile` on every change to
  a note and every change to its note type: wanted and missing cards are created, wanted and deleted
  cards are restored (same ID, so same history), live and unwanted cards are deleted. Only registers
  that change are written.
  - A **standard** template applies when its front would show a filled field. A field is empty if it
    is only whitespace, `&nbsp;` or tags that show nothing; `<img>`, `<audio>`, `<video>`, `<picture>`,
    `<svg>`, `<object>`, `<embed>` and `<iframe>` count as content. Sections (`{{#F}}`, `{{^F}}`)
    hide what is inside them, so `{{#Add reverse}}{{Back}}{{/Add reverse}}` makes the optional
    reverse card.
  - A **cloze** type makes one card per distinct number in `{{cN::...}}` markers of the fields the
    front shows through `cloze:`. Numbers are 1 to 500 (`scan::MAX_CLOZE`); a larger number is plain
    text, so a typo cannot make a thousand cards.
  - Names in templates are matched exactly (case-sensitive), as written. A removed field reads as
    empty.
  - **This reading of templates was a stand-in for step 1.4** (`note::scan`, replaced and deleted in 1.4, see its build notes below). It
    never fails and reads a malformed template as far as it makes sense. 1.4 defines the grammar,
    its errors and the real renderer, and should replace the scanner with its parser.
- **Adding a note.** `add_note` takes values by field ID and refuses a note that would make no
  cards (`NoCards`: an empty front, or a cloze note with no cloze). It is the only refusal of this
  kind: edits never refuse, because a merge can produce the same state, and the card simply goes to
  the trash.
- **Duplicates.** On the first live field of the note type, compared after removing tags, collapsing
  spaces and lower-casing. Returned with the result of `add_note`, and of `set_note_fields` when it
  changed the first field. Never a block. `find_duplicates` is public for a UI that checks while
  typing. It scans the first-field values of one note type, with no cache. At 50,000 notes it
  took 60 ms (native release, Linux, from a throwaway test: `fc-core` may not read the clock, so the
  timing test is not kept; 1.14's fake collection should measure it again). A cache would have to be rebuilt on every field reorder and
  merge, so it waits until a measurement on the phone or the web says it is needed.
- **Deleting.** Deleting a note tombstones its cards. Deleting a note type tombstones its notes and
  their cards (section 5). Restoring a note reconciles it, so it gets back the cards it should have
  and not those removed before. Restoring a note type restores the notes deleted along with it,
  recognised by their `deleted` clock being at or after the note type's own, so a note deleted on
  its own earlier stays in the trash. Removing a template deletes its cards and restoring it brings
  back the same ones.
- **"Still referenced, so still alive"** (section 5) is a read-time rule for note types: one whose
  `deleted` register is set but which has a live note reads as live, in `note_types`,
  `deleted_note_types` and `note_type`, and can be edited. Nothing is written. Deleting it again
  tombstones the notes that kept it alive. The same rule for decks comes with 1.5.
- **Card visibility.** A card is listed when its own `deleted` register is clear and its note is live.
- **Renaming a field** rewrites `{{OldName}}` in every template of the note type (removed ones too),
  including filters and sections, and then reconciles. Without this, cards would disappear on a
  rename. (This was carried over to 1.4 from 1.2. 1.4 keeps the full grammar, and should reuse or
  replace `scan::rename_field`.)
- **Note type changes reconcile every live note of the type**, in the same transaction as the
  change. Adding a template to a type with 50,001 notes took 3.5 s natively (release). It is a rare
  operation; on the phone and the web it will be slower and 1.14's large fake collection should
  time it.
- **Not done here:** card events and scheduling columns (1.7), decks (1.5), tags (1.6), the merge of
  note registers (1.11: a remote `note` register must be applied through a write method that opens
  the guard, then `reconcile_note_type` or a per-note reconcile recomputes cards), and a purge.
- **Verified.**
  - Linux: 44 new core tests (157 in `fc-core` in all, plus two CLI tests), including the 1.2 stand-in tests redone with real notes, a v3 to v4 upgrade, the guards
    refusing raw SQL on the new tables, and every register having a clock. The CLI (`fc notes`,
    `fc add-note`) on a fresh collection and on a copy of the real desktop collection (v3 to v4).
  - Browser: release wasm in headless Brave 143 creates a collection at storage version 4 in OPFS
    and reopens it after a browser restart.
  - Android: the debug APK builds.
- **Not verified:** Windows, Firefox, Safari, the phone at runtime (the migration to v4 on its real
  collection), reading notes back in a browser (the web API has no note methods yet), the scan and
  reconcile timings on the phone and the web, merging notes from two collections (1.11).

## Build notes (step 1.4)

The template language and card rendering, built to this ADR with no change to the decision. It
replaces step 1.3's stand-in (`note::scan`, deleted) with one reading of the language,
`fc_core::template`. These are the choices step 1.4 left open.

- **Grammar.** A template has a front and a back. Text is HTML and is kept as written. Tags are in
  `{{...}}`, with spaces around the inside allowed.
  - `{{Field}}` shows a field's HTML as written. A name is matched exactly, spaces included
    (`{{Add reverse}}`). A name the note type does not have reads as empty, as a removed field does.
  - `{{filter:Field}}`, chained (`{{hint:text:Field}}`), the filter next to the field applying first.
    The filters are `cloze`, `text` (the field as plain text: no tags, a space where a line ends, a
    stray `<` or `>` escaped) and `hint` (nothing when the field is empty, otherwise
    `<details class="hint"><summary>Field name</summary>...</details>`, which needs no JavaScript).
    There are no other filters, and no `type:`, `Tags`, `Deck` or `Card` yet (decks are 1.5, tags
    1.6).
  - `{{#Field}}...{{/Field}}` shows its content when the field has content, `{{^Field}}...{{/Field}}`
    when it has none. Sections nest. "Has content" is the rule of step 1.3 (`html::has_content`).
  - `{{FrontSide}}` is the rendered front, for the back. On the front it is an error.
  - A field's value is never read as template syntax, so a note cannot add tags to a template.
  - A word before a colon that is not a filter is an error (`{{furigana:Word}}` from a newer app, or
    a field whose name has a colon in it, like `a:b`, which cannot be used in a template as written).
    A colon after a word with a space in it (`Time zone: x`) is part of the name.
- **Cloze markers** in a field are `{{cN::answer}}` or `{{cN::answer::hint}}`, N from 1 to 500, as in
  1.3. The first `::` that is not inside a nested marker starts the hint. Markers can nest (to 8
  levels, deeper ones are text). Something that does not form a marker (no number, out of range, no
  closing `}}`) is plain text. The last is a change from 1.3, which counted `{{c1::` with no `}}`
  as cloze number 1: now it is text, and makes no card.
  - On the front, the card's own number shows `<span class="cloze">[hint]</span>` (`[...]` with no
    hint). On the back it shows `<span class="cloze">answer</span>`. Markers with other numbers show
    their answer as plain text. A template that uses `cloze:` on a standard note type (ordinal 0)
    hides nothing.
- **Mistakes** are reported with a place (line and column, counted from 1 in characters) and a
  sentence that says what to do: a `{{` with no `}}`, an empty tag (`{{}}`, `{{cloze:}}`), a section
  never closed, an end tag with no section (a section skipped by an end tag of an outer one is
  reported as never closed), an unknown filter, `{{FrontSide}}` on the front.
  - **Lenient and strict.** The parser always returns a tree as well as the errors: an unclosed
    section ends at the end of the template, a stray end tag is ignored, an unknown filter does
    nothing, an empty tag is dropped, a `{{` with no `}}` is text. Card generation and renaming a
    field use this reading, because edits and merges never refuse (section 5 of this ADR, step 1.3).
    Rendering and saving use the errors.
  - **Saving.** `add_template` and `set_template_text` refuse a side that has a mistake
    (`NoteTypeError::Template`, "Nothing was saved."). A field name the note type does not have is not
    a mistake, so removing or renaming a field never blocks saving a template. `template::unknown_fields`
    lists them for a screen to warn about, and `template::check` returns the mistakes.
  - **Rendering** a card whose template has a mistake (a merge can bring one) is
    `RenderError::Template` with the same sentences and "Fix the template to see this card." Nothing is
    shown for it, and its cards stay.
- **Output.** `Collection::render_card(card)` returns `RenderedCard { front, back, media }`. Each side
  is a complete document: `<!doctype html>`, `<meta charset>`, the note type's CSS in one `<style>`
  (`</style` in the CSS is written `<\/style`, so the CSS cannot close its element), and a `<body
  class="card">` holding the card. `[sound:name]` becomes `<audio controls src="name">`.
  - **Media** lists the names in the `src` of `<img>`, `<audio>`, `<video>` and `<source>`, front then
    back, without repeats. A web address, a `//` address, a `data:` or `blob:` URL is not listed (the
    frame's CSP blocks them). These are the names the card frame looks up (ADR 0005). The exact name
    format is step 1.10's.
  - **Not done: media in CSS `url()`** and in scripts. ADR 0005 put it in Phase 1, but supporting it
    changes the trusted `frame.html` and depends on 1.10's names. Step 1.10 should do both.
  - **Safe for the sandbox** here means: the output holds only the note type's CSS, the template's
    text, the note's field values and the few elements above, nothing is read as template syntax, and
    no field is trusted. It is meant only for `CardFrame` (ADR 0005). It is not sanitised, and must
    never be put in the app's DOM. The tests check that rendering adds no script, handler, link or
    remote URL of its own.
- **Code.** `template/` has `lex` (never fails), `parse` (tree and errors), `cloze`, `render` and the
  public errors. `html.rs` has the HTML readings that 1.3 had in `scan` (`has_content`,
  `comparison_key`) and the new ones (`plain_text`, `media_names`, `expand_sound`). Generation reads
  the fronts of a note type once for all its notes (`generate::Plan`).
- **CLI.** `fc render <file> <note ID>` prints the front, back and media of each card of a note.
- **Also fixed:** a flaky 1.2 test (`equal_positions_are_ordered_by_id...`) that failed about 60% of
  runs on master: it moved a tied field to the end, which changes nothing when that field is already
  last, and which of the two it was depended on random IDs.
- **Verified.**
  - Linux: 47 more core tests than before (204 in `fc-core` in all, counting the ported `scan` tests, run 12 times with no failure), 1 new CLI test.
    Every kind of mistake has a test that checks its message. A deterministic loop of 3,000 random
    strings of template tokens through the lexer, parser, renderer, generation and rename found one
    real bug (a slice in `safe_css` that split a multi-byte character), which is fixed. The CLI on a
    copy of the real desktop collection (storage version 4) rendered a Polish cloze note with a hint
    and an image. The release-profile wasm build compiles (`cargo xtask check`).
- **Not verified:** a rendered card inside the real card frame (nothing wires `render_card` to
  `CardFrame` until Phase 2, and the web API and the bridge have no note methods), Windows, Firefox,
  Safari, the phone, the timing of reconciling 50,000 notes with the new parser (it reads each
  template once per note type, which should be no slower than 1.3's 3.5 s, but it was not measured).

## Build notes (step 1.5)

Decks and option presets, built to this ADR with no change to the decision. These are the choices
step 1.5 left open.

- **Tables (migration v5).** `deck` (name, parent, options_preset, deleted), `options_preset` (name,
  new_per_day, reviews_per_day, learning_steps, desired_retention, deleted), and a `deck` register
  added to `card`. The sync entity types are `deck` and `options_preset`. `parent` and
  `options_preset` hold an ID, or are empty for none. From the indicative table in section 3, the FSRS
  parameters register of a preset is not there yet: 1.7 and 1.8 own it, and section 10 allows adding
  a register later. There are no relearning steps either, for the same reason.
- **Default deck and preset.** Fixed IDs (`fc-builtin-ids-1` namespace, `default/deck` and
  `default/preset`), seeded by migration 5 with the lowest clock and marked pushed, like the built-in
  note types (the content is frozen in the same way). The Default deck and the Default preset can be
  renamed and edited but **cannot be deleted** (`DeckError::Default`): a card with no deck is in the
  Default deck, and a deck with no usable preset uses the Default preset. The Default deck always
  reads as live, whatever its register says.
- **Existing cards.** Migration 5 rewrites no card. A card whose `deck` is empty (every card made
  before 1.5) reads as being in the Default deck, and has no clock for the register until it is moved.
  Any real write beats a missing clock.
- **Hierarchy.** A deck's `parent` register points at its parent, and `name` is only its own part.
  The path people see joins the display names with `::`.
  - A name cannot contain `::` (`NameHasSeparator`), so a path is never ambiguous in search (1.9).
    This is checked on this device only. A name that arrives from elsewhere with `::` in it is shown
    as written.
  - A deck cannot be moved inside itself or its own sub-decks (`MoveIntoItself`). A move writes only the
    moved deck.
  - **Read-time rules, applied once in `deck::read::Tree::load`.** A parent that is missing, or the
    deck itself, shows the deck at the top level. In a cycle, the deck whose `parent` register has the
    highest `(hlc, device, id)` shows at the top level (section 6), so every device breaks it in the
    same place. Same-named decks next to each other (ignoring case) get " (2)", " (3)" in ID order
    among live decks. A fuzz test with 200 random merges (random parents, loops, missing parents,
    clocks) checks that every deck shows once with a unique path, and that inserting the decks in two
    different orders gives the same answer.
  - Sibling names must be different, ignoring case, for the person on this device (`NameTaken`). A
    merge can still produce two, which the rule above handles.
- **Deleting a deck** writes the tombstones section 5 asks for, in one write: the deck, the decks
  inside it, their live cards, and every note left with no live card. A card whose note has another
  card in a different deck leaves that note alone. Cards keep their history (nothing is purged).
  - If the Default deck was moved inside the deck being deleted, it is moved to the top level first,
    and it and what is inside it are not deleted.
  - **Restoring a deck** brings back the decks, cards and notes whose `deleted` clock is at or after the
    deck's own, the rule 1.3 uses for note types. A card or note deleted on its own before stays in
    the trash. Fails if a live deck next to it has its name.
  - **"Still referenced, so still alive"** (section 5) now also holds for decks: a deleted deck with a
    live card in it, or with a live deck inside it, reads as live (and so do the decks above it). Nothing
    is written. Deleting it again tombstones what kept it alive.
  - **An edit does not undo a deck delete.** The card reconcile does not restore a deleted card
    whose deck is deleted, so editing a note or changing its note type cannot bring back a card the
    user deleted with its deck. Restoring the note (`restore_note`) or the deck does bring it back.
- **Cards and decks.** `Card` has a `deck`. `add_note` still puts cards in the Default deck, and
  `add_note_to_deck` takes a live deck. A card made later by an edit goes to the deck of the note's live
  cards (lowest card ID if they differ), otherwise of any card it has, otherwise the Default deck.
  `move_cards` moves live cards into a live deck, refuses the whole move if any card is not live, and
  writes only the cards whose deck changes.
- **Presets.** `learning_steps` is a list of whole minutes, 1 to 1440, at most 8, stored as text
  (`1 10`); an empty list is allowed. `desired_retention` is 0.70 to 0.99 (the range `scheduling`
  already enforces). The daily limits are 0 to 9999. `set_preset_options` checks everything before
  writing and writes only the registers that change, so two devices that change different options both
  keep theirs. Values that arrive from elsewhere are read in range (clamped, invalid steps left out), so
  reading never fails. The starting values (20 new cards and 200 reviews a day, steps `1 10`, retention
  0.90) are placeholders for Anthony to confirm.
  - Several decks can share a preset, and a new deck starts on the Default preset.
  - **Deleting a preset** sends every deck that uses it to the Default preset (one register each), and
    sets its `deleted` register. Restoring a preset does not give its decks back. A deleted preset that
    a live deck uses (the deck was given it on another device) reads as live, as for decks.
  - A deck whose preset is missing or empty uses the Default preset.
- **Code.** `fc_core::deck` has `read` (the tree and its rules), `ops` (decks and cards), `preset`
  and `error`. The scheduling code in 1.7 reads limits, steps and retention through `Collection::deck_preset`.
- **CLI.** `fc decks <file>` prints the tree with card counts and the presets, `fc add-deck <file>
  <Parent::Child>` adds a deck, and `fc add-note` takes `--deck`. The other deck and preset commands
  wait for 1.14.
- **Verified.**
  - Linux: 48 new core tests (252 in `fc-core` in all), 2 new CLI tests (11 in all). They cover the tree
    and its order, the name rules, moves and cycle checks, deleting and restoring with cards and notes,
    cards in two decks, edits not undoing a delete, cycles broken in both directions, missing parents,
    same names, "still referenced" for decks and presets, option validation and clamping, shared
    presets, preset delete and restore, a clock for every register, raw SQL refused on the new tables,
    and an upgrade from a real version-4 layout. `cargo xtask check` passes (it builds the wasm).
  - The CLI on a copy of the real desktop collection (storage version 4) upgraded it to 5, made
    nested decks and added notes into them. That collection has no notes, so the upgrade of old cards is
    covered by the test only.
- **Not verified:** Windows, Firefox, Safari, the phone and a browser (the web API and the bridge have
  no deck methods, so nothing in a UI or the wasm API changed), merging decks from two collections
  (step 1.11 writes the real merge, the tests here write remote registers with their clocks), the time
  to read the deck tree and count cards with many decks and 50,000 cards (the tree is rebuilt on every
  read, and counts group all live cards, which is likely fine; 1.14's large fake collection should
  measure it).
