# 0013: Browser: search box, filter chips, sorting and saved searches

Status: Accepted
Date: 2026-10-16
Accepted: 2026-10-16 (with the answers in Decisions on review)

## Context

Step 3.1 (`docs/phases/03-browse-edit-capture.md`) replaces the Browse placeholder with a real
browser. Acceptance criteria:

- A search box plus tap-friendly filter chips (deck, tag, note type, state, due) that build queries
  without typing syntax.
- Saved searches.
- Sorting.
- The phone layout is first-class, not a squeezed table.

Review focus: "try the exact thing that annoyed you in AnkiDroid, on the phone." `PRODUCT.md` names
that annoyance: "Mobile apps are mainly reviewers. Managing, filtering and editing cards on a phone is
painful." Phase 3's exit criterion is "any card can be found and edited on the phone in a few taps".
3.1 is the "found" half. Editing is 3.2.

The brief says "Sonnet build". A design session comes first because no accepted ADR decides how
chips and typed text make one query, how a chip picks exactly one deck or tag, where search sits on
a phone, or what the browser's API is.

What exists and constrains this:

- **`fc_core::search` (step 1.9, ADR 0006 build notes).** A query language, ten sorts, paging,
  cards or notes, saved searches (a synced table with `name`, `query`, `sort`, `mode`), and
  `check_query`. Approved in 1.9: **"the filters a screen offers as buttons build the same text"**,
  and **names (decks, tags, note types, templates) match any part of the name**, with "no way to ask
  for a whole-name match" listed as a known gap.
- **Speed (1.9, Linux, release, 50,000 cards):** most queries 40 to 160 ms, one text word about
  290 ms, two words about 550 ms. Not measured on the phone or the web.
- **ADR 0003:** on the web, calls run one at a time, and cancelling a running call restarts the
  worker (about 2 s with the core unavailable).
- **ADR 0010:** the shell owns insets and the keyboard. A screen's primary action sits at the bottom,
  in thumb reach, and nothing important lives in the top corners. `BottomAction` is a slot the shell
  pins above the keyboard. One layout per window width, never a platform check. Tap targets 48 px in
  the compact layout. `Dialog` (2.5a) is the shared modal, with Android back closing it.
- **ADR 0005:** card content is untrusted, and no card HTML enters the app's own DOM.
- **`fc-api` today:** `getDeckList`, `getNoteTypes`, `getTags`, and no search or saved-search
  method. `ApiError` has a kind and a message, no details.
- **The phone** is a Samsung SM-S928B (Galaxy S24 Ultra, about 6.8 inches, ADR 0010 findings). The
  top of the screen is out of one thumb's reach.

## Findings

### Run here (throwaway, in the session scratchpad, outside the repo)

1. **A chip cannot use today's `deck:`, `deckonly:` or `tag:` to mean one deck or tag.** With the
   repo's `fc` CLI (debug build of `master`) on a new collection with decks `Polish`,
   `Polish::Verbs`, `Irregular Verbs` and `Verbs`, and tags `verbs`, `verbs::irregular` and
   `adverbs`:

   | Query | Found |
   | --- | --- |
   | `deck:Verbs` | Verbs, Polish::Verbs, Irregular Verbs |
   | `deckonly:Verbs` | the same three (each has "Verbs" in its own name) |
   | `deck:"Polish::Verbs"` | Polish::Verbs |
   | `tag:verbs` | `verbs`, `verbs::irregular` and **`adverbs`** |
   | `deckonly:Polish` | Polish only |

   So a person who taps the deck "Verbs" in a list would also get "Irregular Verbs", and the tag
   `verbs` brings in `adverbs`. This matches the 1.9 rule ("names match any part") and is right for
   typing, but chips need a whole-name form. Reading `compile.rs` agrees: decks, tags and note types
   all use `Pattern::is_inside`; `Pattern::is_all_of` (whole match) exists and is used only for field
   names.

### Read in source

