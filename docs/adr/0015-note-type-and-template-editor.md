# 0015: Note type and template editor

Status: Accepted
Date: 2026-10-22
Accepted: 2026-10-22 (every question as proposed, see Decisions on review)

## Context

Step 3.4 (`docs/phases/03-browse-edit-capture.md`) has these acceptance criteria:

- Manage fields and templates on both platforms.
- Template editor with live preview using real notes.
- Works on a phone, even if desktop is more comfortable.

`PRODUCT.md` lists note type editing and template editing among the features that must work on
mobile "from the start". Templates support HTML and CSS, field substitution, conditionals and
cloze, and card content is untrusted.

The brief says "Sonnet build", but no accepted ADR decides:

- where the editor lives;
- whether changes are saved one at a time or together;
- what kind of text box edits a template;
- how a live preview renders a template that is not saved yet;
- how the preview fits on a phone.

What exists and constrains this:

- **The core already has every operation** (ADR 0006 build notes 1.2 to 1.4, CLI in 1.14b):
  `create_note_type`, `rename_note_type`, `set_note_type_css`, `set_sort_field`,
  `delete_note_type` and `restore_note_type`; for fields and templates `add`, `rename`, `move`,
  `remove`, `restore` and `removed_*`; `set_template_text`. The rules it checks are listed in the
  1.2 build notes (at least one live field and template, a cloze type has exactly one template, names
  unique ignoring case, `kind` fixed after creation). Each is **its own write, and every one that
  touches templates or fields reconciles every note of the type** in that write.
- **Reconcile is the slow part.** Adding a template to Basic took 0.42 s at 6,020 notes and 3.5 s at
  50,001 notes (native, release, ADR 0006 build notes 1.3 and 1.14a). Not measured on the phone or
  the web.
- **What reconcile does to cards.** A template whose front shows no filled field makes no card for
  that note. So editing a front can send many cards to the trash, with their history, and editing it
  back restores the same cards (same IDs, ADR 0006 section 1). Removing a template does the same.
  Removing a field hides its values. Nothing is ever lost, but a careless edit can empty a deck.
- **Renaming a field rewrites `{{OldName}}`** in every template of the type (1.3 and 1.4 build
  notes), so cards do not disappear on a rename.
- **Saving a template with a syntax mistake is refused** (`NoteTypeError::Template`). Unknown field
  names are not mistakes (`template::unknown_fields` lists them for a warning).
- **Rendering** (`render_card`) only renders a stored card with its stored template. The renderer
  itself (`template::render`) is `pub(crate)` and takes a note type, a template, values and an
  ordinal, so it can render a draft as easily as a stored one.
- **ADR 0005:** card HTML is shown only in `CardFrame`, one fresh frame per card side. A card's
  scripts run on the app's main thread, and a looping script freezes the app. Freeze recovery is not
  built (STATUS, open items).
- **ADR 0010:** routes in the URL hash, destinations Decks, Add, Browse and Settings, compact layout
  under 600 px, `BottomAction` above the keyboard, 48 px targets under 600 px, `Dialog` with
  `variant="sheet"`.
- **ADR 0014:** one level of undo kept in `fc-api` memory (`Reverse` in `fc_core::undo`), the
  notice with Undo, explicit Save with "Discard your changes?" in the note editor.
- **ADR 0009 and `events.md`:** note types have no event yet. A kind is added when a screen needs
  it.
- **Dependencies today** in `packages/ui`: React and ProseMirror (the field editor). No code editor.

## Findings

### Run here (throwaway, in the session scratchpad, outside the repo)

1. **CodeMirror 6 bundle size**, built with `bun build --minify` from `@codemirror/view` 6.43.15,
   `@codemirror/state` 6.7.6, `@codemirror/commands` 6.11.1 and `@codemirror/lang-html` 6.4.12:
   - the bare editor (view and state): 204 KB, **65 KB gzipped**;
   - with the HTML language (which includes CSS and JavaScript highlighting), history and the
     default keymap: 458 KB, **157 KB gzipped**.

   For scale, the whole web app's JavaScript was 511 KB (not gzipped) at step 2.7, before Browse and
   the editor.
