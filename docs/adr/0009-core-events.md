# 0009: Core events and study sessions

Status: Accepted
Date: 2026-10-06
Accepted: 2026-10-06, with the answers under "Decisions on review".

## Context

Step 1.12 asks for "stable events and hooks for future add-ons and the UI", with these acceptance
criteria:

- Events for at least: note added, edited, deleted; card answered; study session started and ended;
  sync completed (stub).
- Documented as a public surface, separate from internals.

`PRODUCT.md` says the core exposes stable extension points from the start so add-ons do not depend
on internals, and that add-ons run sandboxed on every platform.

What exists (`docs/plans/1.12-adr-handoff.md` has the details):

- **The core cannot emit.** ADR 0002 says "the core emits through an `EventSink` trait the host
  implements". `EventSink`, `Notice` and `CoreEvent` are in `fc-api`, which depends on `fc-core`, so
  the core cannot call them. This ADR changes how that sentence is carried out (the core gets its own
  event type and listener, and `fc-api` adapts them to `EventSink`). ADR 0002 is not edited. Anthony
  decides whether it gets a pointer here.
- **Delivery is per call.** `OpContext::with_sink` gives a sink to one API call. A merge, or Phase 4's
  background sync, has no call to hang events on.
- **There is no study session** in the core. Nothing defines one.
- **`CoreEvent` has one placeholder variant**, `Debug`, used by the debug-only `debugEmitEvent`.
- **One lock.** `Core` holds the collection behind one `Mutex`, and every `fc-api` method goes through
  `Core::with_collection`. The CLI and the tests use a `Collection` with no `Core`.
- **Every change goes through `Collection::write`**, one transaction per operation that commits when
  the closure returns `Ok`.

Later steps that this must not block:

- **2.3, the review screen**, has a session end screen. It is the first user of the session events.
- **Phase 4** emits `SyncCompleted` for real, and 4.3 starts a sync "after changes".
- **Phase 10** gives add-ons a narrower surface than the UI's (ADR 0002, "Revisit if"). This ADR does
  not design it.
- **1.13 and 1.14** do not use events and need no change.

## Findings

Checked 2026-10-06. Experiments were throwaway Rust crates in the session scratchpad, built with the
repo's pinned toolchain (1.98.1).

1. **A panicking listener called while the collection lock is held breaks the core for good on
   native.** The panic unwinds through the `MutexGuard` and poisons the lock. The next operation's
   `lock().expect("collection lock")` then panics, and so does every one after it, until the app
   restarts.
2. **`catch_unwind` around each listener call prevents that on native.** With it, the panic was
   caught, the lock was not poisoned, the next listener in the list still got its events, and later
   operations ran normally.
