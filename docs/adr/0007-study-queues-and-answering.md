# 0007: Study queues, answering and the day boundary

Status: Accepted
Date: 2026-10-06
Accepted: 2026-10-06, with the changes under "Decisions on review".

## Context

Step 1.7 makes the core decide what to study next and record answers. Its acceptance criteria:
queues respect deck limits, nested decks and learning steps; answering updates scheduling per ADR
0004 and records a review; undo of the last answer; suspend, bury and unbury; a configurable day
boundary tested across time zones; a simulated multi-day test with expected due counts.

Much is already fixed by accepted ADRs:

- **ADR 0004.** FSRS-6 through `fsrs` 6.6.2, behind `fc_core::scheduling` (our `Rating`, `Memory`,
  `Scheduler`). No `fsrs` type reaches `fc-api`.
- **ADR 0006, sections 3 and 4.** Reviews and manual scheduling actions are immutable **card events**
  with UUIDv7 IDs, merged by union. Each records the card, its time and the UTC offset, the inputs
  (preset, desired retention, FSRS parameters: "a hash or an inline copy, 1.7 decides"), the result
  (memory state, scheduling state, learning step, due) and the previous event it built on. The fuzz
  is seeded from the event's own ID. A card's due date, memory, queue and step are a **local cache**
  folded from its events, never synced. Undo is a **void** event. Suspended and buried-until are card
  **registers**, not events.
- **ADR 0006, section 13 item 5** asks for the cache to be rebuildable, and its Consequences say a
  change to how events are folded must be versioned.
- **ADR 0003.** `fc-core` reads no clock and starts no threads. Cached state must be rebuildable from
  the database (the web worker can be restarted at any time).
- **The host `Clock`** (step 1.1b) gives Unix milliseconds and the current UTC offset in minutes. It
  does not give a time zone name.
- **Step 1.5** left for 1.7: what limits mean for nested decks, relearning steps, and the FSRS
  parameters register on presets. Presets already have daily new and review limits, learning steps
  (whole minutes) and desired retention, read through `Collection::deck_preset`.

What is still open, and decided here: how learning steps and FSRS work together, how a "day" is
defined, how limits apply to nested decks, in what order cards come, how events are stored, how the
queue is computed, how undo and bury behave, and the fuzz.

## Findings

Checked 2026-10-06. Experiments were throwaway, in a scratch crate outside the repo, with the repo's
toolchain (1.98.1), `fsrs` 6.6.2 and `rusqlite` 0.40.2 (bundled), release build, on Linux.

1. **FSRS-6 handles same-day reviews itself.** `next_states(memory, 0.9, 0)` (0 elapsed days) uses
   FSRS-6's short-term formula. It counts same-day reviews but ignores whether they were 1 minute or
   6 hours apart. Community guidance for FSRS (Anki forums, 2025-2026) is to keep one or two short
   learning steps, not none and not many. With default parameters:
   - A new card answered Good: S = 2.31 days. Good again the same day: S stays 2.31. So learning
     steps answered Good do not inflate the first interval (2 days).
   - Again, then Good the same day: S = 0.28 days (an FSRS interval of about 7 hours).
   - A review card (S = 10.96, D = 5) answered Again after 11 days: S = 1.46. Then Good the same day
     (a relearning step): S = 1.50, interval 1.5 days.
   - Easy on a new card: S = 8.3 days.
2. **The optimiser sees learning steps the same way.** `memory_state` over the history Good (0 days),
   Good (0 days), Good (2 days) gives exactly the same S and D as feeding the reviews one by one
   through `next_states`. So if every learning-step review updates the memory state, step 1.8's
   optimiser trains on the same thing the scheduler used.
3. **`fsrs` has no fuzz for `next_states`.** Fuzz exists only inside its simulator. We need our own.
4. **Queue queries are cheap at 50,000 cards** (in-memory SQLite, 300 decks, a cache table with an
   index on `(state, due_day)`, 1,000,000 events over 1,000 days with an index on the day):
   - due, new and learning counts for every deck, all 50,000 cards: 38 ms;
   - the first 50 due reviews in order: 0.1 ms;
   - answers done today per deck, from the events: 1.8 ms.
   Not measured on the phone or in a browser, where it will be slower.

