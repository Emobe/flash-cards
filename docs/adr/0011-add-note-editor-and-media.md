# 0011: Add note: field editor, media and the note API

Status: Accepted
Date: 2026-10-07
Accepted: 2026-10-07 (Anthony agreed to every question, see Decisions on review)

## Context

Step 2.4 (`docs/phases/02-study-loop.md`) adds the Add screen. Acceptance criteria:

- Pick deck and note type, fill fields, tags.
- Attach images and audio from files on desktop, and camera, gallery and files on Android.
- Comfortable to use one-handed on a phone; the keyboard never hides the active field.
- Duplicate warning.
- Basic formatting (bold, italic, lists) and cloze insertion.

The brief says "Sonnet build". A `/step 2.4` session stopped because no accepted ADR decided:

1. how a field is edited with formatting;
2. how images and audio get from the device into the collection on each platform;
3. what part of the note and media API 2.4 builds (ADR 0006 and STATUS put "`fc-api` methods for
   media" in 3.1);
4. how field content, which is untrusted, is shown in the app's own page;
5. the smaller rules: when the duplicate warning shows, what the screen remembers.

What exists and constrains this:

- **ADR 0005:** card content is untrusted and "no card HTML is ever inserted into the app's own DOM".
  The main window may call only our own token-checked commands. On Android every command granted to
  the main window is callable from card frames, so platform features go through our own commands or
  are called from Rust, never through Tauri plugin permissions.
- **ADR 0002:** platform features go in the `Platform` interface. Large files on native "pass a file
  reference, not bytes through JS", designed "in the step that adds file pickers". Bytes otherwise
  travel as call attachments. Measured in 0.3b: 5 MB round trip 181 ms on desktop, 250 ms on the phone.
- **ADR 0010:** Add is a destination (`#/add`). The shell owns the keyboard inset and gives a screen
  a `BottomAction` slot pinned above the keyboard ("2.4's save button"). Keyboard shortcuts are
  ignored while a text field has focus. One layout per window width, never a platform check.
- **Core (1.3 to 1.10):** `add_note_to_deck` (refuses a note that makes no card, returns
  `duplicates`), `find_duplicates` (first field, ignoring tags, spacing and case),
  `set_note_tags`, `tags`, `note_types` (with `kind`: Standard or Cloze), `decks`,
  `add_media(original_name, bytes)` (stores once per hash, returns the name to use, no size limit),
  `media_bytes`. Fields are HTML. Cloze is `{{cN::text::hint}}` in a field. Sound is
  `[sound:name]`.
- **`fc-api`:** `getDeckList`, `getMedia` (bytes and a content type) and the study methods. No note,
  note type, tag or `addMedia` method.
- **Main window CSP** (`tauri.conf.json`): `img-src 'self' asset: data:`, and `default-src 'self'`
  covers `media-src`. A `blob:` URL can be neither an image nor audio in the app's page today. The
  web client has no CSP (Phase 6).
- **Phase 3.2** edits notes with the same editor. That is where existing and imported HTML first
  reaches it, so this ADR sets the rule for content it cannot represent, though 2.4 only adds new
  notes.

## Findings

### Run here (throwaway experiments in `$CLAUDE_JOB_DIR/tmp`, outside the repo)

1. **`<input type="file">` works in WebKitGTK 2.52.6** (the engine Tauri uses on Linux), in its
   `MiniBrowser` with the DMA-BUF renderer off, driven with `xdotool`. A click opened GTK's "Select
   File" dialog, and the page got the file (`rot.jpg`, 399 bytes, `image/jpeg`) through
   `File.arrayBuffer()`. wry 0.57.0 has no file chooser code for Linux (searched its source), so the
   app gets WebKitGTK's default dialog, the same one. **Not run inside the Tauri app itself.**
2. **Image handling in WebKitGTK 2.52.6:** `createImageBitmap` applies EXIF orientation (a 40 × 20
   JPEG tagged orientation 6 came out 20 × 40). `canvas.toBlob(…, "image/jpeg", 0.85)` works.
   **`canvas.toBlob(…, "image/webp")` silently returns PNG**, so WebP cannot be the output format.
3. **`DOMParser` documents are inert in WebKitGTK:** `<img src="x" onerror=…>` and a `<script>` in
   a parsed string did nothing. `document.execCommand("bold")` works on a `contenteditable` (relevant
   to option C below).
4. **A ProseMirror schema is a working allowlist** (prosemirror-model 1.25.12 in happy-dom 20.14.5,
   with the schema proposed in decision 1). Parsing then serialising:
   - kept: plain text, `<b>`, `<i>`, `<br>`, lists, `<img src="cat-0123456789abcdef.jpg">`,
     `{{c1::Warszawa::city}}`;
   - dropped: `<script>`, `<iframe>`, `<style>`, `onerror` and `onclick` attributes, an `<img>` with
     an `https://` source, `style="color:red"`, `<u>`.

   So unknown formatting is **lost** on a round trip: fine for new notes, not for editing imported
   ones (decision 1, "Content the editor cannot represent"). List items come back as
   `<li><p>a</p></li>`, so the serialiser has to unwrap the paragraph.
5. **Bundle sizes** (`bun build --minify`, gzip -9, the parts this ADR would use):
   - ProseMirror (model, state, view, commands, keymap, history, schema-list): 65 KB gzipped, 11
     packages with their dependencies;
   - Lexical 0.52.0 with its React, list, rich-text, HTML and history packages, React excluded:
     91 KB gzipped, 38 packages.

   For scale, the whole native app bundle today is 84 KB gzipped.

### Read in source (wry 0.57.0's `RustWebChromeClient.kt`, generated into our Android project)

6. **The Android WebView already handles `<input type="file">`.** `onShowFileChooser`:
   - with `capture` and `accept="image/*"`, it launches `MediaStore.ACTION_IMAGE_CAPTURE` (the camera
     app) with a file of its own (`getExternalFilesDir(Pictures)/JPEG_<time>_….jpg`, through our
     existing `FileProvider`) and hands that file to the page;
   - without `capture`, it launches `FileChooserParams.createIntent()` (Android's content picker,
     where Gallery, Photos and Files are sources), with multiple selection when the input allows it.
   - **It asks for the CAMERA permission only if the manifest declares it.** Ours does not, and
     `ACTION_IMAGE_CAPTURE` needs no permission when the app does not declare CAMERA. So the camera
     works with no manifest change, and adding CAMERA later would add a runtime prompt.
   - **The temporary photo is never deleted.** Every photo taken this way stays in the app's
     external files folder.
7. Android's documentation (`WebChromeClient.FileChooserParams`) says a picker result must be treated
   as untrusted. Here the page receives a `File`'s bytes and nothing else, and those bytes go through
   the same checks as any file.

### Not verified (needs taps on the phone, which this session may not do)

- The camera, gallery and file pickers from the app on the phone, and whether Android 14 and later
  show the Photo Picker or the older content picker for `image/*`.
- ProseMirror with Samsung Keyboard and Gboard on the phone (composition, Backspace, autocorrect).
  The ProseMirror project reports Android IME bugs fixed as recently as prosemirror-view 1.41.2
  (2025-09); 1.42.6 is current.
- Whether Android kills the app while the camera app is open (low-memory devices do this to WebView
  apps). Decision 5 makes the draft survive it either way.
- Windows (WebView2) file input, Firefox and Safari.

## Decision 1: the field editor

**Use ProseMirror directly** (no Tiptap), one editor per field, wrapped in our own React component
`FieldEditor`.

**Schema** (the whole allowlist):

| Node or mark | Stored as | Notes |
|---|---|---|
| text | text | |
| line break | `<br>` | Enter inserts a line break, not a new paragraph |
| bold, italic | `<b>`, `<i>` | `<strong>` and `<em>` are read as the same |
| bullet and numbered list | `<ul>` / `<ol>` with `<li>` | one level of nesting is enough for now |
| image | `<img src="name">` | `name` must be a plain media name: no `:`, `/`, `\`, `?` or `#` |
| sound | `[sound:name]` | an inline node shown as a chip with a play button |

**Storage rule.** A field with no list is stored as its inline HTML with no wrapper: typing `kot`
stores `kot`, not `<p>kot</p>`. Duplicate checks, the sort field and card templates see what they see
today. Two paragraphs (from a paste) are joined with `<br>`. Cloze markers are plain text in the field
and need no node.

**How content enters the editor.** Field HTML is parsed with `new DOMParser().parseFromString()`
(an inert document, finding 3), then with the schema's parser (finding 4). Nothing from a field is
ever put into the page as HTML. The same applies to pasted HTML: ProseMirror runs paste through the
schema, so a paste from a web page brings text, bold, italic and lists and nothing else.

**How media shows in the editor.** An image node's view makes its own `<img>` whose `src` is a
`blob:` URL our code made from bytes (a file just picked, or `getMedia(name)`), never the field's
`src`. A sound chip plays through an `<audio>` element with a `blob:` URL. This needs `blob:` in the
main window CSP: `img-src 'self' asset: data: blob:; media-src 'self' blob:`. A `blob:` URL can only
come from code in our own page, so this gives card content nothing.

**Toolbar and keys.**

- One toolbar, for the focused field: Bold, Italic, Bullet list, Numbered list, Cloze (only for a
  Cloze note type), Image, Sound. It lives in the `BottomAction` slot with the Add button, so it sits
  above the keyboard on a phone and in thumb reach. The buttons do not take focus (`pointerdown`
  default prevented), so tapping one does not close the keyboard.
- Keys: Ctrl+B, Ctrl+I, Ctrl+Z and Ctrl+Shift+Z (ProseMirror history), Ctrl+Shift+C cloze with a
  new number, Ctrl+Shift+Alt+C cloze with the current number, Ctrl+Enter adds the note. Key hints
  only where there is a mouse, as in 2.3.

**Cloze insertion.** Wraps the selection in `{{cN::…}}`, where N is one more than the highest number
in any field of the note (or the same number, with the second shortcut or a long press). With no
selection it inserts `{{cN::}}` with the cursor inside. The button is labelled "Cloze" with a tooltip
"Hide this text on a card". No jargon beyond the note type's own name.

**Content the editor cannot represent (rule for 3.2).** When a field's HTML does not survive a
parse and serialise unchanged (after normalising whitespace and `<strong>`/`<b>`), that field opens
in a plain HTML text box with a line saying "This field has formatting the editor cannot show". The
editor never silently drops content. 2.4 builds the check, because Add's draft restore (decision 5)
and paste use the same loader, and tests it. Nothing in 2.4 shows the HTML box.

### Options considered

| Option | For | Against |
|---|---|---|
| **A. ProseMirror (chosen)** | 1.x and stable for years. The schema is the sanitiser (finding 4). The most work on Android input of any editor. Framework-free, so the React wrapper is ours. 65 KB gzipped, 11 packages, MIT. | We write the wrapper, toolbar state and the serialiser (about 400 lines). Unknown formatting is dropped, so 3.2 needs the HTML fallback. |
| B. Lexical (Meta) | React-first. Also schema-based import. | Still 0.x (0.52.0) with breaking releases. 91 KB gzipped and 38 packages. Less Android input history. |
| C. Own `contenteditable` with `execCommand` and our own sanitiser | No dependency. Keeps unknown formatting. `execCommand` works in WebKitGTK (finding 3) and Chromium. | `execCommand` is deprecated and makes different HTML per engine. We own every Android IME bug and list-editing edge case. **The sanitiser is security-critical code we would write and maintain.** |
| D. Plain text box with markup (HTML or a Markdown subset) and a preview | No dependency. Perfect keyboard behaviour. | The user sees `<b>` or `**`. Fails "easy to use, no jargon" (PRODUCT.md). |
| Tiptap | Friendlier API over ProseMirror. | More packages on top of A for features we do not use. |

## Decision 2: getting images and audio in

**Use the web platform's `<input type="file">` on every platform.** No native command, no Tauri
plugin, no Kotlin for picking.

| | Desktop (Linux, Windows) | Android | Web |
|---|---|---|---|
| Image | system file dialog | content picker (Gallery, Photos, Files) | browser file dialog, or the phone's own picker |
| Take photo | not shown | camera app (`capture="environment"`) | camera on a phone browser |
| Sound | system file dialog | content picker (Files, audio apps) | browser file dialog |

- "Take photo" is shown when `matchMedia("(pointer: coarse)")` matches (a touch screen), not by
  platform check, as ADR 0010 asks.
- Recording audio is **out of scope** (the brief asks for audio from files). Recording would need the
  RECORD_AUDIO permission and `getUserMedia`, which wry routes through its own permission handler, and
  that needs a look at what card frames could then do.
- **Bytes travel as one call attachment** to `addMedia`, the existing path. ADR 0002's "pass a file
  reference" rule was for large imports and exports. Images here are under 1 MB after decision 3, and
  a 5 MB file is 250 ms on the phone.
- **The Android manifest stays as it is.** No CAMERA permission (finding 6: declaring it would add a
  runtime prompt and change how the WebView handles capture), no RECORD_AUDIO.
- **Leftover camera files:** `MainActivity.onCreate` deletes `JPEG_*.jpg` files older than one hour
  from `getExternalFilesDir(Pictures)`. Plain Kotlin at start-up: no command, nothing a page can call.

**Card frames.** A card frame on Android can already open the same pickers, and could before this
step: they come from the WebView, not from our code. A card would get only a file the user picked
from a dialog, in a frame with no network (ADR 0005). This ADR adds no new reach.

### Options considered

| Option | For | Against |
|---|---|---|
| **A. `<input type="file">` everywhere (chosen)** | Already works in all three engines (findings 1 and 6). No new command, permission or dependency. One code path. | Bytes go through JS (fine at these sizes). Leftover camera files need the start-up clean-up. The Android picker UI is whatever the WebView launches. |
| B. Native pickers behind token-checked commands (`tauri-plugin-dialog` called from Rust on desktop, our own Kotlin plugin for camera, gallery and files on Android); Rust reads the file and calls `add_media` itself | Bytes never cross into JS. We choose the Android picker (for example the Photo Picker explicitly). | A new dependency and a sixth and seventh command. Kotlin for three intents. The web still needs A. Two code paths to test. Nothing it buys matters at the file sizes in decision 3. |
| C. `getUserMedia` camera preview inside the app | A custom camera UI. | Needs the CAMERA permission, our own capture UI, and decisions about what card frames could reach. Far more work than the brief asks. |

## Decision 3: what is stored

- **Images** are decoded with `createImageBitmap` (applies EXIF rotation, finding 2):
  - **JPEG** (every camera photo) is always re-encoded as JPEG, quality 0.85, longest side at most
    **1600 px**. A photo shrinks from several MB to a few hundred KB, and re-encoding **drops EXIF
    data, including the location**, which matters once decks are shared.
  - **PNG** larger than 1600 px is resized and stays PNG (keeps transparency). Smaller PNG is stored
    as it is.
  - **GIF, WebP, SVG** are stored as they are (animation, already small, vector). An SVG is only ever
    shown through `<img>` in the app and inside the card frame, where its scripts do not run.
  - **A file the browser cannot decode** (for example HEIC from a gallery) is refused with "This
    picture's format can't be shown on cards. Try a JPEG or PNG."
  - Not WebP output: WebKitGTK cannot encode it (finding 2).
- **Audio** is stored as it is. A file the `<audio>` element reports it cannot play is still stored,
  with a warning, because the card frame engine may differ from the editor's.
- **Size limit: 20 MB per file**, checked in the UI before the call. Over it: "This file is larger
  than 20 MB. Pick a smaller one." The core keeps no limit (1.10).
- **Media is added when it is picked,** not when the note is added. The core names it
  (`<stem>-<hash>.<ext>`), and the field gets that name straight away, so the preview, the draft
  (decision 5) and a retry all use the real name. A note that is never added leaves an unused file.
  `delete_unused_media` exists and gets a button when a "Check media" screen exists (not planned yet).
  Unused files cost space now and, after Phase 4, sync bandwidth.

## Decision 4: the API this step adds

New `fc-api` methods, all thin over the core:

| Method | Input | Output |
|---|---|---|
| `getNoteTypes` | none | each live note type: `id`, `name`, `kind` (`standard` or `cloze`), `fields` (`id`, `name`), in sort order |
| `getTags` | none | every tag name, for suggestions |
| `findDuplicates` | `noteTypeId`, `value` | `noteIds` (empty when none) |
| `addNote` | `deckId`, `noteTypeId`, `fields` (field ID and value), `tags` | `noteId`, `cardCount`, `duplicates` |
| `addMedia` | `name` (the original file name), the bytes as the attachment | `name` (to use in the field), `new` |

- **`addNote` is one write, tags included.** Today it would be `add_note_to_deck` then
  `set_note_tags`, two transactions, so a failure between them leaves a note without its tags. The
  core gains `Collection::add_note_with(NewNote { deck, note_type, values, tags })`, which checks
  the tags first and writes note, cards and tags in one `write`. `add_note_to_deck` calls it with no
  tags. The CLI coverage table needs no new command (`fc add-note --tag` already exists and moves to
  the new function).
- Errors map to plain messages: no card would be made ("This note would make no cards. Fill in the
  front." or, for Cloze, "Add a cloze with the Cloze button."), deck or note type gone, a bad tag.
- `getMedia` (2.3) loads existing media for the draft. 3.1 still owns search and the browser's
  methods. This step builds the media methods STATUS gave to 3.1, and STATUS is updated to say so.

## Decision 5: the Add screen

Layout, compact (top to bottom; one column at every width, at most 46 rem as elsewhere):

```
 +-------------------------------+
 | Add                           |
 | Deck  [Polish::Verbs      v]  |   native <select>s: the system sheet on a phone
 | Type  [Basic              v]  |
 |                               |
 | Front                         |
 | [ kot                       ] |   one FieldEditor per field
 |  ! A note with this front     |   duplicate warning under the first field
 |    already exists.            |
 | Back                          |
 | [ cat  [img]                ] |
 | Tags                          |
 | [animals] [pl] [ add a tag ]  |   chips, suggestions from getTags
 +-------------------------------+
 | B  I  •  1.  [..]  🖼  ♪   Add |   BottomAction: toolbar + Add, above the keyboard
 +-------------------------------+
```

- **Remembered choices, per device** (`localStorage`, like the theme): the last deck and note type.
  First use: the Default deck and Basic. If a remembered one is gone, fall back the same way.
- **After Add:** the fields clear, deck, note type and **tags stay** (adding a batch of words with
  one tag is the common case), focus returns to the first field, and a status line says "Added (2
  cards)". There is no undo for an add in 2.4. Editing and deleting arrive with 3.2.
- **Duplicate warning:** under the first field, checked 400 ms after typing stops and again by
  `addNote`'s answer. It warns, never blocks: "A note with this front already exists." It names the
  first field by its own name.
- **The keyboard never hides the active field.** The shell already pads the content by the keyboard
  inset (ADR 0010). The screen adds, on focus and on every change of `--keyboard-inset`, a
  `scrollIntoView({ block: "nearest" })` of the focused editor, with a `scroll-margin-bottom` equal
  to the bottom action's height. Fields grow with their content (no inner scroll), so the caret line
  is what has to stay visible: the editor scrolls the caret into view on input (ProseMirror's
  `scrollIntoView` on each transaction, with `scrollMargin` set the same way).
- **Draft:** the deck, note type, field HTML and tags are saved per device in `localStorage` on every
  change (debounced) and restored when the screen opens. Cleared by Add and by a "Clear" button. This
  covers Android killing the app while the camera is open (see Findings), an accidental back, and a
  switch to Study and back. Media in a draft is already in the collection (decision 3), so a restored
  draft shows it through `getMedia`.
- **Changing note type** keeps values whose field names match, and keeps the rest of the old values in
  the draft until Add, so switching back does not lose them.
- **Leaving with unsaved text asks nothing.** The draft keeps it.
- Errors use `role="alert"`, the added line `role="status"` (ADR 0010 decision 5).
- Every control meets ADR 0010's tap target sizes. The toolbar scrolls sideways if it does not fit at
  200% text size.

## Consequences

- **New dependency:** ProseMirror (`prosemirror-model`, `-state`, `-view`, `-transform`,
  `-commands`, `-keymap`, `-history`, `-schema-list`; with `orderedmap`, `rope-sequence`,
  `w3c-keyname`), MIT, pinned to exact versions in `packages/ui`. About 65 KB gzipped added to an
  84 KB bundle.
- **CSP change** on the main window: `blob:` for `img-src` and a `media-src 'self' blob:`. The card
  frame's own CSP is unchanged.
- **No new bridge command, no new Tauri plugin, no manifest change.** One Kotlin addition: the
  start-up clean-up in `MainActivity`.
- **Field HTML written by the app is a fixed, small format** (decision 1). Imports (Phase 5) will
  bring richer HTML; the 3.2 fallback rule keeps it intact when edited.
- **Core change:** `Collection::add_note_with` and `NewNote`, so a note and its tags are written
  together.
- **Media moves into 2.4** from 3.1 (`addMedia`). Unused media from abandoned drafts accumulates until
  a "Check media" screen exists.
- The editor's behaviour on the phone's keyboards is the main risk and can only be judged by
  Anthony's hands. If it is bad, the fallback is option D for the phone only, which needs no change
  to the stored format.
- Images are capped at 1600 px. A user who wants full-resolution images (maps, diagrams) gets a
  setting later.

## Revisit if

- ProseMirror misbehaves on the phone's keyboards in ways a version bump does not fix.
- Imports (Phase 5) show most real notes trip the "cannot represent" rule: widen the schema
  (underline, colour, sub and superscript, tables) rather than send people to raw HTML.
- Files over 20 MB, or many files at once, are needed (a deck's worth of audio): then ADR 0002's file
  reference path (option B of decision 2) for that flow.
- Audio recording is wanted (permissions, and what card frames could then use).
- The Android picker turns out to lack Gallery or the camera on some device: option B of decision 2,
  for Android only.
- The 3.5 share target arrives: shared images should go through decision 3's processing.
- A web CSP is added in Phase 6: it needs the same `blob:` sources.

## Questions for Anthony

1. ProseMirror as a new dependency (about 65 KB gzipped), rather than our own `contenteditable`
   editor or Lexical.
2. `<input type="file">` on every platform, with no native picker code, and audio recording left out.
3. Photos re-encoded as JPEG at most 1600 px on the longest side, which also removes location data.
   Is 1600 px right for your cards?
4. Media is saved as soon as it is picked (an abandoned draft leaves an unused file).
5. Tags stay after Add, and the deck and note type are remembered per device.
6. The split into three PRs in the plan (`docs/plans/2.4-add-note.md`).

## Decisions on review

Anthony accepted the ADR on 2026-10-07 and agreed to all six questions as written:

1. ProseMirror as a new dependency.
2. `<input type="file">` on every platform, no native picker code, no audio recording.
3. JPEG re-encoded at most 1600 px on the longest side (location data removed).
4. Media saved as soon as it is picked.
5. Tags stay after Add; deck and note type remembered per device.
6. Three stacked PRs: 2.4a note API, 2.4b Add screen and editor, 2.4c media.

## Build notes (step 2.4a)

Built on `step/2.4a-note-api`, from `master` (the plan was already merged there). First of three PRs.
About 820 lines of code and tests (about 400 of them tests) and 130 lines of generated bindings,
against the plan's 700. No migration, no new dependency.

### What was built

- **`Collection::add_note_with(&NewNote { deck, note_type, values, tags })`** (`note/ops.rs`). The tags
  are checked and spelled first (`tags_for_new_note` in `tag/ops.rs`: `tag::check`, then the spelling the
  collection already uses, each tag once). Then the note, its cards and its tag rows are one `write`
  (`write_new_note_tags`, which takes the same `WriteTx`; no tag rule is duplicated). `add_note_to_deck`
  calls it with no tags. `NewNote` is exported from `fc_core::note`.
- **A tag error is its own `NoteError::Tag(TagError)`** (a wrapped `TagError::Collection` becomes
  `NoteError::Collection`). Its text is the tag error's text.
- **CLI:** `fc add-note --tag` uses `add_note_with`, so the tags are one write with the note. The
  `fc-cli` coverage table lists `add_note_with` under `add-note`. No other CLI change.
- **Five `fc-api` methods** (`crates/fc-api/src/notes.rs`): `getNoteTypes` (`id`, `name`, `kind` as
  `"standard" | "cloze"`, `fields` in order), `getTags` (every tag name, parents included),
  `findDuplicates`, `addNote` (`noteId`, `cardCount`, `duplicates`) and `addMedia` (the file as the
  request attachment; `name`, `new`). No or an empty attachment is `invalidInput`.
- **Error messages:** no cards ("This note would make no cards. Fill in the front." and, for Cloze,
  "Add a cloze with the Cloze button."), a missing deck or note type (`notFound`, "The deck or note
  type was deleted. Choose another and try again."), a bad tag (`invalidInput`, the tag error's text), a
  field of another note type (`invalidInput`).
- **Bindings** regenerated; `core-client` exports `NoteTypeList`, `NoteTypeSummary`, `FieldSummary`,
  `NoteTypeKind`, `TagList`, `AddNoteOutput`, `AddMediaOutput`.
- **Tests:** 7 core tests (`note/with_tags_tests.rs`): tags written with the note, no tags, spelling
  and dedupe, a bad tag writes nothing (digest, row counts and no event), no cards writes no tags,
  `NoteAdded` once, a `changes(All)` replay into another device has the tags and the same digest. 13
  `fc-api` tests through `dispatch`: every method, the error mappings, `addMedia` twice, no collection.

### Verified

`cargo xtask check` passes (656 core tests, 74 `fc-api` tests, the wasm build, 165 Vitest tests).

### Not verified

Nothing in this PR shows in a UI, so nothing was run in the app, the web client or the phone. The
methods are reachable from the web build (they are in the `always` list, and the wasm build compiles),
but no screen calls them yet.

### Deviations from the plan

- **`addMedia` is in `notes.rs`,** not `media.rs` or next to `getMedia`. `study.rs` is already 670
  lines. `parse_id` and `internal` in `study.rs` became `pub(crate)` so `notes.rs` can use them.
- **The same bytes under another file name** give a different name (the stem is kept) with `new: false`.
  The plan's "same bytes gives the same name" holds for the same file name, which is what the test uses.
  The bytes are still stored once.
- **`getTags` returns `{ tags: string[] }`** and `findDuplicates` returns `{ noteIds }`, wrapped in
  objects like the other methods. The ADR only said "every tag name".
- **Size:** about 820 lines against 700 (17% over, inside the 30% limit).