2. **CodeMirror 6 edits through `contenteditable`**, not a textarea, with its own Android
   workarounds (10 `browser.android` branches in `@codemirror/view`'s `dist/index.js`). So it does
   not get a plain text box's keyboard behaviour for free.

### Read in source

3. `generate::Plan::new(&NoteType)` and `generate::wanted` decide a note's cards from a note type
   value held in memory. So the cards a draft would add and remove can be counted **without
   writing**: build a `Plan` from the draft and compare it with each note's live cards. The cost is
   the read half of a reconcile.
4. `render.rs`'s `values_by_name` maps a note's values (by field ID) to names through the note
   type it is given. A draft keeps field IDs, so a draft rename shows the note's value under the new
   name in a preview without anything stored changing.
5. The app already turns off keyboard help on code-like inputs with `autoCapitalize`, `autoCorrect`
   and `spellCheck` (`TagInput.tsx`, Browse's search box, the note editor's HTML box).

### Looked up (web, 2026-10-22)

6. Searches on CodeMirror 6 with Android keyboards and on Gboard with `autocorrect="off"` found
   little that was current or authoritative: forum threads about CodeMirror 5 on Android keyboards,
   and a CodeMirror 6 page from its beta saying that touch support was a design goal. Nothing
   confirms either way how CodeMirror 6 behaves with Samsung Keyboard or Gboard today.

### Not verified

- Anything on the phone: typing templates with Samsung Keyboard or Gboard, whether `autocorrect`
  is honoured, CodeMirror on the phone, the preview's speed.
- Save and the card count dry run at 50,000 notes on the phone or the web (only the native figures
  above exist).
- How quickly a fresh card frame per preview update appears on the phone.

## Decision 1: where the editor lives

**Two routes under Settings: `#/notetypes` (the list) and `#/notetypes/<id>` (one note type).**

- Settings gets a "Note types" row. Its destination keeps Settings highlighted, as `options` does
  for Decks.
- The Add screen's note type picker gets "Manage note types", which opens the list.
- **The list:** each note type with its kind and how many notes use it, and **New note type**. New
  asks for a name and "Start from": a copy of any existing note type (the built-ins first). A copy
  gets new IDs for itself, its fields and its templates, and the same names, text and styling. When
  every type has been deleted it still offers "Basic" and "Cloze" shapes (today's
  `create_note_type`).
- **The note type screen** has these sections:
  - Name.
  - **Fields**, in order, with the sort field marked.
  - **Cards**, one row per template (a cloze type has one row and no Add).
  - **Styling**, which opens the template editor on its Styling tab.
  - **Removed fields** and **Removed cards** (only when there are any), each with Restore.
  - Delete note type.
- A route, not a layer: nothing underneath needs keeping, unlike Browse in ADR 0014. Back and
  Android back work by themselves, and a later step can link to a note type.

### Options considered

| Option | For | Against |
|---|---|---|
| **Routes under Settings (chosen)** | Back works. Room for a long screen. Reached from Settings and from Add | One more route pair |
| A layer over Add or Browse | Edit a note type without leaving a note | A note type is not something you change mid-note. A layer inside a layer for the template editor |
| A fifth destination | One tap away | Rarely used. ADR 0010 keeps four destinations in a phone's bottom bar |

## Decision 2: edit a draft, save once

**The note type screen and the template editor edit one draft of the whole note type. Save writes
all of it in one write, with one reconcile and one undo.** Back or Escape with changes asks "Discard
your changes?", as the note editor does (ADR 0014).

- **The draft** is the note type as `getNoteTypeDetails` returns it: the name, styling, sort field,
  fields (ID, name) and templates (ID, name, front, back), in order. New fields and templates have
  no ID until saved. A removed item is left out of the draft. Restoring one puts it back with its
  ID.
- **Renaming a field in the draft** rewrites `{{OldName}}` in the draft's templates at once, through
  the core's own rename (a pure method, `renameFieldInTemplates`). So the template text the person
  sees is what will be saved, and the template editor never shows a stale name. Save does not
  rewrite anything itself.
- **Before Save** the UI checks the draft locally: names not empty and unique (ignoring case), at
  least one field and one card, no template with a syntax mistake (from the preview call, decision
  4). Save is disabled with the reason shown next to the problem.
- **Save asks the core what it would do** (`checkNoteType`, a dry run, finding 3): how many cards it
  makes and how many it sends to the trash.
  - If **cards would go to the trash**, Save asks: "This sends 34 cards from 34 notes to the trash.
    Their history is kept, and Undo or changing the template back brings them back." (Save anyway,
    Keep editing).
  - Otherwise it saves without asking. New cards are reported afterwards: "Saved. 120 new cards."
