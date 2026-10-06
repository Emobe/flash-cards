# Core events

The public surface for "something changed in the collection". The UI uses it now. Add-ons will use a
narrower form of it in Phase 10. Everything else in `fc-core` is internal and may change without notice.

Design and reasons: `adr/0009-core-events.md`. Rust: `fc_core::events`. Wire form: `CoreEvent` in
`fc-api`, delivered as `{ type: "event", event: { kind, ... } }` and received with
`client.onEvent`. The TypeScript type is `packages/core-client/src/generated/CoreEvent.ts`.

## What an event is

- It is sent **after the change is stored**, so it always describes data that is there.
- An operation's events arrive together, in the order it emitted them. Operations arrive in the
  order they committed.
- It carries **IDs and small facts, never card or note content** (no field text, tags or templates).
  A consumer that needs content reads it through the API. Card content is untrusted.
- IDs are UUID strings. Counts are `u32`. Durations are milliseconds. There are no timestamps.
- Treat an event as "something changed, refetch", not as the only source of truth.

## The events

| `kind` | Sent when | Fields |
| --- | --- | --- |
| `noteAdded` | a note is added | `note`, `noteType`, `deck`, `cards` (the cards it made) |
| `noteEdited` | a note's field values changed, or its cards were added or removed | `note`, `fields` (IDs of the fields whose value changed), `cardsAdded`, `cardsRemoved` |
| `noteDeleted` | a live note goes to the trash | `note` |
| `noteRestored` | a deleted note comes back | `note`, `cards` (the cards that came back) |
| `cardAnswered` | a card is answered | `card`, `deck`, `event` (the review in the card's history), `rating` (`again`, `hard`, `good`, `easy`), `session` (the open study session, or `null`) |
| `answerUndone` | an answer is taken back | `card`, `event` (the review now voided) |
| `studySessionStarted` | a study session starts | `session`, `deck` (`null` for the whole collection) |
| `studySessionEnded` | a study session ends | `session`, `deck`, `reason` (`ended`, `replaced`, `closed`), `summary` |
| `mergeApplied` | a merge, or a restore or import of a backup file (1.13), changed anything | `registersApplied`, `rowsAdded`, `unknownKept`, `notesReconciled`, `cardsRebuilt`, `rejected` |
| `syncCompleted` | **a stub**: nothing sends it until Phase 4 | none yet |
| `debug` | the debug-only `debugEmitEvent` method (debug builds) | `message` |

`summary` of `studySessionEnded`: `answered`, `again`, `studiedMs` (the answers' durations added up),
`elapsedMs` (wall time from start to end). It counts the answers made on this device since the session
started that are not undone, whatever their deck.

### When nothing is sent

- A write that fails or rolls back.
- A no-op: deleting a deleted note, restoring a live one, an edit that changes no field and adds or
  removes no card, a merge that applies and keeps nothing, ending a session that is not open.
- A merge, a restore or an import sends one `mergeApplied`, not an event per note. `registersApplied` of a restore counts what it moved to the trash too.

### What has no event yet

Decks, tags, note types, presets, suspend and bury, media and saved searches. Each gets a kind when a
screen needs it, which is not a breaking change. Also not here: hooks that can change a result
(Phase 10), and any API method that calls the note, answer or session functions (Phase 2 adds them with
the screens). Sessions are held in memory only: one that is lost to an app kill or a worker restart is
never ended.

### Study sessions

At most one session is open per collection. Starting one while another is open ends the old one with
`replaced`. Closing the collection ends the open one with `closed`. `cardAnswered` names the open
session.

## Rules for a listener (Rust)

A listener implements `fc_core::events::Listener` and is added with `Core::listen` (or
`Collection::listen`). `fc_api::forward_events` is the one the hosts use.

- **Return quickly, and never call into the core.** Listeners run while the collection lock is held,
  so a call back into the core deadlocks. Hand the events off: send them on a channel, post a message,
  set a flag.
- **Ignore kinds you do not know.** New kinds are added without notice.
- **Do not panic.** On native, a panic is caught and ignored, the listener stays registered and the
  other listeners still get the batch. On the web (wasm), a panic traps the worker whatever the core
  does, the write has already committed, and the worker restarts (ADR 0003).
- Listeners last for the life of the `Core` or `Collection`. There is no removal.

## Stability promise

- Adding an event kind, or a field to one, is **not** breaking. Consumers must ignore both.
- Renaming or removing a kind or a field, or changing what a field means, is **breaking** and needs an
  ADR.