2. **The lexer forgets where quotes were.** `deck:="x"` and `deck:"=x"` both lex to the value `=x`.
   A whole-name marker that must not collide with a name that starts with `=` needs one more flag on
   the lexed word (whether the value's first character was an unquoted `=`).
3. **A name ending in a backslash cannot be quoted today.** Inside quotes the lexer turns only `\"`
   into a quote, and `Pattern` reads `\\` and `\*`. So `"a\\"` lexes as `a\` followed by an escaped
   quote and an unclosed string. Chips quote every name, so the build must handle `\\` inside quotes
   (or test that such a name round-trips). Deck and tag names with a backslash are rare but allowed.
4. **`SearchRow` has the sort field (`title`) but no second field**, and its `total` is a `u64` and
   `due_day`/`due_ms` are `i64`, which ADR 0002 forbids in API types. The API needs its own row type.
5. **Saved searches have no restore.** `delete_saved_search` sets `deleted` and nothing sets it back,
   so Undo after a delete has to save the search again (a new ID).

### Looked up (web, 2026-10-16)

6. **Bottom-placed search is now a mainstream phone pattern for one-handed use.** Chrome for Android
   added "Move address bar to bottom" (Chrome 135, rolled out from June 2025), after Samsung Internet
   and Chrome on iOS; Google's post says the best position depends on hand size and device
   ([Google blog](https://blog.google/products/chrome/address-bar-position-change/),
   [9to5Google](https://9to5google.com/2025/06/24/chrome-bottom-address-bar-android/)). One report
   says Chrome moves the bar back to the top while the keyboard is open. Not verified.
7. **Filter chips** (Material and design systems built on it): a chip set is either single-select or
   multi-select, not mixed; a selected chip shows a check mark; long sets scroll sideways with a cue
   at the edge; use short category nouns and avoid negative wording
   ([Firefox Acorn](https://acorn.firefox.com/latest/mobile/components/chips/android-ZX1gImzt),
   [Flutter FilterChip](https://api.flutter.dev/flutter/material/FilterChip-class.html)). The
   official m3.material.io page did not come up, so this is second-hand.

### Not verified

- **Search speed on the phone and in wasm.** 1.9 measured Linux only. The UI design below does not
  depend on it (debounce, latest result wins, no cancel), but whether typing feels instant on the
  phone with Anthony's collection is checked in the build.
- **A text field inside a `Dialog` with the phone's keyboard open.** The pickers have a filter field.
  2.5a opened a `Dialog` on the phone but did not type in it. Whether the keyboard hides the list is
  checked in the build.
- The search panel in the `BottomAction` slot with the keyboard open on the phone (the slot itself
  was built for this in 2.1, and 2.4's toolbar uses it).
- Windows, Firefox, Safari.

## Decision 1: how chips and the search box make one query

**Chips and free text are two parts of the screen's state, joined into one query text when the
search runs.** The query text stays the only thing the core and saved searches know, as 1.9 approved.

- The screen holds `filters` (per chip kind, the chosen values) and `text` (what is in the box).
- `toQuery(filters, text)` (TypeScript, pure) writes the filters in a fixed order and shape, then the
  text in brackets: `deck:="Polish::Verbs" (tag:="verbs" or tag:="nouns") is:new (kot or pies)`.
  - Values of one kind are joined with `or`, kinds with a space (and).
  - Every name is quoted, with `"` and `\` escaped, and `*` escaped so a name is never a wildcard.
  - The text goes in brackets so its own `or` cannot reach the chips (`and` binds tighter).
- `fromQuery(query)` (TypeScript) reads back only that fixed shape from the front of a query and
  leaves the rest as text. A query that was typed by hand or written by the CLI opens with everything
  in the box. Either way it finds the same cards. A round-trip test (`fromQuery(toQuery(s)) == s`
  for generated states, names with quotes, stars, backslashes, `::` and Polish letters) keeps the two
  in step, and a fixture file of `toQuery` outputs is also run through the Rust parser and search
  (plan, 3.1b).
- **The box shows only the free text.** Typing `deck:Polish` there still works (it is query text).
  The box's own mistakes are checked with `checkQuery(text)`, so the position in the error points
  into the box, not into the joined query.
- The box is the person's own words. A chip is never turned into text behind their back, and text is
  never turned into a chip.

### Options considered

| Option | For | Against |
|---|---|---|
| **A. Chips and free text, joined into one query (chosen)** | No core change for the joining. Saved searches stay text. The box stays plain words. Typed syntax still works | A second, small reader of the query in TypeScript (`fromQuery`), which only knows the shape it writes |
| B. One box holds the whole query; chips edit its text, and the core describes the top-level terms so the screen can draw them as chips | One source of truth. Typing `deck:x` makes a chip appear | The box fills with `deck:="…"` syntax, the jargon the brief wants hidden. A new core method to describe and locate terms. Editing chips means splicing text by position |
| C. Filters as structured data in the API (IDs), and in saved searches | Exact by construction. Survives a rename | Reverses the approved 1.9 rule. A new register on `saved_search` (a sync format change). Two ways to say the same filter |

## Decision 2: a whole-name form for names

**Add `=` after the colon for `deck:`, `deckonly:`, `tag:`, `note:` and `card:`: the name must match
from end to end** (case and accents still ignored, `*` still a wildcard, `\*` a star).

| Query | Means |
| --- | --- |
| `deck:="Polish::Verbs"` | the deck with that path, and the decks inside it |
| `deckonly:="Polish::Verbs"` | that deck only |
| `tag:="verbs"` | the tag `verbs` and the tags inside it (`verbs::irregular`), not `adverbs` |
| `note:="Basic"` | the note type Basic, not "Basic and reversed" |
| `card:="Card 1"` | templates with exactly that name |

- `=` counts only when it is the first character of the value and not in quotes, so `deck:"=x"` still
  looks for a deck with `=x` in its name (finding 2).
- `deck:=` matches the whole path. A deck chosen from the tree is written by its path, so two decks
  with the same name in different places stay apart. Two decks with the same path after a merge
  (ADR 0006 rule 6) are both found, which is right: they look the same to the person.
- Chips always write this form. Typing keeps the "any part" form, which is what people expect from a
  search box.
- Quoting handles `\\` (finding 3).
- The module doc's filter table and the 1.9 grammar notes gain the `=` row.

### Options considered

| Option | For | Against |
|---|---|---|
| **`=` before the name (chosen)** | Readable. Same sign as numbers' `=5`. Small lexer and compiler change. Works when typed | Changes the meaning of `deck:=x`, which today looks for "=x" inside a name (nothing relies on it) |
| IDs in the query (`deck:id:0192…`) | Exact. A saved search keeps working after a rename | Unreadable in a saved query and in the CLI. The screen needs the names anyway to draw chips |
| Make whole-name the default for names | Simplest for chips | Reverses Anthony's 1.9 decision ("`deck:Animals` finds `Polish::Animals`") for typed searches |
| Quoted means whole (`deck:"Verbs"`) | No new sign | Quotes today only group words. Changing that silently changes existing queries |

## Decision 3: the screen layout

### Compact (under 600 px): the search panel at the bottom

```
 +-------------------------------+
 | Browse            128 notes   |   heading and count (scroll away)
 | kot                           |   rows: sort field, bold
 | cat                           |         second field, muted, one line
 | Polish::Animals · Due in 3d   |         deck · state or due
 |-------------------------------|
 | być                           |
 | to be                         |
 | Polish::Verbs · New           |
 |            ...                |   list scrolls, next page loads near the end
 +-------------------------------+
 | [Saved][Deck ✓][Tag][Type]... |   BottomAction: chip row, scrolls sideways
 | [⌕ Search your cards     ][⇅] |   search box, Sort button
 +-------------------------------+
 | Decks   Add   Browse   Settings|  navigation bar (hidden while typing)
 +-------------------------------+
```

- The search box and the chip row live in the shell's `BottomAction` slot, so they are in thumb
  reach, and while typing they sit straight above the keyboard with the results visible above them
  (ADR 0010 hides the navigation bar then). Nothing is in the top corners.
- Results read top to bottom as usual. The count is under the heading.
- **Rows are cards in a list, not table columns**: the note's sort field (bold, one line), the next
  field that has text (muted, one line, so `kot` shows `cat`), then the deck and the state or due
  ("New", "Learning", "Due today", "Due in 3 days", "2 days overdue", "In 12 minutes", "Suspended",
  "Buried until tomorrow"). Long text is cut with an ellipsis. Text is shown as React text nodes from
  the core's plain-text reading, never as HTML (ADR 0005). `[sound:…]` shows as a ♪ and a picture as
  🖼, not as markup.

### 600 px and over: the same panel at the top

- The search box, Sort and the chip row sit above the list (the desktop habit), as one row where it
  fits. The list is the same component, with the rows' three lines laid out as columns from 1024 px.
- `/` focuses the search box (ignored in text fields, like the other shortcuts). Escape in the box
  clears the text. Enter runs at once instead of waiting for the debounce.
- Where the panel goes is chosen by a width media query (a small `useMediaQuery` in the shell if it
  has none), never by platform.

### Options considered

| Option | For | Against |
|---|---|---|
| **Bottom panel on phones, top from 600 px (chosen)** | One-handed on a 6.8-inch phone. Above the keyboard while typing, results visible. Chrome and Samsung Internet now offer the same (finding 6) | Two placements. Less space for rows on a phone (box, chips and the navigation bar). Unusual on Android outside browsers |
| Top search bar everywhere | The common Android and desktop pattern | Out of thumb reach on Anthony's phone. ADR 0010 asks for the primary action at the bottom |
| A list with a Filter button that opens a full-screen filter page | Room for every filter | Two screens for one search. What is applied is hidden behind a button, against the brief's "tap-friendly chips" |

## Decision 4: chips and their pickers

**Chip row, in order:** Saved, Deck, Tag, Note type, State, Due, Added, Answered. A chip with values
shows a check mark and a short label ("Deck: Verbs", "Tag: 2"), and a long press or its × clears it.
The row scrolls sideways with a fade at the edge. "Added" and "Answered" go beyond the brief's list
(see Questions).

**Tapping a chip opens its picker** in a `Dialog`, styled as a sheet from the bottom under 600 px.
Each picker has Apply and Clear, and Android back closes it with nothing changed (2.5a).

| Chip | Choice | Writes |
|---|---|---|
| Deck | several, from the deck tree with a filter box | `deck:="path"` each, joined by `or` (a deck includes the decks inside it, as studying it does) |
| Tag | several, from `getTags` with a filter box | `tag:="name"` each (children included) |
| Note type | several | `note:="name"` each |
| State | several of New, Learning, Review, Suspended, Buried | `is:new`, `is:learning`, … joined by `or` |
| Due | one of Due now, Overdue, Due tomorrow, Due in the next 7 days | `is:due`, `due:..-1`, `due:1`, `due:1..7` |
| Added | one of Today, Last 7 days, Last 30 days | `added:1`, `added:7`, `added:30` |
| Answered | one of Today, Answered Again today, Last 7 days | `rated:1`, `rated:1:1`, `rated:7` |

- Several values of one chip are "any of" (or). Different chips are "all of" (and). Single-choice
  chips are radio lists, multi-choice ones checkboxes (finding 7: no mixing within one set).
- A deck, tag or note type that no longer exists keeps its chip, marked "(not found)", and matches
  nothing, so a saved search does not change silently. Its × removes it.
- **No "exclude" in chips** for now. `-tag:x` can be typed. See Revisit.
- The pickers use names from `getDeckList`, `getTags` and `getNoteTypes`, read when the picker opens.

## Decision 5: running searches as people type

- A search runs 300 ms after the last change to the box, and at once after a chip, sort or mode
  change, or Enter.
- **One search in flight at a time, the latest wins.** A change while one runs queues one more run
  with the newest state. Older results that arrive late are dropped. Nothing is cancelled: on the web
  a cancel costs a 2 s worker restart (ADR 0003), and searches are short.
- While a search runs, the old results stay and a quiet "Searching…" status shows (after 300 ms, so
  fast searches do not flicker).
- A mistake in the box (`checkQuery`) shows its message under the box (`role="alert"`), with the old
  results kept and dimmed. No search runs until it reads.
- Pages of 100 rows. The next page loads when the end of the list comes near
  (`IntersectionObserver`, in WebKitGTK, the Android WebView and every browser). No virtual list:
  plain rows are fine at a few thousand, and the core caps one call at 1,000. See Revisit.
- The list is read again when the screen opens, and on window `focus` and `visibilitychange`, as the
  Decks screen does. CoreEvents are not used yet: 3.2's edits will add a refresh after their own
  writes.

## Decision 6: sorting and cards or notes

- **Sort** (the ⇅ button) opens a `Dialog` with the ten core sorts in plain words, each with its
  direction (a second tap reverses it):

  | Core key | Label | Default direction |
  | --- | --- | --- |
  | `created` | Date added | newest first |
  | `due` | Due date | soonest first |
  | `field` | Alphabetical | A to Z |
  | `deck` | Deck | deck list order |
  | `note` | Note type | A to Z |
  | `difficulty` | Difficulty | hardest first |
  | `stability` | How well you know it | weakest first |
  | `lapses` | Times forgotten | most first |
  | `reviews` | Times reviewed | most first |
  | `random` | Shuffle | (a new order each time it is chosen) |

  Shuffle picks a new seed when chosen, so paging keeps one order.
- **Notes or cards:** a two-way switch in the Sort dialog, "Show: Notes | Cards". **Notes by
  default**, so a "Basic and reversed" note shows once when you search for a word. A note's row
  shows the state of its matching card with the lowest ID (the 1.9 rule), and the count says "notes"
  or "cards".
- **Remembered per device** (`localStorage`, `fc.browse`, try/catch, like the theme and the Add
  draft): the chips, the text, the sort, the mode and which saved search is open. Opening Browse
  shows the last search. Not synced.

## Decision 7: saved searches

- **The Saved chip** opens a sheet that lists the saved searches by name. Tapping one loads its
  query (through `fromQuery`), sort and mode, and its name shows under the heading
  ("Saved: Verbs to fix").
- **Save** in the same sheet saves what is on screen, asking for a name. A name in use is refused
  with the core's sentence, in the window, which stays open (2.5a's pattern).
