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