- **One write** (`Collection::save_note_type`). It writes only what changed:
  - names, styling, sort field and template text that differ;
  - added, removed and restored fields and templates;
  - **a position only for an item whose order changed.** The items that keep their relative order
    (the longest run in the old order) keep their positions. Others get one between their new
    neighbours. So two devices that reorder different items still both keep their change (ADR 0006
    build notes 1.2).

  Then it reconciles the type's notes once. The core's existing rules still apply inside it, and a
  failure writes nothing.
- **Undo** (ADR 0014 decision 4): the notice says "Saved Polish words." with Undo. A new
  `Reverse::NoteType` keeps the old values of every register the save changed. Undo writes them back
  and reconciles, so cards sent to the trash come back with their history. Creating a note type is
  undone by deleting it, and deleting one by restoring it.
- **Delete note type** asks first, with the counts ("Delete Polish words, its 1,240 notes and 1,800
  cards?"), and offers Undo after, as Browse's delete does. There is no trash screen.

### Options considered

| Option | For | Against |
|---|---|---|
| **A draft, one Save (chosen)** | One reconcile per save, not one per step. Intermediate states (a field renamed, a template half-changed) never reach notes, cards or sync. One undo for the whole change. The dry run can warn before cards leave | A new core write that applies a difference, and a position rule to keep merges working |
| Each change saved at once (today's core methods, one call each) | No new core write. Nothing to lose | Every rename, reorder or template edit reconciles every note (seconds each at 50,000 notes). A template edited in steps sends cards to the trash and back. Nowhere to warn before cards go. Undo is per step |
| Immediate for fields, a draft for template text only | Template text needs a draft anyway (syntax mistakes are refused) | Two saving models on one screen. The warning still cannot cover a field removal |

## Decision 3: the template editor

**A full-screen layer over the note type screen**, opened from a Cards row or Styling, with three
tabs: **Front**, **Back** and **Styling**. Its changes go into the draft. **Done** returns to the
note type screen, and nothing is written until that screen's Save.

**The text box is a plain `<textarea>`**:

- a monospace font, `wrap="off"` with horizontal scrolling on desktop and wrapping under 600 px;
- `autocapitalize="off"`, `autocorrect="off"`, `spellcheck="false"`, as on the app's other
  code-like inputs (finding 5);
- Tab keeps its normal meaning (moving focus), so keyboard users are never trapped. Indenting is by
  spaces.

**An Insert row** above the box (in `BottomAction` under 600 px, so it sits above the keyboard)
inserts at the cursor:

- each field as `{{Name}}`;
- `{{FrontSide}}` on the back;
- `{{cloze:Name}}` on a cloze type;
- **Show if…**, which wraps the selection in `{{#Name}}…{{/Name}}` for a chosen field.

**Mistakes** from the core are listed under the box: line, column and the core's sentence. Tapping
one moves the cursor there. Unknown field names are a warning ("This template uses `Meaning`,
which is not a field. It will show nothing."), not a block.

### Options considered

| Option | For | Against |
|---|---|---|
| **A plain textarea with an Insert row (chosen)** | The phone's own text box: selection, keyboards and accessibility work as everywhere else. No dependency. The Insert row does the typing that is hardest on a phone | No highlighting, no matching brackets, no completion |
| CodeMirror 6 with the HTML language | Highlighting, completion and folding. Works with a keyboard and mouse well | +157 KB gzipped (finding 1). `contenteditable` with its own Android code paths (finding 2), unverified with Samsung Keyboard and Gboard, which is where ADR 0011's ProseMirror needed care. A second editor library next to ProseMirror |
| CodeMirror 6 without a language (+65 KB gzipped) | Smaller. Line numbers | Most of the cost and risk of the full one, little of the gain |
| ProseMirror in code mode | Already a dependency | Built for rich text, not code. A schema and plugins to write for no clear gain |

## Decision 4: the live preview

**The preview renders the draft, not what is stored, with a real note of the type, in `CardFrame`.**

- **The core** gains `preview_card(draft, template_index, note, ordinal)`. It renders exactly as
  `render_card` does (the same `template::render`, styling, media list and sound), but with the
  draft's templates and styling and the field names from the draft (finding 4). It answers the
  front, back and media, or the template's mistakes. Nothing is written.
- **Which note:** "Preview with" shows the note's sort field. Previous and Next step through the 50
  notes of the type added most recently (`previewNotes`). For a cloze type, a number picker lists
  the cloze numbers of that note.
  - **No notes yet:** each field shows its own name in brackets, "(Front)", and the preview says
    "Example text, because this note type has no notes yet."
  - **The note makes no card** from this template with the draft (its front shows nothing): the
    preview says "This note makes no card with this template." and shows the front as it would
    render.
- **Front and Back** switch above the frame. The tab being edited chooses the side by default
  (Front for Front, Back for Back and Styling).
- **Updates while typing**, after a pause of 400 ms. While the draft has a mistake, the last good
  preview stays, dimmed, with "Fix the mistake to update the preview." Each update is a fresh frame
  (ADR 0005), shown once it has loaded, so the old one stays until then and the preview does not
  flash.
- **Theme:** the frame gets the app's theme, as in Study.

**The layout:**

- **840 px and over:** two columns, the text box on the left and the preview on the right, both
  visible while typing.
- **Under 840 px:** one column. An **Edit / Preview** switch sits in the header, in thumb reach in
  `BottomAction` while the keyboard is closed. Preview closes the keyboard and shows the frame with
  Front and Back. Edit goes back to the text box with the cursor where it was. The preview is always
  the current draft, with no save needed.

**Scripts in templates run in the preview**, as they do in Study, because the preview must show the
card as it will be studied. A script typed into a template that loops forever freezes the app, as it
would in Study (ADR 0005, Consequences). The 400 ms pause and "only when the template parses" make
that less likely, not impossible. Freeze recovery stays the open item it is in STATUS.

### Options considered

| Option | For | Against |
|---|---|---|
| **Render the draft in the core (chosen)** | The same renderer as Study, so the preview is exact. Mistakes come from the same parser as Save | A new core read method that takes a whole draft |
| Save first, then render the stored card | No new method | Every preview is a write and a reconcile. A half-written template sends cards to the trash and syncs |
| Render in TypeScript | No bridge call per update | A second template engine to keep in step with the core. Exactness is the point of a preview |

| Phone layout | For | Against |
|---|---|---|
| **Edit / Preview switch (chosen)** | Each gets the whole screen. Simple. The keyboard never covers the preview | Not side by side: one tap to look |
| Preview stacked above the text box | Both at once with the keyboard closed | With the keyboard open, about 380 px of height is left on Anthony's phone, too little for both. It would have to collapse, which is the switch with more moving parts |

## Decision 5: fields and cards on the phone

- **A row per field or card**: its name and, for a field, "Sort by" when it is the sort field.
  Tapping a row opens a sheet (`Dialog variant="sheet"`) with **Rename**, **Move up**, **Move
  down**, **Use to sort** (fields), **Edit template** (cards) and **Remove**. From 600 px the same
  actions are buttons in the row.
- **No drag to reorder.** Move up and Move down work the same with a finger, a mouse and a keyboard,
  and need no gesture code. A list of fields is short.
- **Add field** and **Add card** sit at the end of each list. A new card starts as a copy of the
  first card's template, named "Card 2", "Card 3"… so it shows something at once in the preview.
- **Remove** says what happens, without asking:
  - a field: "Its text stays in your notes, hidden, and comes back if you restore the field";
  - a card: "Its cards go to the trash when you save" (the save then asks, decision 2).
- **Under 600 px** Save sits in `BottomAction`. From 600 px it sits at the top right of the screen
  with Ctrl+S. Ctrl+S works in the template editor too, and saves the whole draft.

## Decision 6: the API

New `fc-api` methods. IDs are UUID strings, counts `u32` (ADR 0002).

| Method | Input | Output |
|---|---|---|
| `getNoteTypeDetails` | `noteTypeId` | the draft shape (decision 2), `kind`, `removedFields`, `removedTemplates`, `noteCount`, and per template `cardCount` |
| `getNoteTypes` (changed) | | also `noteCount` per type, for the list |
| `renameFieldInTemplates` | `templates` (front, back), `from`, `to` | the templates, rewritten |
| `previewNotes` | `noteTypeId` | up to 50 notes, newest first: `noteId`, `label` (sort field as plain text), `clozeNumbers` |
| `previewCard` | `draft`, `templateIndex` (in the draft, so a new template works too), `noteId` or null, `ordinal` | `front`, `back`, `media`, `makesCard`, or `problems` (side, line, column, sentence) and `unknownFields` |
| `checkNoteType` | `draft` | `cardsAdded`, `cardsRemoved`, `notesAffected`, or `problems` |
| `saveNoteType` | `draft` | `cardsAdded`, `cardsRemoved`, `undo` |
| `createNoteType` | `name`, `copyOf` (an ID) or `kind` | `noteTypeId`, `undo` |
| `deleteNoteType` | `noteTypeId` | `notes`, `cards`, `undo` |

- `undoChange` (ADR 0014) handles the new `Reverse` kinds.
- Errors as in ADR 0013: a refused change is `invalidInput` with the core's sentence, a note type
  that is gone is `notFound`.
- **Core:** `save_note_type`, `note_type_consequences` (the dry run), `preview_card`,
  `copy_note_type` and a public `template::rename_field`. The CLI gets commands for the new
  `Collection` operations (`tests/coverage.rs` requires them).
- **Events:** none added. `events.md` says note types have no event yet, and no screen in this step
  listens for one.

## Consequences

- **The editor never writes a half-finished note type.** The cost is a new core write that applies
  a difference between the stored type and a draft, with the position rule. That write is the
  riskiest part of the step and gets the most tests (every kind of change, a failure part-way writes
  nothing, a `changes(All)` replay into another collection converges, two collections reordering
  different fields both keep their change).
- **Save can take seconds** on a large type (3.5 s natively at 50,000 notes, slower on the phone).
  The screen shows "Saving…" and stays disabled until it is done. The dry run before it costs about
  the same again, but only when Save is tapped.
- **No syntax highlighting.** Templates are edited as plain text. The Insert row covers the syntax
  people need most. CodeMirror can replace the textarea later without changing anything else.
- **The preview runs card scripts** while a template is being written, so the freeze risk of ADR
  0005 now also applies while editing.
- **A draft is lost if the app is closed or killed** before Save, including by Android when it is
  in the background for long. Nothing is kept across restarts.
- **No new dependency**, no bridge command, no permission or CSP change, nothing that costs money.
  Template text, field names and error sentences are shown as text. Card HTML only ever reaches the
  page through `CardFrame`.
- **No migration.** Every register written exists already.

## Revisit if

- **Typing templates on the phone is painful** in a plain text box (Anthony tries it): CodeMirror 6
  as a measured experiment on the phone first.
- **People lose drafts** to the app being killed: keep the draft in `localStorage` until Save or
  Discard.
- **Save or the dry run are too slow on the phone** at Anthony's size: run the dry run while the
  person is still editing, or reconcile in batches.
- **A looping template freezes the app in practice**: build freeze recovery, and a "Run scripts in
  preview" switch that is off by default.
- **Shared decks (Phase 9)** need note types that one person owns and others only use: the editor
  then becomes read-only for those, with "Make a copy to change it".

## Questions for Anthony

1. **Where:** a "Note types" row in Settings, and "Manage note types" from Add's picker.
2. **A draft and one Save**, with a question only when cards would go to the trash, and Undo after.
   The alternative is saving each change at once, which is slower and cannot warn.
3. **A plain text box with an Insert row** for templates, no highlighting. CodeMirror would add 157
   KB gzipped and is unverified on the phone's keyboards.
4. **On the phone, an Edit / Preview switch** rather than both on screen. Side by side from 840 px.
5. **Scripts run in the preview**, as in Study, accepting the freeze risk until freeze recovery
   exists.
6. **Drafts are not kept across a restart.**
7. **Three PRs** as in the plan (`docs/plans/3.4-notetype-editor.md`).

## Decisions on review

Anthony's answer on 2026-10-22, in chat: "I accept. Approve. Merge". He changed nothing, so every
question is answered as proposed:

1. A "Note types" row in Settings, and "Manage note types" from Add's picker.
2. A draft and one Save, with a question only when cards would go to the trash, and Undo after.
3. A plain text box with an Insert row, no highlighting.
4. On the phone, an Edit / Preview switch. Side by side from 840 px.
5. Scripts run in the preview, as in Study.
6. Drafts are not kept across a restart.
7. Three PRs: 3.4a to 3.4c.

## Build notes (step 3.4a: core and API)

Branch `step/3.4a-notetype-api`, built to this ADR and `docs/plans/3.4-notetype-editor.md`.

**What exists**

- `fc_core::notetype::draft`: `Draft` (`DraftField`, `DraftTemplate`, `Draft::of`), `save_note_type`,
  `note_type_consequences`, `check_note_type`, `preview_card`, `copy_note_type`,
  `restore_note_type_registers` (what undo writes) and `note_count`. `template::rename_field` is
  public. `RegisterChange` and `RegisterValue` carry what undo needs.
- One write per save, only the registers that differ, positions by the longest run already in order
  (the rest go between their neighbours, or the list is numbered again when a gap has no room), one
  reconcile. A new row is undone by setting its `deleted` register, never by deleting it.
- The dry run is the same write, left with an error so it rolls back. Its counts are what a save
  then does by construction, and a test checks it.
- `fc-api` (`notetypes.rs`): `getNoteTypeDetails`, `renameFieldInTemplates`, `previewNotes`,
  `previewCard`, `checkNoteType` (checks, then answers the counts), `saveNoteType`, `createNoteType`,
  `deleteNoteType`; `noteCount` on `getNoteTypes`; `Reverse::NoteTypeSave`, `DeleteNoteType` and
  `RestoreNoteType` in `undoChange`. Mistakes come back with line and column and a message without
  the place, for the editor to jump to.
- CLI: `notetype copy | draft | check | save | preview`; `notetypes` shows note counts.

**Measured** (release `fc` binary, wall time of the whole command including opening the file, 6,019
Basic notes from `fc fake --notes 10000`): `check` and `save` of a front that still shows a field,
0.04 s each; `check` and `save` of a front that sends all 6,019 cards to the trash, 0.20 s each;
`preview` under 0.01 s. Phone and wasm not measured.

**Verified:** `cargo xtask check` passes. Core: 20 new tests (each kind of change alone, all
together, a refused draft writes nothing by digest, the dry run equals the save, undo of an emptied
front brings back the same cards, undo of additions, restore through a draft, rename keeps values,
one moved field writes one position, two devices moving different fields converge both ways, a
replay of `changes(All)` converges, preview with and without a note, with a mistake, per cloze
number). API: 9 tests through `dispatch`, including "That can no longer be undone." after another
change. CLI: one test through the real binary.

**Not verified:** the phone, the web client, Windows, wasm at run time (it only builds and passes
clippy). 3.4a has no screen, so there was nothing to run on the desktop.

### Deviations from the plan

- `preview_card` takes the note type's ID first (`preview_card(note_type, &draft, template, note,
  ordinal)`): it needs the kind (standard or cloze) and a check that the note type is live, and the
  draft does not carry them.
- The dry run is the real save rolled back, not a separate planner over the draft. Same answers,
  one code path, but it does the writes and discards them (0.20 s at 6,019 cards).
- Extra, not in the plan: `check_note_type` (the checks without the counts, used by
  `checkNoteType`), `restore_note_type_registers`, `note_count`, `Draft::of`, and the CLI verb
  `notetype draft` (prints a note type as a draft file to edit).
- A save also rewrites field renames in templates the draft leaves out (removed ones, and ones
  removed by this save), as renaming a field did before, so restoring them later still shows the
  field. A swap of two names in one save (A to B and B to A) is not handled by that rewrite; the
  draft's own templates are the screen's to rewrite, as planned.
- New dependency: `serde_json` in `fc-cli`, to read `@draft.json`. It is already in the lockfile
  through `fc-api`; no new crate is downloaded.
- Three API types are named `NewNoteTypeInput`, `NewNoteTypeOutput`, `NoteTypeSaved` and
  `NoteTypeDeleted` instead of `CreateNoteTypeInput` and so on: Biome orders the generated imports
  differently from Rust for names like `DeleteNoteTypeOutput` next to `DeletedDeck`, which fails
  the bindings test.
- Test fixtures in `packages/ui` gained `noteCount: 0`, because `noteCount` is now a required field
  of `NoteTypeSummary`.

## Build notes (step 3.4b: the note type screens)

Branch `step/3.4b-notetype-screens`, stacked on `step/3.4a-notetype-api`.

**What exists**

- Routes `#/notetypes` and `#/notetypes/<id>` under the Settings destination; a "Note types" row in
  Settings; "Manage note types" under the Type choice on Add.
- `screens/notetypes/NoteTypeList.tsx`: name, kind and note count per type, and New note type (name,
  Start from: a copy of any type, or the Basic or Cloze shape when none is left).
- `screens/notetypes/NoteTypeScreen.tsx` with the draft in a reducer (`draft.ts`): name; Fields and
  Cards rows (an Options sheet under 600 px, row buttons from 600 px: Rename, Move up, Move down,
  Use to sort, Remove); Add field and Add card (a copy of the first card, next free name); Removed
  fields and cards with Restore; Delete note type with the note count and Undo.
- Save: the name check, `checkNoteType`, the trash question when `cardsRemoved > 0`, "Saving...",
  `saveNoteType`, then the notice with Undo. Ctrl+S saves. Back, Escape and the system Back ask
  "Discard your changes?" when there is a change. A rename of a field calls `renameFieldInTemplates`
  and replaces the draft's templates.
- `core-client` exports the new types the screens use.

**Verified:** `cargo xtask check` passes. New tests: the reducer (9), the list (3), the screen (15:
both layouts, rename rewriting, Save paths with no question, the trash question and a refusal,
Undo, Discard, delete with Undo, Ctrl+S, cloze), the route, the Settings link and the Add link. In
the desktop app (Linux, `bun run dev`, an empty collection, view only, nothing saved): Settings,
Note types, a note type screen at the app's default width (about 1,200 px) in the light theme, and
the Rename window. That run found two layout faults (a field name squeezed to "Fro/nt" by the
buttons, and plain text boxes), both fixed.

**Not verified:** the compact layout (Options sheet, Save above the keyboard, 48 px targets) in
any real browser or on the phone (the desktop window could not be narrowed from here and no phone
was connected); the dark theme; Save, Undo and Delete against a real collection (the desktop run
was view only, so as not to write to Anthony's data); Windows; the web client; typing with Samsung
Keyboard and Gboard.

### Deviations from the plan

- Edit template and Styling are not shown, as the plan says. Nothing else is missing.
- A rename opens a small window (Rename) instead of editing the name in the row, so one rename is
  one `renameFieldInTemplates` call, not one per key.
- After "New note type" the screen opens the new type at once, with no list notice; Undo of a
  create is therefore only reachable through `undoChange` until the next change. (3.4a keeps the
  token; the list does not show it.)
- Deleting shows the notice on the same screen (Undo, Dismiss, a link back) instead of returning to
  the list with the notice.
- `core-client` gained type exports; the destination list gained the two routes so Settings stays
  highlighted.
- Two existing tests (`StudyScreen` "Replay sound" and `AddScreen` "photo from the camera") timed
  out once each when the whole suite ran under load, and pass alone and on a second full run. Not
  changed.

## Build notes (step 3.4c: the template editor and live preview)

Branch `step/3.4c-template-editor`, cut from `status/3.4ab-merged` (local `master` was behind and did
not have 3.4a or 3.4b).

**What exists**

- `TemplateEditor.tsx`: a full-screen layer (`Dialog variant="layer"`) over the note type screen
  with tabs Front, Back and Styling, a plain `<textarea>` (monospace, autocapitalize, autocorrect and
  spellcheck off, wrapping under 600 px, scrolling sideways from 600 px, Tab keeps its meaning), an
  Insert row (`{{Name}}` for each field, `{{FrontSide}}` on Back, `{{cloze:Name}}` on cloze types,
  Show if... wrapping the selection in `{{#Name}}...{{/Name}}`) and Done. It edits the screen's draft
  as you type, so Done only closes it.
- Mistakes and warnings under the box, from `previewCard`: line, column and the core's sentence;
  tapping one switches to its side and puts the cursor there. Unknown fields are a warning.
- `TemplatePreview.tsx`: `usePreview` (`previewNotes` for 50 notes, `previewCard` after 400 ms of
  stillness, at once for another note or number) and the pane (Front and Back, Previous and Next,
  cloze number, the "no notes", "makes no card" and "Fix the mistake" lines). The last good render
  stays, dimmed, while there is a mistake. Two frame slots take turns, so the old card stays until the
  new one has reported a height.
- `templateText.ts`: insert, wrap, jump to a line and column, cloze numbers. Reducer actions
  `templateText` and `css`. Edit template in a card's options and a Styling button on the note type
  screen.
- Layout: side by side from 840 px (the window widens to 72 rem), one column under it with an Edit /
  Preview switch. Under 600 px the Insert row and the switch share the bar above the keyboard;
  from 600 to 839 px the switch is in the header. Preview blurs the box; Edit puts the cursor back.

**Verified:** `cargo xtask check` passes. 29 new tests (text helpers, the reducer actions, the
editor: insert at the cursor, wrapping, tabs, Done keeping the edit and Save sending it, Styling, the
debounce, a mistake keeping the last preview and the cursor jump, a field that is not a field, notes
Previous and Next, sides, no notes, makes no card, cloze numbers, the frame swap not going back,
phone and 840 px layouts). In the desktop app (Linux, `bun run dev`, a throwaway data directory,
two notes added through the UI): Settings, Note types, Basic, Edit template; side by side at the
default width; the Insert chips; typing `{{#` showed the mistake with its place and dimmed the
preview; fixing it updated the preview; Done, Save ("Saved Basic.") and Undo ("Put back the earlier
version of Basic.") all worked against a real collection. The run found three faults, fixed: an
unreadable pressed button, an oversized warning icon, and the preview going back to the old card
after 1.5 s (the hidden frame slot kept the old card and the fallback timer showed it again).

**Not verified:** the one-column layouts (under 840 px, including the Edit / Preview switch and the
Insert row above the keyboard) in a real browser or on the phone: the desktop window cannot be
narrowed from here and no phone was connected; the dark theme; the web client; Windows; typing with
Samsung Keyboard and Gboard; Save on Anthony's real Polish note type with timing; a card with media
in the preview.

### Deviations from the plan

- Branched from `status/3.4ab-merged`, not `master`, because local `master` was behind.
- `PreviewNote` has no `clozeNumbers` (3.4a shipped `noteId` and `title`), so the cloze number picker
  reads the numbers from the note's fields with `getNote` and a pattern in the UI. No API change.
- Ctrl+S inside the editor closes the editor and then saves, so a refusal or the trash question is
  seen on the note type screen. The plan said it saves the whole draft; it does.
- The Insert row's buttons are labelled with the tag they insert (`{{Back}}`), so they are not
  confused with the Front and Back tabs. A cloze type gets both the plain and the `{{cloze:Name}}`
  button for each field.
- "Edit / Preview" is in the header from 600 to 839 px and in the bar above the keyboard under 600
  px (the ADR said both "header" and "BottomAction").
- The double buffer uses the height a frame reports as its ready signal, so `CardFrame` is
  unchanged. A frame that never reports one is shown after 1.5 s.
- Styling opens the editor on the first card, which is the card the preview shows.
- `core-client` exports `PreviewNote`, `PreviewCardInput` and `TemplateMistakes`.
- The editor's window has a minimum height from 840 px so a list of mistakes does not make the
  centred window jump.