- With a saved search open and changed, the sheet offers **Update "Verbs to fix"** and **Save as
  new**.
- **Rename** and **Delete** on each row. After a delete, a notice offers Undo, which saves it again
  with the same name, query, sort and mode (finding 5: a new ID; on another device it is a new saved
  search, which is the same to a person).
- Saved searches are collection data and sync (ADR 0006), unlike the remembered last search.

## Decision 8: the API

New `fc-api` methods, thin over `fc_core::search`:

| Method | Input | Output |
|---|---|---|
| `search` | `query`, `sort` (`{ key, descending }`), `mode` (`notes` or `cards`), `offset`, `limit`, `seed` | `total` (`u32`, saturating), `rows` |
| `checkQuery` | `query` | `problem`: null, or `{ message, position }` |
| `getSavedSearches` | none | each: `id`, `name`, `query`, `sort`, `mode` |
| `saveSearch` | `name`, `query`, `sort`, `mode` | `id` |
| `updateSavedSearch` | `id`, then `name`, `query`, `sort`, `mode`, each optional | nothing |
| `deleteSavedSearch` | `id` | nothing |

- **A row:** `cardId`, `noteId`, `noteTypeId`, `deckId`, `title` (the sort field), `detail` (the
  next field with text, plain), `state` (`new`, `learning`, `review`, `relearning`), `due`
  (`{ kind: "day", days }` counted from today, `{ kind: "time", minutes }` from now, or null),
  `suspended`, `buried`. All `i32` or `u32` (finding 4). The screen gets deck and note type names
  from the calls it already makes.
