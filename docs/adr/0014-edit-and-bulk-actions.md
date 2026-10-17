# 0014: Editing notes, multi-select and bulk actions

Status: Proposed
Date: 2026-10-17

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