**Not verified:** any of this on Android, in a browser or on Windows; the queue timings with a real
collection file on disk; how FSRS-6 behaves with steps longer than a day (allowed up to 1,440
minutes by step 1.5).

## Decisions and options

Each part below lists the options and the one chosen.

### 1. Learning steps and FSRS

- **A. Steps decide timing, FSRS updates memory at every answer (chosen).** A new card goes through
  the preset's learning steps, a lapsed card through its relearning steps. While in steps, the step
  decides when the card comes back. Every answer, including same-day ones, updates the FSRS memory
  state with the elapsed study days (0 within a day). Graduation uses FSRS's interval.
  - Good: matches finding 2, so the optimiser and the scheduler agree. Matches how the step 1.5
    presets were already shaped (steps in minutes). Language learners get the short same-day
    repetition they expect.
- **B. FSRS only, no steps.** Every answer gets FSRS's interval, rounded to whole days.
  - Bad: `PRODUCT.md` asks for learning steps in presets. A forgotten card would not come back the
    same day (finding 1: 0.28 days rounds to 1 day).
- **C. Steps without memory updates until graduation.** The steps are a separate warm-up, and FSRS
  sees only the graduating answer.
  - Bad: throws away same-day answers that FSRS-6 models, and the optimiser in 1.8 would see a
    different history from the one the scheduler used.

The rules for option A:

| State | Again | Hard | Good | Easy |
| --- | --- | --- | --- | --- |
| New or learning (step *i*) | first step | repeat step *i* | step *i*+1, or graduate after the last | graduate |
| Review | lapse: first relearning step | FSRS interval | FSRS interval | FSRS interval |
| Relearning (step *i*) | first relearning step | repeat step *i* | step *i*+1, or graduate after the last | graduate |

- **Graduating** gives `max(1, round(FSRS interval for that rating))` days, then the fuzz (part 7).
- **No steps.** With empty learning steps every answer on a new card graduates. With empty
  relearning steps a lapse stays a review card due after `max(1, round(FSRS Again interval))` days.
- **Preset changes.** If a card is on step 3 and the preset now has two steps, it reads as being on
  the last step. A card's state never needs rewriting when options change.
- **New preset registers (migration).** `relearning_steps` (same format and limits as learning
  steps, starting value `10`) and `fsrs_parameters` (empty means the defaults; otherwise 21 numbers,
  or 17 or 19 from older FSRS versions filled the way `fsrs` fills them). Step 1.8 writes
  `fsrs_parameters` from the optimiser. A value that does not parse reads as the defaults, so reading
  never fails.
- **Maximum interval.** 36,500 days, a constant for now. A preset option can be added later.

### 2. The day boundary

- **A. A collection-wide "day starts at" hour, applied with the device's current UTC offset
  (chosen).** A study day is
  `floor((unix_ms + utc_offset_minutes × 60,000 − start_hour × 3,600,000) / 86,400,000)`,
  numbered from 1970-01-01. `start_hour` is 0 to 23, starting value 0 (midnight), changed in its own
  setting. It is a synced collection setting (part 8). "Today" uses the clock's current offset.
  An event's day uses the offset recorded on the event.
- **B. A time zone name (IANA) stored in the collection.** Dates would be computed in that zone
  whatever the device says.
  - Bad: the host `Clock` gives only an offset, so it would need a time zone database in `fc-core`
    (`chrono-tz` or similar), several hundred KB of wasm. And a traveller wants "today" to follow
    the phone, not a zone chosen months ago.
- **C. A per-device start hour.** Two devices could disagree about which day an answer belongs to,
  so daily limits and statistics would differ between devices for no reason the user can see.

Consequences of option A, each covered by a test:

- **Review cards are due on a study day, not at a time.** A card due on day N is due from the start
  of day N in whatever time zone the device is in. Flying to another continent does not make a
  day's reviews disappear or arrive twice.
- **Learning cards are due at a time** (Unix ms), because steps are minutes.
- **Elapsed days for FSRS** are `today − day of the last answer`, never below 0. Travelling west can
  make "today" earlier than the last answer's day; it then counts as the same day.
- **DST.** The day containing a DST change is 23 or 25 hours long. Nothing else changes.
- **Changing the start hour** moves where every day begins, from then on. Due days stay as stored.

### 3. Daily limits and nested decks

Studying a deck studies it and every deck inside it. Each deck has its preset's limits. "Done
today" is counted from the non-voided events of the current study day, by the card's current deck:
a new card counts when its first answer is today, a review counts when the card was in the review
state when answered. Learning and relearning answers do not count towards limits.

- **A. Every deck on the path counts, including those above the one chosen (chosen).** A card in
  `Polish::Vocab::Food` can be shown only while `Polish`, `Polish::Vocab` and `Polish::Vocab::Food`
  each have room left, whichever of them you chose to study.
  - Good: a limit means what it says. "20 new cards a day in Polish" is 20, even if the user studies
    the sub-decks one by one.
  - Bad: a sub-deck's count can go down because a sibling was studied.
- **B. Only the chosen deck and those inside it count** (decks above are ignored).
  - Good: clicking a sub-deck always shows its own budget.
  - Bad: studying each sub-deck in turn gets round the parent's limit.
- **C. Only the chosen deck's limit counts.** Simplest, but a sub-deck's own limit means nothing
  when the parent is studied.

**Decided: A**, with a switch on each deck (Anthony, on review):

- **"Limits include sub-decks"**, a deck register `limits_include_subdecks`, on by default. On, the
  deck's limits count its own cards and everything inside it (option A). Off, they count only the
  cards directly in the deck, so a parent used as a category ("Languages" holding "Polish" and
  "Spanish") does not cap the decks inside it. Those use only their own limits.
- It is on the deck, not the preset, because it is about how the tree is arranged, and one preset
  can be shared by decks with different roles.

The new and review limits are separate: new cards are not held back by a review backlog. Review
debt features (later phase) can change this without changing anything stored.

### 4. Order of cards

Fixed rules for 1.7 (preset options for order can be added later without changing stored
data):

1. **Learning and relearning cards that are due now**, earliest first.
2. **Reviews due today or earlier, with new cards spread evenly among them.** Reviews by due day,
   then card ID (card IDs are UUIDv5, so this is a stable pseudo-random order). New cards in the
   order their notes were added (note IDs are UUIDv7, so time-ordered), then template position, then
   cloze number. This needs no "new-card position" register or counter (ADR 0006 section 6 rules
   out counters); one can be added when a "reposition" action needs it.
3. **When nothing else is left**, a learning card due within 20 minutes (a constant) is shown early.
   Otherwise the queue says when the next learning card is due.

**Siblings.** Cards of the same note (both directions of "Basic and reversed", the numbers of a
cloze) are separate cards with their own schedules, and each answer counts as its own review. A new
or review card is held back until tomorrow if another card of the same note was answered today
(non-voided), so the second is answered from memory and not from what was just seen. Learning cards
are not held back. This is a read-time rule that writes nothing, so it merges across devices, and
undoing an answer lifts it. A preset register `space_siblings`, on by default, turns it off.

### 5. How the queue is computed

- **A. Computed from the database on each call (chosen).** One SQL pass over the local schedule
  cache, joined with cards and decks, plus today's events for "done today" and siblings, then the
  limits applied in Rust. Finding 4: 38 ms for counts over every deck at 50,000 cards on Linux.
  - Good: no state outside the database, so a web worker restart or a merge needs nothing rebuilt.
    Counts are always right.