- **Core change:** `SearchRow` gains `detail`, read with the same per-row statements as `title`
  (the first field after the sort field, by position, whose readable text is not empty; then the
  first before it). `html::readable` already gives the plain text.
- **Errors:** a query that does not read, an unknown field, an empty or taken name are
  `invalidInput` with the core's sentence; a saved search that is gone is `notFound`; the rest is
  logged and answers the generic message. `checkQuery` returns the position as data, because
  `ApiError` has no details.
- `limit` above 1,000 is capped by the core. `runSavedSearch` is not added: the screen loads a saved
  search into its state and calls `search`.
- No new events: saved-search writes emit nothing (ADR 0009 adds variants when a screen needs them,
  and only this screen shows saved searches).

## Consequences

- **A grammar change in the core** (`=` and `\\` in quotes) with tests, and one new field on
  `SearchRow`. No migration, no sync format change.
- **No new dependency.** No new bridge command, permission or CSP change. Nothing that costs money.
- **The browser is the first screen that renders its main controls in `BottomAction` on phones.** If
  that feels wrong in Anthony's hand, moving it to the top is a change to one component (decision 3
  table), not to the API or the query model.
- **Two small TypeScript functions mirror a corner of the grammar** (`toQuery`, `fromQuery`). The
  round-trip test, and a shared fixture file of `toQuery` outputs that a Vitest test writes against
  and a Rust test runs through the real parser and search, keep them honest.
