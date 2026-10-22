# 0014: Editing notes, multi-select and bulk actions

Status: Accepted
Date: 2026-10-17
Accepted: 2026-10-17 (every question as proposed, see Decisions on review)

## Context

Step 3.2 (`docs/phases/03-browse-edit-capture.md`) has these acceptance criteria:

- Edit a note from the browser and from the review screen.
- Multi-select on both platforms; bulk move, tag, suspend, delete, reschedule, change note type.
- Undo for bulk actions, or a clear confirmation where undo is not possible.

The phase's exit criterion is "any card can be found and edited on the phone in a few taps". 3.1
built the "found" half. This is the "edited" half.

The brief says "Sonnet build", but no accepted ADR decides:

- where the editor opens;
- how a selection is made on a touch screen;
- what "reschedule" stores;
- what changing a note type does to cards and their history;
- where undo lives.

What exists and constrains this:

- **ADR 0011:** the field editor (`FieldEditor`, ProseMirror), the toolbar in `BottomAction`, tags
  (`TagInput`), the duplicate warning. It sets a rule for this step: a field whose HTML does not
  survive a load and save unchanged opens in a plain HTML text box, with a line saying so
  (`roundTrips` in `editor/html.ts` already checks this). Field HTML is loaded only through an inert
  `DOMParser` and the schema.
- **ADR 0013:** the Browse screen. Rows do nothing when tapped in 3.1, and 3.2 opens the editor from
  them. Notes by default, with a switch to cards. Pages of 100, up to 1,000 per call. The list is
  read again on `focus` and `visibilitychange`, and "3.2's edits will add a refresh after their own
  writes".
- **ADR 0010:** `BottomAction` and the keyboard inset, tap targets of 48 px under 600 px, `Dialog`
  (with `variant="sheet"`) taking one history entry so Android back closes it, the 2.5a notice that
  offers Undo after a deck delete.
- **ADR 0006 and 0007:**
  - Every write is registers with clocks, through `Collection::write`.
  - Scheduling is a cache folded from immutable card events. ADR 0006 section 3 lists the event
    kinds "set due date", "reset to new", "reschedule" and "void". ADR 0007 part 6 says the kinds
    after `review` and `void` "come with the steps that need them", and that an app that does not
    know a kind applies its recorded result when it builds on the card's last event.
  - Undo of an answer is a void event. Suspend and bury are single register writes that the UI can
    reverse directly (ADR 0007 part 10).
  - A card's ID is `UUIDv5(note, template ‖ ordinal)`, and reconcile finds a note's cards by that
    ID.
- **ADR 0009:** `NoteEdited` and `NoteDeleted` are emitted per note. Deck, tag, suspend and other
  writes emit nothing yet. A variant is added when a screen needs one.
- **ADR 0003:** on the web, calls run one at a time, and the worker can restart (cancel, panic),
  which loses anything held in memory.
- **The core today** has `set_note_fields`, `set_note_tags` (separate writes), `delete_note` and
  `restore_note` (one note per write), `move_cards`, `add_tags`, `remove_tags`, `suspend_cards`,
  `unsuspend_cards`. It has nothing to change a note's type, set a due date or reset a card.
- **The phone is unavailable** (STATUS, from 2026-10-16), so nothing here was tried on it.

## Findings

### Run here (throwaway, in the session scratchpad, outside the repo)

1. **Changing a note's type in place starts every card again as New, and changing it back brings
   the old cards back exactly.** A scratch crate on `fc-core` (`master` at `0c85886`, release build) made a Basic
   note, answered its card Good three times over six days (review, stability 26.0, due day 20748),
   then wrote the note's `note_type` register to Basic and reversed, copied the two values to the new
   type's field IDs with `set_note_fields`, and so reconciled:
   - the old card was deleted (3 history entries kept on it);
   - two new cards were made, both New with 0 answers.

   Writing `note_type` back to Basic and reconciling again restored the same old card ID, still
   review, 3 answers, due day 20748, the same stability and difficulty, and deleted the two new
   cards. So history is never lost by a type change, but without more work it is hidden, and a type
   change resets every card the person has learned. This is what decision 6 has to fix.

### Looked up (web, 2026-10-17)