- **B. Stored per-deck counters**, updated on every write. Faster to read, but every write, undo,
  move, suspend, merge and change of day must keep them right, and they are a second truth.
- **C. A queue kept in memory for a study session.** Fast between cards, but it must be rebuilt
  after a worker restart or a merge, and goes stale when another window changes something.

**Decided: A.** If the phone or the web is too slow for answering one card after another (Phase
2), add option C on top of A as a cache, not instead of it.

### 6. Card events: storage

- **One local table, `card_event`, with typed columns (chosen).** Columns: `id`, `card`, `kind`,
  `time_ms`, `utc_offset`, `device`, `previous`; for reviews `rating`, `duration_ms`; inputs
  `preset`, `desired_retention`, `parameters` (an ID, below), `steps` (the step list that applied, as
  text); result `state_before`, `state`, `step`, `stability`, `difficulty`, `due_day` or `due_ms`;
  for a void, `target`. Columns a kind does not use are null.
  - Alternative: one JSON payload column. Easier to extend, but statistics (1.8) and search (1.9)
    filter and group by rating, day and state, which typed columns and indexes make simple.
- **FSRS parameters by reference, not inline (ADR 0006 left this to 1.7).** A parameter set is
  stored once in an append-only `fsrs_parameter_set` table whose ID is
  `UUIDv5("fc-fsrs-parameters-1", the 21 values as little-endian f32 bytes)`. Every device computes
  the same ID for the same values, so the defaults need no seeding and two devices that store the
  same set make one row. An event keeps the 16-byte ID.
  - Alternative: inline. 21 × 4 bytes = 84 bytes per event, about 84 MB for a million reviews.
- **The learning steps are an input too**, because recomputing a concurrent event (ADR 0006 section
  4) needs them as well as the parameters and retention.
- **Immutable.** Guard triggers refuse UPDATE and DELETE on `card_event` and `fsrs_parameter_set`,
  and INSERT outside the write path, like the register tables. Whether a row has been pushed is kept
  in a local table, not on the row.