- **Renaming a deck, tag or note type does not update saved searches**: a saved search for
  `deck:="Polish::Verbs"` finds nothing after a rename, and its chip says "(not found)". See Revisit.
- **Typing is not cancelled.** On a slow phone a long text search finishes before the newest one
  starts. The latest result still wins.
- **The remembered search is per device**; saved searches sync.
- **3.2 builds on this screen:** tapping a row (edit), multi-select and bulk actions. 3.1 gives rows
  no action (see Questions).
- `docs/ROADMAP.md` and the phase file need no change.

## Revisit if

- **A rename breaks saved searches in practice.** Then rewrite the names inside saved queries when a
  deck, tag or note type is renamed (a write in the same transaction), or store IDs (option C of
  decision 1).
- **Typing feels slow on the phone** with Anthony's collection. Then the lower-cased text index from
  1.9's notes (a migration), and only then cancel on native.
- **Excluding is wanted from chips** (for example "not suspended"): a third state per value, or a
  "Without" picker, writing `-tag:="x"`.
- **Thousands of rows scroll badly** on the phone: a windowed list (own code or a small dependency,
  asked first).
- **The bottom panel feels wrong** on the phone, or in landscape: the top placement for every width.
- **Stats (3.7) or another screen wants to open the browser with a search**: a `#/browse?q=` route.
- **Add-ons (Phase 10)** want to add filters: chip kinds become a list other code can extend.

## Questions for Anthony