3. **On wasm, `catch_unwind` catches nothing.** The same code built for `wasm32-unknown-unknown` and
   run under Bun trapped with `RuntimeError: unreachable`, and the instance answered the next export.
   Unwinding on wasm needs nightly Rust and `-Zbuild-std` with `-Cpanic=unwind`
   ([Rust internals](https://internals.rust-lang.org/t/wasm32-unknown-unknown-panic-unwind-support-via-native-wasm-exceptions/18665),
   [Cloudflare](https://blog.cloudflare.com/making-rust-workers-reliable/)). So on the web a panicking
   listener behaves like any other panic: ADR 0003's worker restart.
4. **A listener that calls back into the core while the lock is held deadlocks.** On Linux the
   `std::sync::Mutex` relock on the same thread was still blocked after 2 s. Rust's docs say this may
   deadlock or panic, so neither outcome can be relied on.

**Not verified:** the Android behaviour of findings 1, 2 and 4 (same `std`, unwinding is on by
default there, not run); finding 3 in a browser (Bun's engine, not WebKit or Chromium); any of this
in the app's own code.

## Decisions and options

### 1. How events leave the core

- **A. Listeners registered with the core, called synchronously after each commit (chosen).** A
  plain `fc_core::events` module has an `Event` enum and a `Listener` trait
  (`fn on_events(&self, events: &[Event])`, `Send + Sync`). `Core` and a standalone `Collection`
  each accept listeners. An operation adds its events to the write transaction, and the collection
  calls every listener after the commit.
  - Good: works with no host loop, so a merge or a background sync emits the same way as a UI call.
    Works the same in the CLI and tests (a standalone `Collection`), native and wasm.
  - Good: events of one operation arrive together, and operations arrive in commit order, because
    delivery happens under the collection lock.
  - Bad: a listener holds up every core call while it runs, and must never call back into the core
    (finding 4). A panic needs containing (findings 1 to 3).
- **B. A queue in the core that hosts drain.** The core appends events. Hosts take them after each
  call, and native also from a thread.
  - Good: no listener runs inside the core, so no reentrancy or panic risk.
  - Bad: every host must drain at the right moments, including after a background sync. Events pile
    up when a host forgets, and the CLI and tests need the same plumbing.
- **C. Events returned through each call's `OpContext`** (today's mechanism, extended).
  - Bad: work with no call (a merge in a background sync) cannot emit. Each method must remember to
    forward. Rejected.
- **D. A persisted event log with per-consumer cursors.**
  - Good: survives restarts, and an add-on could catch up on what it missed.
  - Bad: an extra write per operation and a second record of change beside `card_event` and
    `register_clock`. ADR 0008's change records already answer "what changed since". Nothing needs it
    now (see "Revisit if").

**Delivery inside or outside the lock.** Delivering after the lock is released would let a listener
call the core. But two threads could then deliver in the opposite order to their commits, and a
standalone `Collection` has no lock to release. The UI's listener only forwards to the webview, and
4.3's "sync after changes" trigger only needs to note that a change happened. So: inside the lock,
with the rules in part 3.

### 2. What an event is

- **Events carry IDs and small facts, never card or note content.** Card content is untrusted. Field
  values, tags and templates never go in an event. A consumer that needs content reads it through the
  API, where Phase 10 can filter what an add-on may see.
- **The core's `Event` is not `#[non_exhaustive]`.** Only `fc-api` matches on it across a crate
  boundary, and its conversion must fail to compile when a variant is added, so a new event cannot be
  silently dropped. Stability for outside consumers lives in the API type (part 4).
- **One batch per operation**, in the order the operation emitted them.

The events (Rust names; the API names are the camelCase of these):

| Event | Emitted by | Carries |
| --- | --- | --- |
| `NoteAdded` | `add_note`, `add_note_to_deck` | note, note type, deck, cards made |
| `NoteEdited` | `set_note_fields` | note, IDs of the fields whose value changed, cards added, cards removed |
| `NoteDeleted` | `delete_note` | note |
| `NoteRestored` | `restore_note` | note, cards that came back |
| `CardAnswered` | `answer` | card, deck, the event ID, rating, the open session if any |
| `AnswerUndone` | `undo_answer` | card, the event ID that was voided |
| `StudySessionStarted` | `start_study_session` | session, deck (or none for the whole collection) |
| `StudySessionEnded` | `end_study_session`, a new session, closing the collection | session, deck, why it ended, the summary (part 5) |
| `MergeApplied` | `merge` | counts from `MergeReport`: registers applied, rows added, unknown kept, notes reconciled, cards rebuilt, rejected |
| `SyncCompleted` | nothing yet (stub) | nothing yet |

**When nothing is emitted:**

- A write that fails or rolls back emits nothing. Events sit in the write transaction and are dropped
  with it.
- A no-op emits nothing:
  - deleting a deleted note, or restoring a live one;
  - an edit that changes no field and adds or removes no card;
  - a merge that applies and keeps nothing;
  - ending a session that is not open.
- A merge emits one `MergeApplied`, not an event per note. The UI refetches.

**`SyncCompleted` (stub).** The variant exists now in both enums and in the TypeScript types, with no
fields. In 1.12 only a test emits it. Phase 4's sync driver lives in `fc-core`'s `sync` module, so it
emits `SyncCompleted` through the same crate-internal delivery after a successful round, even when
nothing changed (4.6's indicator needs "synced"). A round that pulls changes emits `MergeApplied`
first. Phase 4 adds fields and, if it needs them, `SyncStarted` and `SyncFailed`.

**Not in scope:**

- Hooks that can change a result (Phase 10).
- Events for deck, tag, note type, preset, suspend and bury, media and saved-search writes. Each gets
  a variant when a screen needs it, which is not a breaking change.
- API methods that call the note, answer or session functions (Phase 2 adds them with the screens).
- New CLI commands.

### 3. Listener rules

Written in the `Listener` docs and the public events page:

- **Return quickly, and never call into the core.** The collection lock is held during delivery
  (finding 4). Hand the events off: send them on a channel, post a message, set a flag.
- **Ignore event kinds you do not know.** New kinds are added without notice.
- **Do not panic.**
  - On native, the core wraps each listener call in `catch_unwind`. A panic is ignored, the listener
    stays registered, and the other listeners still get the batch (finding 2).
  - On the web, a panic traps the worker whatever the core does (finding 3). The write has already
    committed. The call is answered as ADR 0003's panic case and the worker restarts.
- **Delivery is after the commit**, so an event always describes data that is stored.
- Listeners are added for the life of the `Core` or `Collection`. There is no removal: hosts register
  once at startup, and tests use their own collection.

### 4. The public surface (`fc-api`)

- `CoreEvent` gets a variant for each event in part 2, as hand-written DTOs in ADR 0002's style:
  camelCase, IDs as UUID strings, counts as `u32`, no 64-bit integers, and no timestamps (durations in
  milliseconds as `u32`). `rating` and the session end reason are string unions.
- `CoreEvent::Debug` stays, emitted only by `debugEmitEvent` in debug builds. The debug methods stay
  too. ADR 0002's 0.3b notes said they go "when real long methods arrive (1.12)", but 1.12 adds none:
  `debugSlow` is still the only test of progress and cancel. They go with the first real long method
  (1.13's export or Phase 5's import).
- `fc_api::forward_events(core, sink: Arc<dyn EventSink>)` registers a core listener that converts
  each event and sends a `Notice::Event`. Hosts call it once at startup: `fc-native` with its
  `NoticeHub`, `fc-wasm` with `JsSink` in `init`. `OpContext`'s sink stays for progress.
- **Stability promise**, stated on the events page:
  - adding an event kind, or adding a field to one, is not breaking;
  - renaming or removing either, or changing a field's meaning, is breaking and needs an ADR.
- **Documented** on one page, `docs/events.md`: each event, when it fires and when it does not, the
  listener rules and the stability promise. `docs/README.md` links to it. The `fc_core::events`
  rustdoc points there. This is the public surface. Everything else in `fc-core` is internal.

### 5. Study sessions

What 2.3's session end screen needs: how many cards were answered, how many were Again, and how long
it took. "What's next" (due later today, tomorrow) comes from the existing queue functions, not from
the session.

- **S1. An explicit session held in memory by the open collection (chosen).**
  - `start_study_session(deck: Option<Id>) -> Id`.
    - A collection has at most one open session. Starting one while another is open ends the old one
      with reason `replaced`, so a reloaded UI never leaves one open forever.
  - `end_study_session(id) -> Option<SessionSummary>`.
    - Returns `None` and emits nothing if that session is not open.
    - Closing the collection ends the open session with reason `closed`.
  - The session holds only its ID, deck and start time.
  - The summary is counted from `card_event` when the session ends: reviews made on this device since
    the start that are not voided. That gives answered, Again, and the time studied (the sum of the
    answers' stored durations). Elapsed wall time is also given.
    - Undo is handled by the log, with no counter to keep right.
  - Every answer made while a session is open counts, whatever its deck. The deck is a label for the
    UI.
  - `CardAnswered` names the open session.
  - Nothing is stored: a session lost to an app kill or a worker restart is simply never ended.
- **S2. Sessions inferred from the review log** (a gap of N minutes ends one). No API needed. But
  "ended" is only known after the gap, which needs a timer, and the screen cannot get its summary when
  the person finishes.
- **S3. Sessions stored in a table.** They would survive a restart, but cost a migration, and nothing
  needs past sessions: statistics come from `card_event`.
- **S4. Leave session events to 2.3.** The brief puts them in 1.12. Moving them is Anthony's call. The
  cost of S1 now is small, and 2.3 only adds the API methods.

**For Anthony to decide:** S1 now (recommended), or move the session events to 2.3.

## Consequences

- **The core can emit with no call in progress**, so a merge now, and a background sync in Phase 4,
  reach the UI.
- **ADR 0002 changes in one sentence:** the core emits through `fc_core::events::Listener`, which
  `fc-api` adapts to the host's `EventSink`. The `Notice` shape, `EventSink` and the "refetch, do not
  rely on order with the call's result" rule are unchanged.
- **Small changes to existing code:**
  - each listed operation gains one emit;
  - `Collection::write` delivers after the commit;
  - `merge` emits one summary, and its behaviour is unchanged (ADR 0008);
  - each host adds one `forward_events` call at startup.
- **Listeners run under the collection lock.** A slow one slows every core call.
- **A listener can never call back into the core synchronously.** Phase 10's add-ons will run in
  their own sandbox and receive events through the host anyway.
- **Content never travels in events**, which keeps Phase 10's filtering simple.
- **Sessions are in memory only.** A crash loses the open session's end event. Its answers are in the
  log as always.
- **Many writes emit nothing yet** (decks, tags, media and others). A second window that needs to
  hear about those must wait for the variant.
- **No new dependencies, no migration.**

## Decisions on review

Anthony, 2026-10-06.

1. **Study sessions: S1, built in 1.12.** One session held in memory per open collection, with the
   summary counted from `card_event`.
2. **The core `Event` is not `#[non_exhaustive]`**, a change from the handoff's starting point, for
   the reason in part 2.
3. **The debug methods stay** until the first real long method (part 4).

## Revisit if

- **A listener needs to call the core**, or a slow listener visibly stalls calls. Then deliver
  outside the lock, with a sequence number for order.
- **Add-ons (Phase 10) need to catch up on events they missed** while not running. Then look at a
  persisted log (option D), or ADR 0008's change records.
- **Bulk operations (Phase 3 browse, Phase 5 import) emit thousands of events.** Then add a plural or
  summary event for that operation, as `MergeApplied` does.
- **Several windows study at once** (one open session per collection is then too few), or sessions
  need to survive a restart.
- **wasm gains stable unwinding.** Then catch a listener's panic on the web too.

## Build notes (step 1.12)

Built to this ADR and `docs/plans/1.12-extension-points.md`. No migration, no new dependency.

**What was built**

- `fc_core::events` (`Event`, `Listener`, `Listeners`), `Collection::listen`, `Core::listen`, delivery
  after the commit in `Collection::write`, and an emit in each operation of the table in part 2.
- `study/session.rs`: `start_study_session`, `end_study_session`, `EndReason`, `SessionSummary`.
- `fc-api`: the `CoreEvent` variants, `EventRating`, `SessionEndReason`, `SessionSummary`, an exhaustive
  `From<&Event>` and `forward_events`. Bindings regenerated.
- Hosts: `forward_events` in `fc-native` (core and hub are made before the Tauri builder) and in
  `fc-wasm`'s `init`.
- `docs/events.md`, linked from `docs/README.md`.

**Verified**

- Linux: `cargo xtask check` passes (it includes the wasm build, the bindings check, the TypeScript
  checks and the TypeScript tests). 621 tests in `fc-core` (28 new), 29 in `fc-api` (3 new: the JSON
  shape of every event, the forwarding in order, no field text in the notices), 21 in `fc-native` (1
  new: a forwarded core event reaches a hub subscriber).
- Adding a variant to `fc_core::events::Event` without an `fc-api` arm fails to compile (tried, then
  reverted).
- A panicking listener on native: caught, the next listener still hears the batch, the next operation
  works, and the panicking listener is called again for the next batch.
- Delivery order: no operation emits more than one event in a batch yet, so the order of a batch is
  tested at `Listeners::deliver` and not through an operation.

**Not verified**

- The desktop app and the web page were not driven by a person: no API method can cause a real event
  yet, so the hosts are covered by their tests and by starting without errors (see STATUS for what
  was run).
- The phone, Windows, Firefox and Safari. Panic behaviour on wasm and on Android (findings 1 to 4 were
  Linux and Bun only).

**Deviations from the plan**

1. **`StudyError::NotFound` reads "That card or deck no longer exists."** A missing deck now reaches it
   from `start_study_session`. `fc-cli/tests/study.rs` was updated to the new text.
2. **The merge and two-device tests are in a new file, `sync/merge_events_tests.rs`**, not in
   `events_tests.rs` and `session_tests.rs`. The merge test helpers are `pub(super)` to `sync`, and I
   did not widen them.
3. **`events::tests::Recorder` is `pub(crate)`**, shared by `events_tests.rs`, `session_tests.rs` and
   `merge_events_tests.rs`.
4. **`Core::open_collection` ends the old collection's session as `closed` (under the lock), and
   `Core::close_collection` now closes under the lock.** The plan only said `Collection::close` ends
   the open session. This keeps the end event in order with the others.
5. **Crate-only helpers beyond the plan:** `Collection::open_session_id` and `end_open_session` (ends
   with no event if the summary query fails while closing).
6. **`MergeApplied.unknown_kept` is `unknown_registers + unknown_rows`** of the `MergeReport`. The ADR
   said "unknown kept" without saying how the two are combined.
7. **The `fc-api` types `EventRating`, `SessionEndReason` and `SessionSummary` are exported** (and have
   generated TypeScript files). The plan named only the string unions.
8. **`fc-wasm`'s `init` registers the forwarder once per instance**, behind a `FORWARDING` flag, so a
   second `init` does not send every event twice.
9. **Two existing TypeScript tests** (`client.test.ts`, `tauriTransport.test.ts`) narrow with
   `e.kind === "debug"` before reading `message`, because `CoreEvent` is now a union.
10. **Two planned test ideas were dropped or changed.** "The order of a batch" is tested at `Listeners::deliver`, because no operation emits more than one event yet. A "listener sees stored data" test was not written: a listener may not call the core, so it could only check after the fact. Delivery after the commit is by construction in `Collection::write`.
11. **The session summary test** answers one card before the session starts, to show it is left out.
    The plan's version had no such answer.