2. **Selection on touch screens.** Material's selection guidance (Material 2; no Material 3 page came
   up) says a long press on an item, or tapping its avatar, enters selection mode, then a tap toggles
   an item, and a contextual bar replaces the app bar while items are selected. The older Android
   docs say long press should be reserved for selection. For desktop it says not to show checkboxes
   all the time: one on hover, and on every item once one is selected
   ([Material 2 selection](https://m2.material.io/design/interaction/selection),
   [Material 1 patterns](https://m1.material.io/patterns/selection.html)).
3. **Long press in a web page on Android** is a text-selection gesture. A WebKit bug thread says a
   page should cancel `selectstart` or use `user-select: none` to stop it, not rely on cancelling
   `contextmenu` ([WebKit bug 98173](https://www2.webkit.org/show_bug.cgi?id=98173)). No current
   source says how today's Android WebView treats a cancelled `contextmenu`. **Not verified.**
4. **An undo that disappears on a timer** is a time limit under WCAG 2.2.1 (Timing Adjustable). One
   design system (Shopware) says not to make a snackbar the only way to recover from a destructive
   action, and a GitLab design issue argues an Undo inside an auto-closing toast fails 2.2.1
   ([WCAG 2.2.1 summary](https://tabnav.com/academy/wcag/success-criterion-2.2.1),
   [Shopware snackbar](https://meteor.shopware.com/components/snackbar),
   [GitLab issue 2771](https://gitlab.com/gitlab-org/gitlab-services/design.gitlab.com/-/issues/2771)).
   These are opinions, not rulings, but they point the same way as 2.5a's notice, which stays until
   dismissed.

### Not verified

- Long press on a row in the phone's WebView (finding 3), and the selection bar with the keyboard
  closed and open.
- The editor in an overlay on the phone with the keyboard open (2.4 checked the Add screen, which is
  a page, not an overlay).
- Bulk writes at 10,000 cards on the phone and in wasm. 1.6 measured adding two tags to 50,000 notes
  at 1.9 s on Linux. Deleting notes today is one write per note.
- Passing 50,000 IDs through the bridge (decision 2). ADR 0002 measured 5 MB attachments at 250 ms
  on the phone; a JSON list of 50,000 IDs is about 2 MB. Not measured.
- Windows, Firefox, Safari.

## Decision 1: editing a note

**One `NoteEditor` component, opened as a layer over the screen it came from**, in Browse (tap a
row) and in Study (an Edit button). Not a route.

- **Under 600 px** it is a full-screen layer: heading "Edit note", Back, the fields, the tags, the
  deck, and Save in `BottomAction` with the formatting toolbar (as on Add). **From 600 px** it is a
  wide `Dialog` (46 rem).
- **Why a layer:** Browse keeps its scroll position, loaded pages and selection underneath, and Study
  keeps its session (the study screen ends its session when it closes, ADR 0010 build notes for
  2.3b). The layer takes one
  history entry, like `Dialog`, so Android back closes it.
- **Content:**
  - The note type, read only, with "Change note type" (decision 6, for this one note).
  - One `FieldEditor` per field. A field that fails `roundTrips` opens in a plain text box showing
    its HTML, with "This field has formatting the editor cannot show" (ADR 0011). The HTML is shown
    as text, never put in the page.
  - Tags (`TagInput`).
  - Deck: a `<select>`. Changing it moves every card of the note. If the cards are in several decks
    it reads "Several decks" until changed.
  - The duplicate warning, leaving out the note itself.
  - Delete note (decision 4's confirmation and undo).
- **Saving is explicit.** Save, or Ctrl+Enter or Ctrl+S on desktop. Back or Escape with unsaved
  changes asks "Discard your changes?" (Discard, Keep editing). With no changes it just closes.
- **One write.** The core gains `edit_note_with(note, values, tags)`: changed field values and the
  tag set in one transaction, like `add_note_with` (ADR 0011). A deck change is a separate
  `move_cards` call made after it.
- After a save the layer closes and the screen under it says "Saved." with Undo (decision 4).
  Browse reads its list again. Study renders the same card again on the side it was on; if the edit
  removed that card (a cloze taken out), it moves to the next card.
- **In Study** the Edit button sits next to Undo, and `e` opens the editor on desktop. The answer's
  time stops while the editor is open, so editing does not count as study time.

### Options considered

| Option | For | Against |
|---|---|---|
| **A layer over the screen (chosen)** | Browse keeps scroll, pages and selection. Study keeps its session and card. One component for both | A full-screen layer is a new shell piece (a `Dialog` variant). Deep links to a note need a route later |
| A route `#/notes/<id>` | Linkable. Back works by itself | Browse unmounts and loses its place. Leaving Study ends the session, so editing mid-session splits it |
| A side pane next to the list from 1024 px | The desktop habit: click rows, edit on the right | A second layout for the same task. Study still needs the layer. Possible later (Revisit) |
| Save as you type | No Save button. Nothing to lose | A register write and a `NoteEdited` per pause, so many clocks and events. No clear point for Undo. Half-typed text reaches cards and sync |

## Decision 2: selecting several

**Selection is a set of IDs of what the rows are**: note IDs in Notes view, card IDs in Cards view.
Switching views clears it.

- **Under 600 px:**
  - A **Select** button in the search panel, next to Sort, in thumb reach. A **long press** on a row
    also enters selection with that row chosen (finding 2). Rows get `user-select: none` and cancel
    `selectstart` and `contextmenu` (finding 3). The button is there because long press is
    undiscoverable and unverified on the phone.
  - In selection mode each row shows a checkbox, and a tap toggles the row instead of opening the
    editor.
  - The search panel in `BottomAction` is replaced by the **selection bar**: "✕ 12 selected",
    "Select all 340", and the actions (decision 3). Android back or ✕ leaves selection mode (one
    history entry, like `Dialog`).
- **From 600 px:** a checkbox at the start of each row, shown on hover and focus, and on every row
  once one is chosen (finding 2). Clicking the checkbox toggles. Clicking the row opens the editor.
  Shift+click chooses a range of loaded rows, Ctrl+A chooses all matching, Escape clears. The
  selection bar sits above the list, where the search panel is.
- **Select all matching** asks the core for every ID that matches the current search
  (`searchIds`), so it is not limited to the loaded pages. The count comes from that answer.
- **After an action** the selection stays, so several actions can be made on the same notes (move,
  then tag). After a delete it is cleared.

### Options considered

| Option | For | Against |
|---|---|---|
| **Select button and long press under 600 px, checkboxes from 600 px (chosen)** | Discoverable on the phone. The Android habit as a shortcut. Desktop gets checkboxes and Shift | Two entry points. Long press is unverified in the WebView (finding 3) |
| Long press only | The Android habit | Hidden. Not available with a mouse. Competes with text selection |
| Checkboxes always visible | No mode | Narrow rows on a phone. Material says not to show them all the time |
| Act on the search, not on IDs (`{ query, except }`) | No large ID list over the bridge | What is acted on can differ from what was shown. Deselecting after "select all" needs exceptions. Undo needs the IDs anyway |

## Decision 3: the actions

| Action | Acts on | Asks first | Core |
|---|---|---|---|
| Move | cards | the deck, from a picker | `move_cards`, returning each card's old deck |
| Add tags, Remove tags | notes | the tags (`TagInput`) | `add_tags`, `remove_tags`, returning the notes that changed |
| Suspend, Unsuspend | cards | nothing | `suspend_cards`, `unsuspend_cards`, returning the cards that changed |
| Delete | notes | a confirmation with the counts | new `delete_notes`, `restore_notes` (one write each) |
| Set due date | cards | the day | new event kind `set_due` (decision 5) |
| Reset to new | cards | a confirmation | new event kind `reset` (decision 5) |
| Change note type | notes | the new type and the mapping | new `change_note_type` (decision 6) |

- **Notes or cards.** A card action on notes applies to every live card of those notes ("12 notes
  (18 cards)"). A note action on cards applies to their notes. Cards view is there for precision.
- Suspend shows as Unsuspend when every selected card is suspended.
- Under 600 px the bar shows Move, Tag, Suspend, Delete and More (Set due date, Reset to new, Change
  note type, Remove tags). From 600 px they fit in one row.
- Each bulk operation is **one write** in the core, so a failure changes nothing and a merge or
  another window never sees half of it.
- **Delete asks first** ("Delete 12 notes and their 18 cards?"), even though it can be undone,
  because there is no trash screen yet to get them back later. Set due date and Reset ask because
  they change schedules. Move, tags and suspend act at once.

## Decision 4: undo

**One level of undo for the last change made from Browse or the editor, held in memory by `fc-api`.**

- Each change method answers an `undo` token (an ID and a sentence: "Moved 18 cards to
  Polish::Verbs."). `fc-api`'s `Core` keeps the latest one with what is needed to reverse it:
  - Move: each card's old deck. Undo moves each back.
  - Tags: the notes that changed. Undo removes or adds them back on those notes only.
  - Suspend: the cards that changed. Undo reverses those.
  - Delete: the notes. Undo is `restore_notes`.
  - Edit: the old values and tags of the fields that changed. Undo writes them back.
  - Set due date and Reset: the event IDs written. Undo voids them (decision 5).
  - Change note type: undo is a change back with the reverse mapping (decision 6).
- `undoChange(id)` reverses it if it is still the latest, and answers what it did. Otherwise: "That
  can no longer be undone." A new change replaces the kept one.
- **The notice** (`role="status"`) says what happened, with Undo and Dismiss. It stays until the
  next change, leaving the screen, or Dismiss. No timer (finding 4).
- **Undo is a new write**, not a rollback. If something changed the same data in between (a merge),
  the undo still writes the old values. A void is refused for a card that has a later event, as
  `undo_answer` is, and the notice says how many could not be undone.
- **Lost on restart.** A web worker restart or closing the app loses the kept change. Undo then
  answers "That can no longer be undone."
- The answer Undo in Study (ADR 0007) is separate and unchanged.

### Options considered

| Option | For | Against |
|---|---|---|
| **One level, kept in `fc-api` memory (chosen)** | No large payloads to the UI (moving 10,000 cards keeps 10,000 old decks in Rust). No migration. Same for every platform | Lost on a worker restart. One level only |
| The UI keeps the inverse (the methods answer old decks, changed IDs) | No state in `fc-api` | Large answers for large selections. Each screen reimplements undo |
| A local undo table in the collection | Survives restarts. Several levels | A migration and a log to keep consistent with merges. Nothing asks for more than one level |
| No undo, a confirmation before every action | Simplest | The brief prefers undo. Confirmations get tapped through |

## Decision 5: set due date and reset (the "reschedule" actions)

"Reschedule" in the brief is two actions: **Set due date** and **Reset to new**. Recomputing memory
with new FSRS parameters (ADR 0007 build notes, 1.8) is not in this step.

**Two new card event kinds**, as ADR 0006 section 3 planned. Each records its result like a review,
so the fold takes it as it is when it builds on the card's last event, and an older app does the
same with a kind it does not know (ADR 0007 part 6).

| Kind | Result | Recomputed when concurrent |
|---|---|---|
| `set_due` | a review card due on the chosen study day; memory and the day of the last answer kept | the card's memory and last answer day at that point, with the recorded due day |
| `reset` | a new card: no memory, step 0, no last answer day | the same |

- **Set due date** offers Today, Tomorrow, In N days, and Between N and M days. A range picks a day
  for each card from the event ID's random bytes, as the fuzz does (ADR 0007 part 7), so it is the
  same on recompute. Learning and relearning cards become review cards on that day. **New cards are
  left as they are**, and the confirmation says so ("3 new cards are not changed"): a new card has
  no memory for a review to build on.
- FSRS sees the real time since the last answer when the card is next answered, so showing a card
  early or late needs nothing else.
- **Reset** keeps the history. The counts of answers and lapses (used by search and sort) stay
  totals of the whole history. The optimiser trains only on answers after a card's last reset.
- **Undo** writes a `void` for each event. `void` today targets reviews; it now targets any event.
  `undo_answer` is refused when a later `set_due` or `reset` exists on the card, as for a later
  answer.
- Both kinds record the day of the last answer in a new `last_day` column (decision 6's migration),
  so the fold never takes the event's own day for it.
- No schedule cache version bump: no stored event changes meaning.

### Options considered

| Option | For | Against |
|---|---|---|
| **Card events (chosen)** | What ADR 0006 planned. Merges by union. Undo is a void. History shows what happened | Two kinds in the fold, the optimiser and history |
| A "due override" card register | One write | Scheduling state would live in two places, the cache and a register, and a later answer would have to clear it. ADR 0006 keeps the schedule derived from events |

## Decision 6: change note type

**The note keeps its ID and changes type in place, with a field map and a card map. A card carried
to a new template keeps its schedule through a third new event kind, `transfer`.**

- **One note type at a time.** Every selected note must have the same type, otherwise the action
  says "Select notes of one note type to change it."
- **The window:** pick the new type, then:
  - **Fields:** for each new field, which old field fills it, or Nothing. The default is the old
    field with the same name (ignoring case), then the one in the same position.
  - **Cards:** for each new template, which old card's schedule it takes, or Start as new. Defaults
    the same way. Cloze to cloze maps by cloze number and has no choice. Standard to cloze, or cloze
    to standard, starts every card as new, and the window says so.
  - A summary before Change: "12 notes. Card 1 keeps its schedule. Card 2 starts as new. Back is
    not used."
- **What the core writes, in one write** (`change_note_type(notes, to, fields, templates)`):
  1. each mapped value onto the new field's ID, and empty onto a new field with nothing mapped;
  2. the note's `note_type` register;
  3. reconcile, as today: the old cards go to the trash, wanted new cards are made or restored;
  4. for each new card with a source card that has a schedule, a `transfer` event whose result is the
     source card's schedule (state, step, due, memory, last answer day) and which names the source;
     the new card also takes the source's deck and suspension.
- **Old values are not touched.** They stay stored under the old field IDs, hidden, as ADR 0006
  section 10 says for removed fields. So **undo is a change back** with the reverse maps (finding 1:
  the old cards come back with their history), with `transfer` events onto them for anything studied
  meanwhile.
- **History follows the source.** `card_history` and the optimiser read a transferred card's source
  first (and its source, in a chain), so 3.3's card info and training see one history. The count of
  answers and lapses of the new card starts at 0 in the cache (a known gap, see Consequences).
- **Schema:** `card_event` gains two nullable columns, `source` (a card ID) and `last_day`
  (migration 12, the rule for new append-only columns in ADR 0008 part 6). Decision 5 uses
  `last_day` too, so the migration comes with it.
- **Events:** `NoteEdited` for each note, with the fields that changed and the cards added and
  removed.

### Options considered

| Option | For | Against |
|---|---|---|
| **A. In place, with a `transfer` event (chosen)** | Card IDs stay derived, so two devices can never make the same card twice. Fits the event model: a recorded result. Undo is a change back. Old values are kept | A third event kind. History is two cards linked by `source`, which history and the optimiser must follow. Answer counts restart |
| B. Cards keep their IDs: rewrite the card's `template` register and find cards by template and ordinal | One card, one history. No new event | Breaks the rule that a card's ID comes from its template. A device that gets the note's new type before the card's new template would make a second card, which then needs a read-time rule to remove. Touches every reconcile |
| C. A new note with copied fields, the old one deleted | No change to how notes work | A new note ID, so tags, history and links are split as well. Undo is two operations |
| D. In place, no carry, a confirmation that schedules start again | Smallest | Changing Basic to Basic and reversed would reset every card already learned. That is the common reason to change type |

## Decision 7: the API

New `fc-api` methods. A `target` is `{ notes: string[] }` or `{ cards: string[] }`.

| Method | Input | Output |
|---|---|---|
| `getNote` | `noteId` | `noteTypeId`, `fields` (field ID and value), `tags`, `deckIds` of its live cards, `cardCount` |
| `updateNote` | `noteId`, `fields`, `tags` | `cardsAdded`, `cardsRemoved`, `duplicates`, `undo` |
| `searchIds` | `query`, `mode` | `ids` |
| `moveCards` | `target`, `deckId` | `count`, `undo` |
| `addTags`, `removeTags` | `target`, `tags` | `count`, `undo` |
| `suspendCards`, `unsuspendCards` | `target` | `count`, `undo` |
| `deleteNotes` | `target` | `notes`, `cards`, `undo` |
| `setDueDate` | `target`, `fromDays`, `toDays` | `count`, `skippedNew`, `undo` |
| `resetCards` | `target` | `count`, `undo` |
| `changeNoteType` | `target`, `noteTypeId`, `fields` (new field ID and old field ID or null), `templates` (new template ID and old template ID or null) | `notes`, `undo` |
| `undoChange` | `undoId` | `message` |

- `undo` is `{ id, message }`. Counts are `u32`. No 64-bit numbers (ADR 0002).
- Errors as in ADR 0013: a refused change is `invalidInput` with the core's sentence, a note or
  deck that is gone is `notFound`, the rest is logged and answers the generic message.
- The CLI gets commands for the new `Collection` operations (`tests/coverage.rs` requires it):
  `edit-note --tag`, `delete-notes`, `restore-notes`, `set-due`, `reset`, `change-notetype`.

## Consequences

- **A migration** (12): two nullable columns on `card_event`. No register is added, and no synced
  table changes shape. Older apps keep and relay the columns as unknown (ADR 0008 part 6).
- **Three event kinds** (`set_due`, `reset`, `transfer`), and `void` widened to any kind. The fold,
  `card_history`, `daily_counts` (they are not answers, so they count nowhere) and the optimiser
  learn them. An app before this one applies their recorded result (ADR 0007 part 6), so it shows the
  right due date, but its optimiser would see a gap in a transferred card's history.
- **A full-screen layer** joins the shell next to `Dialog`, for the editor.
- **Selection mode replaces the search panel** in `BottomAction` under 600 px. The phone has not
  confirmed either the panel or this bar.
- **Undo is one level and in memory.** It is gone after a restart, and only the last change can be
  undone.
- **A transferred card's answer and lapse counts restart at 0**, so `lapses:>3` and the "Times
  forgotten" sort miss its earlier lapses. Following `source` in the fold would fix it, at the price
  of the fold reading another card's events.
- **Notes and cards in the trash** have no screen. Undo is the only way back in the app until one
  exists (CLI `restore-notes` meanwhile).
- **Events:** bulk deletes emit one `NoteDeleted` per note, a type change one `NoteEdited` per note,
  as ADR 0009's table promises. A delete of 5,000 notes sends 5,000 notices to the webview; the build
  measures it.
- **No new dependency**, no bridge command, no permission or CSP change, nothing that costs money.
  Card content still never enters the page: the editor loads fields through the schema, and the HTML
  box shows text.

## Revisit if

- **Long press misbehaves in the WebView**: drop it and keep the Select button.
- **Desktop editing feels slow** one note at a time: a side pane next to the list from 1024 px.
- **People want more than one undo**, or undo after a restart: a local undo table.
- **5,000 per-note events stall the webview**: a summary event for bulk changes (ADR 0009 "Revisit
  if").
- **Lapse counts after a type change matter** to someone: follow `source` in the fold.
- **A trash screen** is built: deleted notes link there, and the delete confirmation can go.
- **Recomputing memory after optimising** is wanted: a `reschedule` event kind, the one ADR 0006
  named and this step does not build.

## Questions for Anthony

1. **The editor as a layer** over Browse and Study, with an explicit Save and "Discard your
   changes?" on Back. The alternative is saving on Back (no question asked, undo after).
2. **Selection:** a Select button plus long press on the phone, checkboxes from 600 px.
3. **Delete asks first** even though it can be undone, because there is no trash screen.
4. **"Reschedule" means Set due date and Reset to new.** New cards are left alone by Set due date.
5. **Change note type keeps schedules** through the `transfer` event (option A), with a migration.
   Option D (no carry, a warning) would be much smaller but resets learned cards.
6. **One level of undo, in memory**, with a notice that stays until the next change or Dismiss.
7. **In Notes view, card actions apply to every card of the notes.**
8. **Five PRs** as in the plan (`docs/plans/3.2-edit-and-bulk.md`).

## Decisions on review

Anthony's answer on 2026-10-17, in chat: "set it as accepted, merge". He changed nothing, so every
question is answered as proposed:

1. The editor is a layer over Browse and Study, with an explicit Save and "Discard your changes?" on
   Back.
2. A Select button plus long press on the phone, checkboxes from 600 px.
3. Delete asks first.
4. "Reschedule" is Set due date and Reset to new, and Set due date leaves new cards alone.
5. Change note type keeps schedules through the `transfer` event (option A), with migration 12.
6. One level of undo, in memory, with a notice that stays until the next change or Dismiss.
7. In Notes view, card actions apply to every card of the notes.
8. Five PRs: 3.2a to 3.2e.

## Build notes (step 3.2a, core and API)

Branch `step/3.2a-edit-api`. No migration, no sync format change, no new dependency, no UI.

- **`edit_note_with(note, values, tags)` (`note/ops.rs`).** `set_note_fields`'s checks and
  writes, plus the tag set when `tags` is `Some` (`set_note_tags`'s rules: spelled as the
  collection spells it, matched ignoring case), in one write. One `NoteEdited` when a value, a tag or
  a card changed; a tags-only edit sends it with no fields (`docs/events.md` says so now). It answers
  `NoteEdit`: the `NoteChange`, the old value of each changed field and, when the tags changed, the
  old tags as stored. `set_note_fields` now calls it with `None`. A field given twice takes the last
  value, as before.
- **`delete_notes`, `restore_notes`.** One write each, one `NoteDeleted` or `NoteRestored` per note
  in one batch. A note that is not there refuses the whole call before anything is written; one
  already in the wanted state is skipped. They answer `NotesChange` (the notes and the cards that
  went or came back). `delete_note` and `restore_note` call them.
- **Changed sets.** `move_cards` answers `(card, old deck)` for each moved card (a card given twice
  is moved once), `add_tags` and `remove_tags` the `(note, tag)` pairs written, `suspend_cards` and
  `unsuspend_cards` the cards that changed. Callers that used the old counts take `.len()`.
- **Reads.** `search_ids(query, mode)`: every match, by ID, no limit and no sort (a shared
  `with_filter` compiles the query for it and for `search`). `cards_of_notes` and `notes_of_cards`
  turn a selection into the other kind in one prepared statement each, and refuse with `NotFound` if
  anything in it is gone.
- **The kept change (`fc_core::undo`).** `KeptChange { id, reverse }` with a `Reverse` per kind
  (decks, tags to take off or put back, suspension, notes to restore, an edit). `Core` keeps one
  (`keep_change`, `take_change(id)` takes it once if it is still the latest) and drops it when a
  collection opens or closes.
- **`fc-api` (`crates/fc-api/src/edit.rs`).** `getNote`, `updateNote`, `searchIds`, `moveCards`,
  `addTags`, `removeTags`, `suspendCards`, `unsuspendCards`, `deleteNotes`, `undoChange`. A `Target`
  is `{ notes }` or `{ cards }`, resolved in one place (`Ids::cards`, `Ids::notes`). A change that
  changed nothing answers `undo: null` and keeps the earlier token. `updateNote` takes an optional
  `deckId`: the deck is checked first, then the edit and the move are two writes with one undo
  ("Saved."). `undoChange` answers `notFound` with "That can no longer be undone." for a token that
  is not the latest. Undo moves cards back with one `move_cards` per old deck and changes tags with
  one call per tag; a group whose card, note or deck is gone is skipped and the message says how many
  could not be put back. An edit's undo moves the cards back before writing the old values, since
  the values can remove a card. `FieldValue` is shared with `addNote`.
- **CLI.** `edit-note … [--tag <tag>]…` (`edit_note_with`), `delete-note` and `restore-note` take
  several IDs and use the bulk operations, `search --ids` (`search_ids`). `tests/coverage.rs` has the
  new rows; `cards_of_notes` and `notes_of_cards` have a reason instead of a command.
- **Tests.** 18 core tests (`note/edit_tests.rs`): values and tags with the old ones answered,
  writing them back, tags only, a no-op writes nothing, no tags leaves tags alone, a bad tag refuses
  everything (digest and events), a deleted note, a card made by an edit, bulk delete and restore
  (events in one batch, skipped and missing notes, the same cards back), the changed sets, the reads,
  `search_ids` in both modes, and replays of edits and bulk deletes into another collection (same
  digest). The random tour test now also edits with tags and deletes and restores several notes.
  One `Core` test for the kept change. 15 `fc-api` tests through `dispatch`: each method, a note
  target on card actions, undo of each kind, one undo for edit and move, an undo that goes once, a
  second change making the first token too old, a new collection dropping it, a move back to a
  deleted deck, undo after a merge still writing the old value, no collection open. One more CLI
  test.

### Measured

Release, Linux, through `dispatch` (so with the JSON), on a copy of a `fc fake` collection in the
session scratchpad (`/tmp`), with a throwaway test that was not committed:

| Call (10,000 notes, 13,629 cards) | Time | Its undo |
|---|---|---|
| `deleteNotes`, every note | 597 ms | 760 ms |
| `suspendCards`, every note (13,499 changed) | 638 ms | 416 ms |
| `suspendCards`, every card | 407 ms | 350 ms |
| `addTags`, one tag on every note | 285 ms | 298 ms |
| `moveCards`, every note | 706 ms | 681 ms (one write per old deck) |
| `moveCards`, every card | 456 ms | 662 ms |

`searchIds` on a `fc fake` collection of 37,000 notes and 50,740 cards: every card 27 ms (50,740
IDs, 1.98 MB of JSON), every note 75 ms (37,000 IDs, 1.44 MB), `tag:*` in cards mode 79 ms (25,539
IDs). Not measured: the same list through the bridge to the webview (ADR 0014 "Not verified").

### Verified

- `cargo xtask check` passes (Rust tests, clippy, the wasm build, bindings, `tsc`, Vitest).
- The CLI commands on test collections (CLI tests).

### Not verified

- The phone: nothing was built for it (Anthony is away, STATUS). 3.2a has no UI; the phone runs it
  with the next debug APK (the app starts and opens the real collection as before).
- The web client and the desktop app: nothing calls the new methods until 3.2b.
- Timings in wasm and on the phone, and on a disk rather than `/tmp`.
- What 10,000 `noteDeleted` events do to the webview (3.2c, with the selection bar).
- Windows, Firefox, Safari.

### Deviations from the plan

- **`updateNote` takes an optional `deckId`** and one undo covers the edit and the move. Decision 7
  had no deck on `updateNote` and decision 1 a separate `moveCards`, whose token would replace the
  edit's, so Undo after "Saved." would only have moved the cards back. Proposed in the chat plan;
  Anthony said "go" without answering, so the proposal was taken.
- **The kept change lives on `fc_core::Core`**, not in `fc-api`: `Core` is `fc-core`'s type and
  `fc-api` has no state of its own. `fc-api` still builds the changes, the messages and the undo.
  (Chat plan, as above.)
- **No `delete-notes` or `restore-notes` commands.** `delete-note` already took several IDs; it and
  `restore-note` now take several and use the bulk operations. (Chat plan, as above.)
- **Undo of a move from several decks, or of several tags, is one write per deck or tag**, not one
  write. (Chat plan, as above; the alternative was two more public core operations.)
- **New core reads not in the plan:** `search_ids`, `cards_of_notes`, `notes_of_cards`, and
  `fc search --ids`.
- `add_tags` and `remove_tags` answer `(note, tag)` pairs, not "the notes that changed", so undoing
  an add of two tags does not take off one a note already had.
- `deleteNotes`'s output type is `DeletedNotes`, not `DeleteNotesOutput`: the binding check compares
  imports in generated order and Biome sorts them ignoring case (as in 3.1a).
- The CLI was in the core commit, not its own.
- `searchIds` was timed at about 50,000 cards on a separate `fc fake` collection of 37,000 notes
  (made in the background), since `fc fake` takes about 75 s for 10,000 notes and grows with the
  square.

## Build notes (step 3.2b, the note editor)

Branch `step/3.2b-note-editor`. No migration, no core change, no new dependency, no bridge or CSP
change. One `fc-api` field.

- **`noteId` on `nextCard`'s card** (`fc-api` `study.rs`, through `notes_of_cards`). Study only knew
  the card it shows, and no method maps a card to its note.
- **The layer: `Dialog variant="layer"`** (`components/Dialog.tsx`, styles in `shell/shell.css`
  next to the sheet, since only that file uses the safe-area insets). Under 600 px it fills the
  screen less the keyboard inset; from 600 px it is a window of at most 46 rem. A Back control and
  the title at the top, the body scrolls, and its own `BottomActionSlot` at the bottom, because a
  modal `dialog` makes the shell's slot inert. Two changes for every `Dialog`: Escape (the native
  `cancel`) and Back call `onClose` and leave closing to the parent, and a dialog the parent keeps
  open after Back pushes its history entry again; a dialog acts only on its own `cancel`.
- **`editor/useFieldEditing.ts`**: the Add screen's active editor, toolbar refresh, caret above the
  keyboard, media cache and `attach`, now shared by Add and the note editor.
- **`editor/NoteEditor.tsx`**: loads `getNote`, `getNoteTypes`, `getDeckList` and `getTags`. The
  type read only; one `FieldEditor` per field, or a `<textarea>` with the field's HTML as text and
  "This field has formatting the editor cannot show." when `roundTrips` fails; tags; the deck
  (`Several decks` until changed); the duplicate warning without the note itself; Delete note
  ("Delete this note and its 2 cards?"). A field counts as changed against what the editor would
  save untouched, so opening and closing is not a change. Save, Ctrl+S or Ctrl+Enter send one
  `updateNote` with only the changed fields, every tag (and a tag still being typed), and `deckId`
  when the deck changed; with nothing changed Save just closes. Back, Escape or the Back control
  with changes ask "Discard your changes?" (Keep editing, Discard; Escape or Back on the question
  keeps editing).
- **`components/UndoNotice.tsx`**: `role="status"`, the message, Undo (once) and Dismiss, no timer.
  After Undo it shows `undoChange`'s message. 3.2c reuses it.
- **Browse**: a row is a button that opens its note (in Cards view, the card's note). After a save
  or a delete, the notice under the heading and the same rows read again (`searchNow` keeps as many
  rows as were loaded), and again after Undo. Nothing changed: no notice.
- **Study**: Edit (with `E`) next to Undo, and `e`. The answer's time stops while the editor is
  open (`pause`, `resume` in `useStudy`). After a save the card is rendered again on the side it was
  on, or the next card comes when the save removed it, the note was deleted, or the card can no
  longer be rendered. The notice sits under the counts and goes when the card is answered. Undo from
  the end screen (a delete of the last card undone) starts a new session.
- **Keys under a modal**: the shell's shortcuts, `/` in Browse and the study keys do nothing while a
  `dialog` is open (`isModalOpen`), so `1` in the editor does not rate the card behind it.
- **`FieldEditor` leaves Escape to the page.** ProseMirror's view prevents Escape's default, so the
  window around it never got it (found in headless Brave).
- **Tests**: 18 for `NoteEditor` (loading, Several decks, the HTML box shown as text, no
  `innerHTML` in the editor's files, only changed fields sent, the deck in the same call, a typed
  tag, a no-op Save, Ctrl+S and Ctrl+Enter, a refused save, Escape and Back with and without
  changes, a bubbled `cancel`, the duplicate warning, Delete), 3 for Browse (a row opens its note,
  Saved. and Undo with the rows read again, Cards view, closing writes nothing), 6 for Study (Edit
  and `e`, keys off under the editor, the timer, re-render on the same side and Undo, the next card
  when removed, the notice going on an answer), 1 for `FieldEditor` (Escape not prevented, Enter
  still is), and the `nextCard` `fc-api` test checks `noteId`. `editor/testEditor.tsx` holds a
  text area in place of `FieldEditor` and the fake note methods for screen tests.

### Verified

- `cargo xtask check` passes (433 UI tests).
- **Web** (`bun run web:dev`, headless Brave over CDP, real mouse and key events, sample cards from
  the Developer screen, screenshots read): at 390 px light and dark the layer fills the screen
  (390 × 844) with Save and the toolbar in its bar at the bottom; typing, Escape asks, Keep
  editing, history Back asks, Ctrl+S saves and closes with no dialog history entry left, "Saved."
  with Undo, Undo answers "The note is back as it was.", Escape with nothing changed closes. At
  1100 px light and dark: a window at most 46 rem, the Back control, a deck change saved with
  Ctrl+Enter (the row then shows the new deck). Study: Space, `e` opens the editor, `1` does not
  rate, a save keeps the answer side and the counts, the notice shows at 1100 and 390 px dark.
- **Desktop** (`bun run dev`, Linux X11, scratch `XDG_DATA_HOME`, sample cards, `xdotool`,
  screenshots read): the window in WebKitGTK (found collapsed, fixed, checked again); Escape asks,
  Escape on the question keeps editing (found asking again, fixed, checked again); Ctrl+S saves and
  the row changes; Undo; in Study `e` on the answer side, `1` ignored, Ctrl+Enter saves and the
  card shows the new back.

### Not verified

- **The phone (skipped while Anthony is away, STATUS):** the layer with the keyboard open (the
  field and the toolbar visible above it), Android back with unsaved changes (asks, then Keep
  editing), a picture added while editing, the Edit button in Study.
- A field that does not round-trip in a real browser (covered by Vitest only; the sample cards have
  none).
- Windows, Firefox, Safari.

### Deviations from the plan

- **`noteId` on `nextCard`**: an `fc-api` change the plan's 3.2b section did not list (proposed in
  the chat plan and approved with "go").
- **Changes to every `Dialog`**, not only the new variant: Escape and Back go through `onClose` and
  the parent can keep the window open; a bubbled `cancel` is ignored (a WebKitGTK difference found on
  desktop).
- **Screen keys are off while any modal window is open** (`isModalOpen` in `useShortcuts.ts`), for
  the shell's shortcuts too, not only Study's.
- **`FieldEditor` leaves Escape to the page**, a change to 2.4's editor found while checking the
  web.
- **The Study notice goes when the card is answered**; ADR decision 4 says "until the next change,
  leaving the screen, or Dismiss". In Study it would otherwise stay through the whole session. Undo
  of that change stays possible until the next change, from Browse.
- **Nothing changed: Save closes without a call** and no notice is shown.
- **An edit that moves the studied card to another deck keeps it on screen** (it can still be
  answered); the ADR only covers a removed card. Stated in the chat plan.
- `components/UndoNotice.tsx` and `editor/testEditor.tsx` are new files the plan did not name;
  `DecksScreen`'s own notice is unchanged.
- "Change note type" in the editor is not there yet: it comes with 3.2e.

## Build notes (step 3.2c, multi-select and the simple bulk actions)

Branch `step/3.2c-bulk-select`. No migration, no sync format change, no new dependency, no bridge,
permission or CSP change. One `fc-api` method and one core read.

- **`describeTarget`** (`fc-api` `edit.rs`): a target's `notes`, `cards` (what a card action
  reaches), `suspended` (of those cards) and `noteCards` (every live card of the notes, what a delete
  takes). The bar needs it for "12 notes (18 cards)", for Unsuspend when every card is suspended,
  and for the delete question's counts when the selection is "all 5,000". Core gains
  `Collection::suspended_among(cards)` (a reason row in `tests/coverage.rs`), and the shared read
  behind suspend and bury now skips repeats with a set instead of a scan, which was quadratic in the
  selection.
- **`shell/useBackEntry.ts`**: each window, layer or mode that Back closes holds its own history
  entry (`{ fcDialog: depth }`), so Back closes only the top one (a Move sheet over selection mode).
  A holder that stays open gets its entry back; closing by a button takes it back, with one
  `history.go` at a time; an entry left behind by a route change is skipped. `Dialog` uses it in
  place of its own history code (it used to share one entry between windows opened over each other).
- **`browse/useSelection.ts`**: a set of IDs (notes or cards, as the view), the phone's selection
  mode, Shift ranges over the loaded rows, cleared when the query or the view changes (kept on a
  sort change), and `useTargetSummary` (`describeTarget` 150 ms after the selection rests, the latest
  answer winning, asked again after each change).
- **`browse/useLongPress.ts`**: 500 ms, cancelled by 10 px of movement, lifting or a pointer cancel
  (a scroll); the click that ends a long press is swallowed. Under 600 px rows also have
  `user-select: none`, and `selectstart` and `contextmenu` cancelled.
- **`browse/SelectionBar.tsx`**: ✕, the count, "Select all 340" (`searchIds`), and the actions:
  Move, Tag, Suspend or Unsuspend, Delete and More (Remove tags) under 600 px, in `BottomAction` in
  place of the search panel; Move, Add tags, Remove tags, Suspend and Delete from 600 px, above the
  list in place of the search panel. Move is `PickerDialog` with one choice (new props: an action
  label, no Clear, a note line, a find label, busy); Tags is a sheet with Add | Remove and `TagInput`;
  Delete asks "Delete 12 notes and their 18 cards?" and then clears the selection; Suspend acts at
  once. A refused action says why in the bar (`role="alert"`).
- **`BrowseScreen`**: rows are a checkbox and the row's button (`data-checks` on the list: hidden,
  hover or shown). A tap in selection mode toggles; otherwise a row opens the editor, or with Shift
  and a selection chooses a range. Escape clears and Ctrl+A selects every match (a checkbox with
  focus does not count as typing). One notice for edits and bulk actions, under the search panel or
  the bar: in the bottom slot under 600 px, above the list from 600 px. Browse keeps what Undo did,
  so the notice does not offer Undo again when the layout changes.
- **`.button:disabled`** now looks disabled (no style existed).
- **Tests**: 18 in `browse/Selection.test.tsx` (Select and ✕, a tap toggles and opens otherwise, long
  press, a moved finger, the context menu and `selectstart`, Back closing the sheet and then the
  mode, checkboxes, Shift and Escape, Select all and Ctrl+A with `searchIds`, "2 notes (3 cards)",
  Move, Add and Remove tags, Suspend to Unsuspend, Delete with its counts then Undo, Cards view
  targets and counts, a refused action, the notice across a layout change, `useSelection`'s scope and
  range), 5 for `useBackEntry`, 2 more `NoteEditor` assertions (the question's own entry), 1 core
  test (`suspended_among`), 1 `fc-api` test (`describeTarget`) and a row in the no-collection test.

### Measured

Desktop app (`bun run dev`, debug build, Linux X11, WebKitGTK), on a `fc fake` collection of 5,000
notes and 6,810 cards, from the click until the window changed (screenshots polled, so about
±50 ms):

| Action on all 5,000 notes (Ctrl+A) | Time |
|---|---|
| Delete (5,000 `noteDeleted` events reach the webview) | 974 ms |
| Its Undo | 1,250 ms |
| Suspend (6,727 cards changed) | 551 ms |
| Its Undo | 414 ms |
| Move to one deck (6,478 cards moved) | 764 ms |

Ctrl+A itself (`searchIds` of 5,000 notes, then `describeTarget`) showed "5,000 notes (6,810 cards)"
too fast to time this way. Nothing stalled: 5,000 per-note events do not need ADR 0014's summary
event yet.

### Verified

- `cargo xtask check` passes (456 UI tests).
- **Web** (`bun run web:dev`, headless Brave over CDP, real touch, mouse and key events, sample cards,
  screenshots read): at 390 px light, a long press (touch, 700 ms) selects the row with no editor and
  no text selected, a tap toggles another, the bar sits in the bottom slot with 48 px targets, Back
  closes the Move sheet and keeps the selection, Move with its notice and Undo, the selection kept,
  Back leaves selection mode and stays on Browse with no history entry left; at 390 px dark, Select,
  Select all 13, Add tags, Suspend becoming Unsuspend, Unsuspend, More then Remove tags, Delete ("Delete
  13 notes and their 15 cards?"), the mode left, Undo bringing the rows back; at 1100 px light and
  dark, a checkbox replacing the panel with the bar, Shift+click for a range, a row still opening the
  editor with the selection kept, Escape clearing, Ctrl+A, and hover showing one row's checkbox.
- **Desktop** (`bun run dev`, scratch `XDG_DATA_HOME`, `xdotool`, screenshots read): checkbox,
  Shift+click range, Escape from a focused checkbox, Ctrl+A on 5,000 notes, Delete with its question,
  Undo, Suspend to Unsuspend, Move through the picker, and the timings above.
- Found and fixed while checking: Delete's question and selection mode closing together went back
  twice, to the page before the app (`useBackEntry`); the notice offering Undo again after the layout
  changed.

### Not verified

- **The phone (skipped while Anthony is away, STATUS):** long press on a row in the WebView (finding
  3), the selection bar with the keyboard closed and open (the Tags sheet types), Android back leaving
  selection mode and closing a sheet first, a bulk action on a real-sized selection with its time.
- The compact layout in the desktop app (the window manager would not resize the window); a long press
  with a mouse in WebKitGTK.
- Timings in wasm and on the phone, and Ctrl+A over 50,000 notes through the bridge.
- Windows, Firefox, Safari.

### Deviations from the plan

- **`describeTarget` and `suspended_among`**: an `fc-api` method and a core read not in the plan
  (proposed in the chat plan, approved with "go").
- **`useBackEntry` replaces `Dialog`'s history handling**, so every window opened over another now
  has its own entry and Back closes only the top one (chat plan, approved). The `NoteEditor` test that
  checked one shared entry now checks two.
- **The selection is cleared when the query changes**, not only the view (chat plan, approved). A
  sort change keeps it.
- **One notice place**: 3.2b's "Saved." notice moved from under the heading to under the search
  panel, so in the bottom slot under 600 px (chat plan, approved).
- **The bar's count** reads "12 notes (18 cards)" in Notes view (the cards only when they differ),
  "5 cards" in Cards view and "Nothing selected" with none, instead of ADR decision 2's "12 selected".
- **Tags** is one sheet with an Add | Remove switch; under 600 px Tag opens it on Add and More has
  Remove tags (the only More item until 3.2d and 3.2e); from 600 px Add tags and Remove tags are
  separate buttons.
- **Ctrl+A works under 600 px too** (it enters selection mode), not only from 600 px.
- **Shift+click on a row** (not only on a checkbox) chooses a range when something is selected.
- **`.button:disabled`** is a shared style change, outside Browse.
- **`UndoNotice` takes `undone`** and passes Undo's message to `onUndone`, for the layout change.
- A change that changes nothing (Move to the deck the cards are in) shows no notice, as 3.2b does
  for an edit.
- The undo messages from `fc-api` print counts without a thousands separator ("Deleted 5000 notes"),
  unlike the bar ("5,000 notes"); left as 3.2a wrote them.

## Build notes (step 3.2d, set due date and reset)

Branch `step/3.2d-reschedule`. Migration 12. No new dependency, no bridge, permission or CSP change.
The plan was approved in chat with "go", every proposed choice taken.

- **Migration 12**: nullable `source` and `last_day` on `card_event`, then `adopt_unknown` for both
  (its first use outside tests). `docs/backup-format.md` lists the columns and the kinds.
- **The schedule cache can hold a new card with a history.** `Cached.memory` and `Applied.memory`
  are optional; a reset card is a state-0 row with no memory that keeps its answer and lapse totals
  and its last event, so a later event can build on it. The queue and search already read a state-0
  row as new. `CardSchedule.last_day` is `None` for a new card.
- **Reading an older layout.** Reading events, `changes(All)` and the merge use only the
  `card_event` columns the database has (a pragma), and a merge keeps a column the database does
  not have yet as unknown data. In practice every database is migrated before it is read; this keeps
  the tests that model an older build (upgrades from versions 7 to 11, the tour test) honest.
- **`EventKind::SetDue` and `Reset`** (`set_due`, `reset`). Each records its full result and the
  day of the card's last answer (`last_day`), the reset too, so an app that does not know the kind
  ends on the same card. Fold: the recorded result when built on the card's last event; otherwise a
  set due date is recomputed from the card at that point (its state, memory and last answer day, the
  recorded due day) and does nothing on a card that is new at that point, and a reset gives the same
  result. An unknown kind now uses `last_day` when it has one. No cache version bump.
- **`Collection::set_due(cards, from, to)`, `reset_cards(cards)`** (`study/reschedule.rs`): one
  write each, one event per card, the cache updated in the same transaction. New cards are skipped
  and counted (`Rescheduled.skipped_new`); a deleted card refuses the whole call; a range outside
  0 to 36,500 or the wrong way round is `StudyError::DueRange`. The day in a range is
  `scheduling::day_in_range`, from the event ID's last 8 bytes, as the fuzz. **`void_events(ids)`**:
  one write, newest first, refusing (and counting) an event whose card has a later event that is not
  voided, as `undo_answer` does; one already voided is skipped. `new_among(cards)` for Browse.
  `undo_answer` is refused after a later set due date or reset (the existing check); the
  `LaterAnswer` text now says "answered or rescheduled".
- **History and statistics.** `ReviewEntry` gains `kind` (`Answer`, `SetDue`, `Reset`) and its
  `rating` is optional; `days_since_previous` is between answers only. `daily_counts` and
  `retention` already counted answers only (tests pin it). `optimisation_data` drops a card's answers
  before its last reset that is not voided.
- **`fc-api`**: `setDueDate` (`target`, `fromDays`, `toDays`; answers `count`, `skippedNew`,
  `undo`), `resetCards` (`count`, `undo`), `Reverse::Events` (undo voids the events: "Put back the
  schedule of 4 cards.", with how many could not be because they were studied since), and
  `newCards` on `describeTarget`.
- **CLI**: `fc set-due <file> <days | from-to> <card ID>...`, `fc reset <file> <card ID>...`;
  `fc schedule` and `fc history` show the two kinds, and `fc schedule` shows the counts of a reset
  card. `void_events` and `new_among` have reason rows in `tests/coverage.rs`.
- **UI**: `browse/DueSheet.tsx` (Today, Tomorrow, In N days, Between N and M days; typing a number
  chooses its option; "11 new cards are not changed"; the sheet is the confirmation) and the Reset
  question in `SelectionBar` ("Reset 4 cards to new?", the history kept, how many are new already).
  Both are in More, under 600 px and from 600 px (see deviations).
- **Tests**: 23 core tests (`study/reschedule_tests.rs`): the result and the event of a set due
  date, a learning card, new cards skipped, the range spread and that it repeats, bad ranges and a
  deleted card writing nothing, the next answer's elapsed days, reset and studying it as new, reset
  skipping new cards, undo of both, undo refused for a card answered since, `undo_answer` refused,
  another device's answer before and after a set due date, after a reset, a set due date on a card
  that is new when folded, an unknown kind with the same columns giving the same card, a merge into
  another collection and a fresh one, history, daily counts and retention, the optimiser after a
  reset and an undone one, the version-11 upgrade adopting a kept `last_day`. A mutation check (the
  fold always taking a set due date's recorded result) fails 2 of them. 3 `fc-api` tests, 1 CLI test,
  4 Vitest tests in `Selection.test.tsx` and 2 for `dueRange`. Existing tests changed where the
  schema version or the history's `rating` moved, and one fold test used `set_due` as its example of
  an unknown kind.

### Verified

- `cargo xtask check` passes.
- **Web** (`bun run web:dev`, headless Brave over CDP, real touch, mouse and key events, sample
  cards with four answered Easy in Study, screenshots read): at 390 px light, Select all, More (Set
  due date, Reset to new, Remove tags), the sheet with "On 15 cards" and "11 new cards are not
  changed", 48 px options, In 5 days giving "Made 4 cards due in 5 days." and the rows "Due in 5
  days", the selection kept, Undo ("Put back the schedule of 4 cards.") and the rows back; at 390 px
  dark, Between 9 and 7 refused with its line, Back closing the sheet and keeping selection mode,
  "Reset 4 cards to new?" with "11 cards are new already", the rows reading New, Undo; at 1100 px
  light and dark, the row Move, Add tags, Remove tags, Suspend, Delete, More on one line, More with
  Set due date and Reset to new, Today giving "Due today"; at 700 px nothing overflows.
- **Desktop** (`bun run dev`, Linux X11, WebKitGTK, scratch `XDG_DATA_HOME`, sample cards with four
  answered Easy in Study, `xdotool`, screenshots read): the Developer screen shows storage version
  12; Ctrl+A, More (Set due date, Reset to new), Between 2 and 6 typed and sent with Enter ("Made 4
  cards due in 2 to 6 days.", rows due in 6, 6, 4 and 6 days), Undo putting back 9, 7, 7 and 7,
  Escape on the Reset question keeping the selection, Reset ("Reset 4 cards to new.", every row New)
  and its Undo. Found and fixed: WebKitGTK drew the radios in its own colour; the options now use
  the shared `.choice` style (the app's accent).
- **More actions** (Anthony, in chat: its buttons were too close together; Cancel touched the last
  action): 0.75 rem between the actions and 1.5 rem above Cancel, checked at 1100 px light and
  390 px dark in headless Brave. It also spaces 3.2c's More, which only held Remove tags.

### Not verified

- **The phone (skipped while Anthony is away, STATUS):** the Set due date sheet with the keyboard
  open (the day boxes and Set due date visible above it), Android back closing the sheet and then
  More, the Reset question, a set due date on a real-sized selection with its time. The phone also
  migrates to storage version 12 with the next debug APK.
- Timings: nothing measured for a set due date or reset on thousands of cards. Each is one write
  that reads one cached row and one event per card, like `answer` without the scheduler.
- **Older builds after sync** (none exist yet; Phase 4): an app before this one applies a set due
  date (it has every column that build reads), counts the next answer's days from the event's own
  day because it does not know `last_day`, and ignores a reset, because that build only applies an
  unknown kind that has a memory. Per-entity `requires` (before 4.3) covers this.
- Windows, Firefox, Safari.

### Deviations from the plan

- **Set due date and Reset to new are in More from 600 px too**, not in the row: with them the row
  did not fit at 1100 px and Delete wrapped onto a line of its own (seen in headless Brave), and 3.2e
  adds Change note type. ADR decision 3 said "From 600 px they fit in one row"; the chat plan said two
  more buttons in the row.
- **Reading an older layout**: `card_event` reads, `changes(All)` and the merge now check which
  columns the database has, so the tests that build an older version's collection keep working. Not
  in the plan.
- **A reset records `last_day`** (the card's last answer day), not "no last answer day" as decision
  5's table says, so an app that does not know the kind ends on the same card (found by the
  unknown-kind test). The cache keeps that day and `card_schedule` still reads `None` for a new card.
- **The unknown-kind fold uses `last_day`** when an event has it.
- **The Set due date sheet starts on Tomorrow**, with 7 for In N days and 3 to 7 for Between (the
  plan did not say).
- **`fc schedule` shows the counts of a reset card**, and **`fc history` counts only answers**
  in its "N reviews" line. Not in the plan.
- **Accepted ADRs**: 0014 and 0007 were read in full, and the relevant parts of 0006 and 0008; the
  rest only through 0014's Context and the earlier build notes (said in the chat plan).
- One more existing test changed than expected: the fold test that used `set_due` as the name of an
  unknown kind now uses `move_due`.

## Build notes (step 3.2e, change note type)

Branch `step/3.2e-change-notetype`. No migration (3.2d added `source` and `last_day`), no new
dependency, no bridge, permission or CSP change. The plan was approved in chat with "go", every
proposed choice taken.

- **`EventKind::Transfer`** (`transfer`, `study/transfer.rs`): records the source card's full result
  (state, step, memory, due) and its day of last answer in `last_day`, with `source` naming the card.
  The fold always applies the recorded result, even when the event did not build on the card's last
  event. An older build reads it as an unknown kind and applies the same result when it builds on the
  last event (tested). It is never earlier than the target's last event or the source's last event,
  so history reads in order.
- **`Collection::change_note_type(notes, to, fields, templates)`** (`note/change_type.rs`), one
  write: values mapped onto the new field IDs (only changed ones written), the `note_type` register,
  `reconcile`, one `NoteEdited` per note, then a `Transfer` per new or returning card with a source
  card, and the source's deck and suspension. Old values and old cards stay stored. Cloze to cloze
  carries by cloze number; between kinds nothing is carried and `templates` is ignored.
  `note_types_among` tells `describeTarget` whether a selection shares a type.
- **Maps.** A field listed with no source is emptied; one not listed keeps what it has stored. A
  template listed with no source is "start as new" (a card that comes back from an earlier life is
  reset with a `Reset` event); one not listed is left as it is. Undo uses "not listed", so the old
  values and cards come back as they were.
- **Refused, nothing written:** notes of several types (`MixedTypes`, "Select notes of one note type
  to change it."), the same type (`SameType`), a note that would be left with no cards
  (`LeavesNoCards`), one old card for two new ones (`DuplicateSource`), a field or template of
  another note type (`UnknownField`).
- **A transfer is skipped when the target already has that schedule** (a change back with nothing
  studied in between), so a round trip adds no events.
- **History and the optimiser** follow `source` (`events_with_sources`, a chain, each card once).
  `card_history` gains `HistoryKind::Transfer`; `optimisation_data` reads the few chained cards one by
  one, so an old card in the trash is counted once, through the live card that replaced it.
- **`fc-api`:** `changeNoteType` (`target`, `noteTypeId`, `fields`, `templates`; answers `notes`,
  `carried`, `undo`); `Reverse::NoteType` (undo is a change back with the reversed maps, for the
  notes still of the new type; "Changed 12 notes back to Basic."); `getNoteTypes` also lists
  templates; `describeTarget` also gives `noteTypeId` (`null` for a mixed selection).
- **CLI:** `fc change-notetype <file> <type> [--field New=Old]... [--card New=Old]... <note ID>...`
  (parts not given are matched by name, then position, each old part once); `fc schedule` and
  `fc history` show a transfer.
- **UI:** `browse/ChangeTypeWindow.tsx` (a `Dialog variant="layer"`: the type, one select per new
  field and per new card, the card choices exclusive, a sentence saying what it will do),
  `browse/changeType.ts` (defaults and the sentence, tested). "Change note type" is in More in the
  selection bar at both widths. The editor has a Change button next to the type, off while there are
  unsaved edits; after a change the editor closes and the screen shows the notice with Undo. Study
  moves on to the next card, since the card is replaced. In Cards view a type change clears the
  selection (the cards are replaced); in Notes view it stays.
- **Tests:** 22 core tests (`note/change_type_tests.rs`) and 2 (`study/transfer_tests.rs`): the
  schedule, deck and suspension carried, field maps, refusals writing nothing (digest), a change back
  with and without study in between, start as new, cloze, merge into another and a fresh collection,
  two devices changing at once, history, the optimiser across a chain and a reset, the fold with a
  concurrent answer (a mutation check, the fold taking the result only when built on, fails it), an
  older build reading the kind. 5 `fc-api` tests, 1 CLI test, 22 Vitest tests (window, maps, selection
  bar, editor, Study).

### Verified

- `cargo xtask check` passes.
- **Web** (`bun run web:dev`'s vite, headless Brave over CDP, real clicks and keys, screenshots
  read): at 390 px light and dark and 1100 px light and dark, three notes with two answered Easy:
  More, the window with its defaults and sentence, Change ("Changed 3 notes to Basic and reversed.",
  "3 notes (6 cards)"), Undo ("Changed 3 notes back to Basic."), the editor's Change button, Cloze
  chosen with its sentence, and the core's refusal shown in the window.
- **Desktop** (`bun run dev`, scratch `XDG_DATA_HOME`, a collection made with `fc`, `xdotool`):
  Ctrl+A, More, the window, Change, and Undo; read back with `fc`: the new Card 1 is a review card
  with the old card's due day and memory and a `transfer` event, Card 2 is new, and after Undo the
  old card is live again with its one answer and no extra event.

### Not verified

- **The phone, partly.** Anthony stopped the checks (usage). Seen with `adb` taps and the UI dump:
  the debug APK installed, More, the window opened with its defaults, Android back closed it and kept
  the selection, Change gave "12 notes (24 cards)", Undo gave "Changed 12 notes back to Basic.".
  Screenshots came out black, so the layout was not seen. Not done: the editor's Change button, a
  longer list of fields.
- Timings: nothing measured for `change_note_type` on thousands of notes (one write; per note two
  reads and a reconcile, as `restore_notes`).
- The editor's Change button on the desktop app, Windows, Firefox, Safari.

### Deviations from the plan

- **None in what was built.** The seven choices listed at the plan (same type refused; a note left with
  no cards refused; one old card to one new card; listed and unlisted map entries; a transfer skipped
  when the schedule is the same; Change off while the editor has unsaved edits; buried state not
  carried) were taken as proposed, and Anthony said "go".
- **Not done from the plan:** the timing on 10,000 notes (usage), and the phone checks beyond the list
  above (Anthony stopped them).
- **Small additions not in the plan:** `note_types_among`, `HistoryKind::Transfer`, `templates` in
  `getNoteTypes`, `noteTypeId` in `describeTarget`, CLI display of a transfer, and the CLI's default
  matching.

### Seen, not changed

- **Notes view shows one card of a note**, the one with the lowest card ID (`MIN(c.id)` in
  `search/run.rs`). After a change to Basic and reversed a note can read "New" while its Card 1 is a
  review card, and another reads "Due in 9 days". It was always so for Basic and reversed notes (3.1).

## Build notes (step 3.3, card info)

Read only, no ADR of its own (no new data, no schema change). ADR 0014 decision 6 already says
history follows the source card, so card info and training see one history.

- **Core:** `Collection::card_detail` (note, note type, template, cloze number, deck, suspended,
  buried; `NotFound` for a deleted card or note) beside `card_history`, and `fc_core::readable_text`
  so the API can hand the page plain text.
- **API:** `getCardInfo { cardId }` answers the schedule (state, step, due as in `search`, FSRS
  memory, days since the last answer, answers, lapses, suspended, buried, deck path, template), the
  note (type, each field as plain text, tags, card count) and the history, oldest first, each entry
  with its event ID, kind (answer, set due, reset, transfer), time and UTC offset, rating, state
  before and after, days since the previous answer, when it came back and how long it took.
  Answers and lapses are counted from the history, not the cached counts, which start at 0 for a card
  made by a note type change (the gap noted under decision 6).
- **UI:** `CardInfoWindow` (`packages/ui/src/card/`), a `layer` dialog: full screen on a phone, a wide
  window from 600 px. Fields are shown as text, never as HTML. Opened from Study (Info, key `i`; the
  answer's time stops while it is open, as for Edit) and from Browse's More sheet when exactly one row
  is selected.

### Verified

- `cargo xtask check` passes: core tests for `card_detail`, `getCardInfo` tests through `dispatch` (new
  card, answered card with a lapse, suspended, history across a note type change, deleted and bad
  IDs), and UI tests for the window (schedule, history order, text not markup, empty card, failure and
  Try again, Close and Back), the Info button and `i` in Study, and Card info in More (on for one
  row, off for several, selection kept on close).
- **Web** (`bun run web:dev`, headless Brave over CDP, real clicks, screenshots read): at 390 px a new
  card from Browse's More sheet (no horizontal scroll, Close leaves the selection), and at 1100 px an
  answered card (learning, stability, difficulty, last answered, the Good answer with "New to learning
  · back in 10 minutes · took 1 second"). Study's Info button was clicked in the same run.

### Not verified

- **The phone and the desktop app.** No phone was connected, and the desktop app was not run. Not seen
  on a phone: the layer's height with the browser bars, Android back closing it (the same `Dialog` as
  the note editor, which was checked in 3.2).
- Dark theme, and a card with a long history (hundreds of answers): no scroll or speed check.
- Windows, Firefox, Safari.

### Deviations from the plan

- **Entry point in Browse:** Card info is in the More sheet when one row is selected, not an Info
  action on the row. A tap on a row opens the editor (3.2b) and the row has no room for a second
  control on a phone, so this is two taps after selecting. Easy to move if you want it elsewhere.
- **No created or modified time** for the note: the core stores none (ADR 0006 keeps clocks, not
  dates), so "note details" is type, fields, tags, deck and card count.
- **Added beyond the plan:** the `i` key in Study, an `id` on each history entry (a stable key for the
  list), `fc_core::readable_text`, an `fc-cli` coverage entry for `card_detail`, and the helpers of
  `edit.rs` and `edit_tests.rs` made `pub(crate)` so `card_info` and its tests reuse them.
- **Not done from the plan:** the long-history check (risks list).
- Size and commit structure as planned: one PR, three commits (core and API, UI, docs).