1. **Chips plus a plain search box** (decision 1), rather than one box that shows the whole query.
2. **The `=` whole-name form** in the query language for chips (decision 2), which also works typed.
3. **The search box and chips at the bottom on the phone**, at the top from 600 px (decision 3).
4. **"Added" and "Answered" chips** beyond the brief's five ("Answered Again today" is the quick way
   to find a card you just got wrong). Keep, or leave them for typing?
5. **Notes by default**, with a switch to cards (decision 6).
6. **Rows do nothing when tapped in 3.1**, and 3.2 opens the editor from them. The alternative is a
   read-only preview of the card in 3.1 (more code now, through the existing card frame).
7. **The last search is remembered per device** and shown when Browse opens.
8. **Three PRs** as in the plan (`docs/plans/3.1-browser.md`): 3.1a core and API, 3.1b the screen,
   3.1c saved searches.

## Decisions on review

Anthony's answer on 2026-10-16, in chat: "I accept the plan". Every question is answered as
proposed:

1. Chips plus a plain search box.
2. The `=` whole-name form, also usable typed.
3. The search panel at the bottom on the phone, at the top from 600 px.
4. "Added" and "Answered" chips are kept.
5. Notes by default, with a switch to cards.
6. Rows do nothing when tapped in 3.1; 3.2 opens the editor from them.
7. The last search is remembered per device.
8. Three PRs: 3.1a, 3.1b, 3.1c.

## Build notes (step 3.1a, core and API)

Branch `step/3.1a-search-api`. No migration, no sync format change, no new dependency.

- **Grammar (`search/parse.rs`).** The lexed word records whether its value starts with an `=`
  that is not in quotes. For `deck:`, `deckonly:`, `tag:`, `note:` and `card:` the `=` is taken off
  and the term gets `whole: true`; every other key keeps it (`front:=x` is the text `=x`,
  `difficulty:=5` is still a number). An `=` with no name after it is the usual "needs a deck after
  the colon" error. Inside quotes `\\` is kept as a pair, so `Pattern` reads one backslash and the
  quote after it closes the string: a name ending in `\` can be written as a chip writes it.
- **Matching (`search/compile.rs`, `functions.rs`).** Decks with `=` match the whole path
  (`deckonly:=` too, without the decks inside), note types and templates the whole name. Tags use a
  new SQL function `fc_tag_is(tag, pattern)`: the whole folded tag, or any part of it before a `::`,
  is the pattern. Case, accents and `*` work as before.
- **`SearchRow::detail` (`search/run.rs`).** The first field after the sort field (by position)
  whose readable text is not empty, else the first such field before it, from the start. Title and
  detail come from one statement that reads all of the note's values.
- **`fc-api` (`crates/fc-api/src/search.rs`).** `search`, `checkQuery`, `getSavedSearches`,
  `saveSearch`, `updateSavedSearch`, `deleteSavedSearch`. The row's `state` reuses
  `StudyCardState`. `due` is `{kind: "day", days}` from today for review cards and
  `{kind: "time", minutes}` from now, rounded up (negative when overdue), for learning and
  relearning cards. `checkQuery`'s message is the core's sentence without "(at character N)"; the
  position is in characters. `search`'s error keeps the full sentence. `checkQuery` does not need a
  collection, and an unknown field name only shows when the search runs.
- **Tests.** 7 more core search tests and 1 more SQL function test: finding 1's collection with
  every `=` form, `=` only when unquoted and first, an empty `=`, a deck called `a"b*c\` and a tag
  `x\`, `-deck:=`, `detail` for Basic, an empty field, Cloze, a sound, a picture and a sort field
  that is last. 8 `fc-api` tests through `dispatch`: every sort both ways in both modes, paging and
  `limit` 0, syntax and unknown-field errors, `checkQuery` positions, due in minutes (an hour, 30
  rounded up, 10 overdue) and in days (3 ahead, 2 overdue), saved searches end to end, no
  collection open. The 300 random boolean queries test still passes.
- **CLI.** No new command. `tests/coverage.rs` needs no row (it lists `Collection` operations, and
  none was added). `fc search` on a scratch collection with finding 1's decks and tags:
  `deck:=Verbs` and `deckonly:=Verbs` find Verbs only, `deck:="Polish::Verbs"` that deck,
  `deck:=Polish` Polish and Polish::Verbs, `deckonly:=Polish` Polish, `tag:=verbs` `verbs` and
  `verbs::irregular` but not `adverbs`, and `deck:"=Verbs"` nothing.
- **For 3.1b.** `html::readable` leaves `[sound:name]` as written, so the row shows ♪ for that text.
  A picture leaves no text at all (an `<img>` is a tag), so a field with only a picture is skipped
  for `detail` and never reaches the screen: there is nothing to turn into 🖼 unless the core keeps
  a marker for it.

### Verified

- `cargo xtask check` passes (Rust tests, clippy, the wasm build, bindings, `tsc`, Vitest).
- `fc search` with the `=` forms on a scratch collection, as above.

### Not verified

- The phone: no build was installed (Anthony is away, STATUS). 3.1a has no UI; the phone runs it with
  the next debug APK.
- The web client and the desktop app: nothing calls the new methods until 3.1b.
- Search speed with `fc_tag_is` was not measured (it checks every `::` prefix of a tag on each row).
- Windows, Firefox, Safari.

### Deviations from the plan

- `card:=2` is a template called "2", not the second template. The plan did not say; with `=` the
  value is always a name.
- Title and detail are read with one statement over all of the note's values instead of one
  statement per field (proposed in the chat plan, approved with it).
- `seed` in `search` is a `u32`, since ADR 0002 allows no 64-bit numbers in API types (in the chat
  plan).
- Two input types were named `NewSavedSearchInput` and `SavedSearchIdInput` (not `SaveSearchInput`
  and `DeleteSavedSearchInput`): the binding test compares files in generated import order, and
  Biome sorts imports ignoring case, which put those two names elsewhere and failed the check.
- The grammar and `detail` went in one commit, not two.
- The "learning card due in an hour" test sets the Default preset's learning steps to 60 minutes
  through `setPresetOptions`; the review-card days are checked by moving the clock around the
  card's due day rather than making a card due in exactly 3 days.

## Build notes (step 3.1b, the Browse screen)

Branch `step/3.1b-browse-screen`, rebased on PR #50. No migration, no core change, no new
dependency.

- **Query (`packages/ui/src/browse/query.ts`).** `toQuery` writes the chips in the order of decision
  4, values of one kind joined by `or` in brackets, every name quoted with `\`, `"` and `*` escaped,
  and the box's text last in brackets. `fromQuery` reads that shape from the front. If anything
  after the chips is not one bracketed group that runs to the end, the whole query is text, so a hand-typed
  `deck:="x" a or b` keeps its meaning. `query-fixtures.json` holds a small collection (`notes`)
  and 18 `cases`. `query.test.ts` checks both directions on each case, plus 500 seeded random
  states for the round trip. `fc-api`'s `the_browse_screen_chip_queries_find_the_right_notes`
  builds the notes and runs each query through the real search. Changing the star escape
  makes it fail ("a star is not a wildcard": `litera` found too), so it does test something.