- **Event times never go backwards for a card.** An event's time is `max(now, time of the event it
  builds on + 1 ms)`. Without this, a device whose clock was set back would write an event that sorts
  before the one it built on, and the fold would treat it as concurrent.
- **Kinds.** 1.7 writes `review` and `void`. The other kinds in ADR 0006 (set due date, reset,
  reschedule) come with the steps that need them. Every event records its result, so the fold can
  apply a kind it does not know as long as it builds directly on the previous event (it takes the
  recorded result). Unknown kinds from a newer app are kept and relayed; step 1.11 decides how
  unknown fields of an event are kept.

### 7. Fuzz

Our own rule, since `fsrs` has none (finding 3). For an interval of *i* whole days:

- *i* < 3: no fuzz.
- otherwise a whole number of days chosen uniformly from `[i − r, i + r]`, with
  `r = max(1, round(0.05 × i))`, never below 2 or above the maximum interval.
- The choice comes from the 8 random bytes at the end of the event's UUIDv7, as ADR 0006 requires, so
  recomputing the event gives the same day.

The intervals shown on the answer buttons before answering are unfuzzed ("about 11 days"), because
the event ID does not exist yet.

### 8. Collection settings

The day start hour is the first collection-wide setting. A synced table `collection_setting`, one
row per setting, ID `UUIDv5("fc-setting-1", key)`, registers `key` and `value` (text). A missing row
means the default. This is the "collection setting" entity in ADR 0006's indicative table, with no
`deleted` register (resetting means writing the default).

### 9. The schedule cache

- A local table `card_schedule`: card, state, step, due_day, due_ms, stability, difficulty, day of
  the last answer, the last applied event, and the number of answers and lapses (for 1.8 and 1.9).
  A card with no row is new.
- Answering computes the result, inserts the event and updates the card's row in one transaction.
- `rebuild_schedule` folds every card's events (ADR 0006 section 4). A `schedule_cache_version` in
  `meta` is compared on open, and a mismatch rebuilds the cache, so changing the fold in a later
  version is a version bump. Rebuilding 50,000 cards × 20 reviews was 219 ms in step 0.7.
- **The fold for 1.7**: drop voided events, sort by `(time_ms, id)`, and for each event take its
  recorded result when its `previous` is the event just applied; otherwise recompute it from the
  current state with its own rating, time and recorded inputs, and the fuzz from its own ID. Step
  1.11 calls the same function for the cards a merge touched.

### 10. Undo

- **A. Undo voids the newest non-voided review made on this device (chosen).** It writes a void
  event, so it syncs (ADR 0006: "Undo of an answer that has already synced"). The fold then drops
  the review, which puts the card's state, today's counts and held-back siblings back as they were.
  Calling undo again goes back one more answer. It is refused if the card has a later event (from
  another device after a merge), with a message saying so.
- **B. Undo deletes the review row.** Simple locally, but impossible once the review has synced, and
  events are immutable by design.
- **C. An in-memory undo stack.** Lost when the app or the worker restarts.

Undo covers answers only, as the brief asks. Suspend and bury are single register writes that the
UI can reverse directly.

### 11. Suspend and bury

New card registers (migration): `suspended` (0 or 1) and `buried_until` (a study day; the card is
hidden while today is at or before it, 0 for none). Bury sets it to today, unbury sets it to 0, and a
new day unburies with no write. Both are ordinary registers, so "suspend on one device, review on
another" keeps both (ADR 0006 section 11). Answering a suspended or buried card is allowed (the UI
will not offer it, and a merge can produce it anyway). Answering a deleted card is refused.

## Consequences

- One scheduling path: the state machine in part 1 computes every result, and the fold reuses it
  for recomputation. The tests in 1.7 pin it, and changing it later is a cache version bump.
- Review history is the source of truth for scheduling, as ADR 0006 intended. Daily counts,
  siblings and undo all read the events, so they merge across devices with no extra state.
- Limits can be exceeded across devices for days studied offline on both (ADR 0006 section 11
  already says this).
- New schema: two card registers, a deck register, three preset registers, the `collection_setting`, `card_event` and
  `fsrs_parameter_set` tables (synced), and local `card_schedule` and push-state tables. The sync
  registry gains an append-only table kind next to `SyncedTable` and `DynamicTable`.
- No new dependencies.
- Card order options, a reposition action, a maximum interval option and a learn-ahead option are
  left out. Each can be added later without migrating data.

## Revisit if

- Answering one card after another is slow on the phone or the web (option C of part 5).
- Users want limits that ignore parent decks (option B of part 3), or review debt features need new
  cards held back by reviews.
- FSRS starts modelling sub-day intervals in a way that makes learning steps unnecessary (option B of
  part 1).
- Event storage grows too large on the phone (then compact the typed columns or move rarely read
  inputs out of the row).
- The fold gives visibly different due dates across devices (ADR 0006 already lists this).

## Decisions on review

Anthony, 2026-10-06:

1. **Limits on nested decks:** every deck on the path counts, including decks above the one chosen
   (part 3, option A), plus the per-deck "limits include sub-decks" switch so a parent can be only a
   category.
2. **Siblings:** separate cards and separate reviews, held back until the next day by default, with
   the preset switch `space_siblings` to turn it off (part 4).
3. **New cards** are spread evenly among reviews (part 4).
4. **The day starts at midnight** by default, set in its own setting (part 2). Relearning steps start
   at `10` minutes (part 1). Step 1.5's 20 new and 200 reviews a day, steps `1 10` and retention 0.90
   stay.
5. **Two PRs**, 1.7a then 1.7b, as in `docs/plans/1.7-study-queues.md`.

## Build notes (step 1.7a)

Answering and the schedule, built to this ADR (parts 1, 2, 6 to 10, and the preset and setting
registers). Queues, limits, suspend and bury and the simulated multi-day test are step 1.7b, so
parts 3, 4, 5 and 11 are not built yet. Where the build differs from the text above:

- **`card_event` has a `day` column**, the study day of the event, worked out when it is written from
  its time, its UTC offset and the start hour at that moment. Part 6 lists the columns without it. It
  is there because recomputing an event during the fold (part 9) needs the days between two events,
  and without the column that would read the start hour setting as it is now, which another device can
  have changed since. It is also what 1.7b's "done today" will read. Indexed.
- **`steps` records both lists**, learning then relearning, `1 10|10`. Part 6 says "the step list that
  applied". A concurrent event is recomputed from whatever state the card is in when the fold reaches
  it, which can differ from the state it was answered in (Easy on a learning card, folded after another
  device graduated the card), so the one list that applied is not always the one needed.
- **Namespaces are 16 bytes**, so the IDs are `UUIDv5("fc-setting-ids-1", key)` and
  `UUIDv5("fc-fsrs-params-1", the 84 bytes)`, not the longer names written in parts 6 and 8. They are
  part of the sync format now. The ID of the default parameter set is
  `77e38a6d-65b2-5412-9163-cb372a98777b`, pinned in a test and worked out separately with SHA-1
  outside the crate.
- **One generic write path for append-only rows**, `WriteTx::insert_row(entity, id, columns)`
  (insert or ignore, listed in the local `unpushed_row` table when new), in place of `insert_event` and
  `insert_parameter_set`. The registry has the third table kind, `AppendOnlyTable`, with guard triggers:
  INSERT needs the write path and UPDATE and DELETE are always refused (checked with raw SQL inside and
  outside the write path, and by `check_schema`). `WriteTx::local()` (crate only) gives the transaction's
  connection for the local `card_schedule`, so answering updates the cache in the same transaction.
- **Indexes** on `card_event`: `(card, time_ms)`, `(time_ms)`, `(day)` and a partial one on `target`
  (undo looks for a review without a void).
- **`fsrs_parameters` is read only.** `Preset.fsrs_parameters` is empty for the defaults, or 21 numbers
  (17 and 19 are filled by `fsrs::check_and_fill_parameters`, public in `fsrs` 6.6.2); a stored value
  that does not parse reads as empty. `PresetChange` has `relearning_steps` only. The Default preset
  got clock rows for the two new registers in migration 7 (lowest clock, already pushed), because every
  register of every row has a clock.

Choices the ADR left open:

- **The state machine** is in `scheduling/machine.rs`, a pure function of the card, the elapsed days,
  the rating, both step lists, the scheduler and the event ID. Hard repeats the step (it does not
  average two steps). With empty learning steps every answer on a new card graduates, Again too, with
  that rating's FSRS interval (at least 1 day). A card on a step the preset no longer has is on the last
  step. Fuzz is applied to every interval in days, reviews included, never to a step in minutes.
- **Event time**: `max(now, time of the card's last applied event + 1 ms)`, and the event's `day` comes
  from that time. A void is timed the same way, so it never sorts before what it voids.
- **The fold** (`study/fold.rs`): a review that builds on the last applied event takes its recorded
  result; one that does not is recomputed from the card at that point with its own rating, time, day
  and recorded inputs. If that is impossible (a parameter set this device does not have), the recorded
  result is applied, so a review is never lost from the card. An unknown kind counts only where it
  builds on the last event, and is kept otherwise. Voids and the reviews they target are left out. A
  void's `previous` is the card's last applied event, so a review after an undo builds on what came
  before the voided one.
- **Undo** voids the newest non-voided review written by this device. It is refused with `LaterAnswer`
  when the card has any other non-voided answer after it in the fold's order, or one that was built
  directly on it (a clock that ran behind on another device). Nothing to undo is `Ok(None)`. Undo works
  for a card deleted since.
- **The cache** is rebuilt when `meta.schedule_cache_version` differs from the build's (now 1), on open,
  and by `rebuild_schedule`. It streams one card's events at a time.
- **CLI.** `fc answer`, `fc undo`, `fc schedule`, and `--now <RFC 3339 time>` and `--utc-offset
  <minutes>` for any command (the offset in `--now` is used unless `--utc-offset` is given). `fc notes`
  prints card IDs.

**Timing** (Linux, release, a throwaway test, not kept): `rebuild_schedule` over 50,000 cards with 20
events each (1,000,000 events) takes 1.7 s when every event builds on the one before, and 2.2 s when
every event has to be recomputed. Most of it is reading the rows. A version bump of the fold therefore
costs a one-off wait on open, longer on the phone and in the browser (not measured).

**Verified.**

- Linux: 81 new core tests (370 in `fc-core`) and 4 new CLI tests (17). The state machine has every
  cell of the part 1 table, empty steps, a card on a removed step, graduation never under 1 day, the
  maximum interval, and five Good answers from new with steps `1 10` (10 minutes, then 2, 11, 46 and 163
  days, which are the intervals of step 0.5, with the first one matching finding 1). Fuzz: the range for
  several intervals, every day in the range reached, the same ID gives the same day, none under 3 days,
  and an even spread. The day boundary: start hours 0, 4 and 23, offsets −720 and +840, an answer just
  before and just after the boundary, a DST change in the middle of a run, travelling west, and changing
  the start hour. Events: every input and result recorded, immutable (raw UPDATE and DELETE refused
  inside and outside the write path), time never before the previous event's, equal parameter set IDs
  in two collections. The fold: a rebuild equals the cache that 900 random answers built (with offset
  changes and lapses), a concurrent pair is recomputed, insertion order does not matter, unknown kinds,
  a missing parameter set, a damaged cache and a version bump on open. Undo: state, due, memory and the
  lapse count back, repeated undo, refused after a later answer (also with an earlier time but built on
  it), a remote answer that was itself voided, a void is stored, a review after an undo. Upgrade from a
  real version-6 layout. A mutation check (the fold trusting every recorded result) fails two fold tests.
- The CLI on a copy of the real desktop collection (it was still at storage version 2, so it went from
  2 to 7 in one transaction): three answers, an undo, and raw SQL on an event refused.
- `cargo xtask check` passes, including the wasm build.

**Not verified:** the phone and the browser (the web API has no study methods, nothing in a UI
changed, the phone migrates when the APK next runs), Windows, Firefox, Safari, timings on the phone
and the web, merging events from two collections (1.11 writes the real merge; the tests insert remote
events directly), answering a suspended or buried card (those registers come in 1.7b).

## Build notes (step 1.7b)

Queues, limits, siblings, suspend and bury, and the simulated month, built to parts 3, 4, 5 and 11.
Migration v8. Where the build differs from the text above, or settles something it left open:

- **Registers** (migration 8, column defaults, no row rewritten): `card.suspended`, `card.buried_until`,
  `deck.limits_include_subdecks` (on), `options_preset.space_siblings` (on). The seeded Default deck and
  preset got lowest-clock rows for their new register, as in migration 7. `PresetChange.space_siblings`,
  `Deck.limits_include_subdecks` and `set_deck_limits_include_subdecks`.
- **Counts (approved in the plan).** `learning` counts every learning or relearning card due before the
  end of today's study day, so a deck list shows what is left today. `new` and `review` are the cards the
  queue would show: a card held back by the sibling rule or cut by a limit is not counted.
- **`Waiting`** is for a learning card due later today (past the 20-minute learn-ahead). One due after
  the study day ends is not waited for: the queue says `Done`.
- **Limits.** For a card in deck X, a deck A counts it if A is X, or A is above X and has
  `limits_include_subdecks` on. A deck above that has the switch on still counts X past a deck between
  them that has it off. Limits are applied in card order, separately for new and review cards, over the
  whole path to the top deck whichever deck is being studied. A card whose deck row is missing reads as
  in the Default deck.
- **Spreading new cards among reviews** is stateless: the next review is at position
  `(done + 1/2) / total` of the day's reviews, the next new card likewise, and the earlier position goes
  first (a review on a tie). Totals include what was done today in the deck being studied, so the choice
  does not change as cards are answered. With 6 reviews and 2 new cards the order is R R N R R R N R.
- **Done today** reads today's non-voided `review` events (by the `day` column added in 1.7a) for cards
  that still exist, by the card's current deck. Siblings use the same rows.
- **Bury** hides a card while today is at or before `buried_until`. `unbury_cards` writes only cards that
  are buried now, `unbury_deck` does the same for a deck and what is inside it. Only cards that change are
  written by suspend, unsuspend and bury.
- **Test-only seeded IDs** (`id::seeded`, `#[cfg(test)]`): `Id::generate` can draw its random bits from a
  fixed sequence on the calling thread. The fuzz is chosen from an event's ID, so without it the pinned
  30-day table would change on every run. Nothing outside tests can reach it.
- **CLI.** `fc due [--deck]`, `fc next [--deck]` (the Default deck if none is given, like `add-note`; it
  does not answer), `fc suspend`, `fc unsuspend`, `fc bury`, `fc unbury` with card IDs, and
  `fc unbury --deck <deck>`, which the plan did not list.
- **Existing tests changed** only where the schema moved: version 7 became 8 in four upgrade tests and
  the CLI info test. The tag upgrade test now writes its note by hand, because the note API reads the
  new deck column and an old layout does not have it.

**The simulated month** (`study/simulation_tests.rs`): 200 Basic notes, half in `Polish` and half in
`Polish::Food`, the Default preset (20 new, 200 reviews, steps `1 10`, retention 0.90), studying `Polish`
every morning until the queue says `Done`, with ratings fixed by card number and answer number (85% Good,
10% Again, 3% Hard, 2% Easy). The table of 30 days is pinned (new, learning and review cards waiting in
the morning, then new cards started, reviews done and learning answers). Checked alongside: never more
than 20 new or 200 reviews a day, every card started by day 10, no review shown before its due day, the
cache after the month equals a rebuild from the events. The same table comes out at UTC offsets −720,
−300, +330 and +840, across a daylight saving change in each direction, and with the day starting at 4 am
with sessions at 03:30 and at 22:00. A session never needs `Waiting` here (the 20-minute learn-ahead is
longer than the steps), so `Waiting` is tested on its own. A second test studies after two months away: 120
overdue reviews, 30 offered a day.

**Timing** (Linux, release, a throwaway test, not kept): 50,000 cards in 301 decks, 25,000 of them due
reviews and the rest new: `deck_counts` for every deck 130 ms, `next_card` 120 ms, `study_counts` for one
deck 120 ms, all on an in-memory database. Answering one card (1.7a's code) takes 39 ms at that size,
because it reads the deck tree, which counts every card. The step 1.7 findings measured the SQL part
alone (38 ms). The phone and the web will be slower; Phase 2 measures the real study screen, and
option C of part 5 (a queue kept in memory on top of this) is the answer if it is too slow.

**Verified.**

- Linux: 53 new tests in `fc-core` (47 queue tests, 5 simulation tests, the v7 upgrade; 423 in total)
  and 5 new CLI tests (22 across files). A mutation check: making ancestor decks never limit a card
  fails 8 tests, including the pinned month; making siblings never held fails 4.
- The CLI on a copy of the real desktop collection (it holds no cards): opened and upgraded to storage
  version 8, then three notes added and `due`, `next`, `suspend`, `unsuspend`, `answer`, `bury`,
  `unbury --deck` and a `--now` a long way ahead all gave what the tests expect.
- `cargo xtask check` passes, including the wasm build.

**Not verified:** the phone and the browser (the web API has no study methods and no UI changed; the
phone migrates to 8 the next time the APK runs), Windows, Firefox, Safari, timings on the phone and the
web, merging suspend and bury from two collections (1.11; the tests write the other device's registers
and events directly), limits when two devices study the same day (ADR 0006 section 11 already allows
going over).