- **`useBrowseSearch`.**
  - Typing searches 300 ms after the last change. A chip, the sort, the mode, Enter and Escape
    search at once.
  - One search runs at a time. A change while one runs makes it run again with the newest state,
    and a result for an older state is dropped.
  - `checkQuery` runs on the box text first. A problem, or a search the core refuses, shows under
    the box, and the old rows stay dimmed.
  - Results come in pages of 100. A refresh of the same search (on `focus`, `visibilitychange` or
    mount) asks for as many rows as are on screen, up to 1,000, so coming back to the window does
    not drop what was scrolled into.
  - The search is remembered in `fc.browse`. Each part falls back to its default on its own.
- **Screen.**
  - Under 600 px the panel (chips, box, Sort) sits in `BottomAction`. From 600 px the box and Sort
    sit above the chips and the list. Rows are three lines, and columns from 1024 px.
  - The count line is the status region ("120 notes", "Searching…").
  - The next page loads through an `IntersectionObserver` 600 px before the end, with a Show more
    button as well.
  - With no match: "No cards match. Try fewer filters." and Clear filters. An empty collection
    uses the Decks wording.
- **Chips and pickers.**
  - A chip with values is filled and shows a check mark, a short label ("Deck: Verbs", "Tag: 2"),
    and a × to clear it. Its accessible name lists every value, decks by path.
  - The deck and tag pickers have a Find box. Find ignores case and accents and matches a deck's
    whole path.
  - Pickers and Sort open as `Dialog variant="sheet"`, a sheet from the bottom under 600 px. The
    sheet CSS is in `shell.css`, because it uses the safe-area and keyboard insets.
  - A chosen name the collection no longer has is shown "(not found)" in the chip and in the
    picker.
- **Sort window.** The ten sorts with their direction text, a Notes | Cards switch, and Done.
  Tapping the chosen sort again reverses it, and Shuffle takes a new seed each time.
- **Shell.**
  - `shell/useMediaQuery.ts`: `useWideLayout()` uses the same 600 px as `shell.css`.
  - `useKeyShortcut` in `useShortcuts.ts` follows the same rules as the other shortcuts. `/` is in
    the shortcuts list.
  - `core-client` exports the search types.
  - The Browse placeholder is deleted.

### Verified

- `cargo xtask check` passes (389 Vitest tests, among them 61 for Browse, plus the Rust fixture
  test).
- **Web** (`bun run web:dev`, headless Brave over CDP, real mouse and key events, screenshots read),
  at 390 × 844 and 1100 × 800, light and dark. The collection was 9 runs of `debugAddSampleCards`
  (117 notes), the decks Polish and Polish::Verbs made through the Decks screen, and 3 notes with
  tags made through the Add screen. Checked at each width:
  - typing (`kot`: 9 notes), Escape, a box mistake (`"kot`: the core's sentence, rows dimmed);
  - the deck picker (Find "verbs" shows only Polish::Verbs, which gives 2 notes and "Deck: Verbs");
  - tag plus deck with no match (the empty state), the ×, State New, Due now (0);
  - Alphabetical and its reverse, Cards (138 cards), Shuffle;
  - scrolling to the end (100 rows, then 120);
  - `/` (1100 px);
  - no sideways scroll at any step.
- **Desktop** (`bun run dev`, Linux X11, a scratch `XDG_DATA_HOME` with 113 notes made by `fc`,
  driven with `xdotool`, screenshots read):
  - the wide layout; `g` `b`, then `/`, then typing `być` (1 note);
  - the tag picker in WebKitGTK;
  - wheel-scrolling loads all 113 rows;
  - Escape in the Find box (found broken, fixed, then checked again).

### Not verified

- **The phone (skipped while Anthony is away, STATUS):**
  - the bottom panel in his hand, and landscape;
  - typing with the keyboard open (the panel above it, rows visible);
  - a picker sheet with the Find box and the keyboard open;
  - Android back closing a picker;
  - timings of a one-word search and a chip search on his collection.
- Firefox, Safari, Windows, 200% text size, a screen reader.
- Speed with a large collection in wasm.

### Deviations from the plan

- **Rebased on PR #50**, a separate PR that moves `zip` to 9.0.4. `cargo deny` started failing on
  RUSTSEC-2026-0369. Anthony chose a separate PR in chat and let me merge it.
- **No Saved chip yet.** It comes with 3.1c, so the row starts at Deck.
- **The fixture is an object:** `notes` (the collection the Rust test builds) and `cases`, each
  with an `about`. The plan described a plain list of cases.
- **Picker details the plan did not specify:**
  - **Cancel button:** pickers have Cancel next to Apply and Clear.
  - **Focus on phones:** under 600 px a picker opens with its first option focused, not the Find
    box. Otherwise the keyboard covers the list (found in the 390 px screenshots).
  - **Escape in the Find box:** it clears the text, then a second Escape closes the window.
    WebKitGTK's search field keeps Escape even when empty (found on desktop).
- **No long press to clear a chip:** only the ×. A long press competes with the Android context
  menu.
- **No 🖼 in rows:** the core leaves no text for a picture (3.1a notes). Only `[sound:…]`
  becomes ♪.
- **Cloze markers are taken out of row text** (Anthony, in chat, after the PR opened):
  `{{c1::Warszawa}} jest stolicą {{c2::Polski}}.` shows as `Warszawa jest stolicą Polski.`.
  Hints are dropped and nested clozes come apart too. This is done in `rowText` in the UI. The core's
  `title` and `detail` still carry the markers.
- **`formatDue` choices the plan left open:**
  - a learning card due now or overdue reads "Learning";
  - from 60 minutes the label is in hours ("In 3 hours");
  - "1 day" is singular.
- **Clear filters also empties the box,** so the button always gives results back.
- **"Searching…" shows at once on the first load,** when there are no rows yet. After that it waits
  300 ms.
- **Extras:**
  - a Show more button next to the observer;
  - refreshes keep as many rows as are on screen;
  - `useKeyShortcut` as a sibling of `useShortcuts`, not an option on it;
  - `/` added to the shortcuts window;
  - the search types exported from `core-client`;
  - `packages/ui/tsconfig.json` includes `src/**/*.json`;
  - a `testRows.ts` test helper.
- **Edge fade colour:** it uses `mask-image` with the keyword `black`. It is an alpha mask, not a
  colour anyone sees, so the tokens rule is not broken.
- **Desktop test data:** the scratch collection was made with the `fc` CLI rather than the UI.
