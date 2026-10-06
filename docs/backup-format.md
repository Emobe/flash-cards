# Backup file format

The format of the files written by export and read by restore and import (step 1.13a). It is open:
anything that can read a zip file and JSON can read it, and this page is all you need. It is
versioned, and the app reads every version it has ever written.

- File extension: `.fcbackup` (any name works, the content is what counts).
- Current format version: **1**.
- Written by `Collection::export_backup`, read by `restore_backup`, `import_backup` and
  `read_manifest` in `fc_core::backup`. In the CLI: `fc export`, `fc restore`, `fc import`,
  `fc backup-info`.

## What it is

A backup is the **changes** of a collection (the data that sync moves between devices, ADR 0006 and
ADR 0008) written as text, plus the media bytes. It is not a copy of the SQLite file. So:

- It does not change when the database layout does. A collection migrates to a new layout; a backup
  of the old one still reads.
- A whole collection and a single deck use the same format.
- Reading one back uses the same rules as a merge, which have their own tests.

## The zip

A standard zip (deflate or stored; no encryption, no split archives). Three kinds of entry:

| Entry | Content |
| --- | --- |
| `manifest.json` | What the file is. Read first. |
| `changes.jsonl` | One JSON object per line: the registers and rows. |
| `media/<sha256>` | The bytes of one media file, named by the lowercase hex SHA-256 of the bytes (64 digits). One entry per file, stored without compression. |

Order does not matter. Entries this version does not know are ignored.

## `manifest.json`

```json
{
  "format": "fc-backup",
  "format_version": 1,
  "app_version": "0.0.0",
  "storage_version": 11,
  "created_ms": 1760000000000,
  "device": "0194e8a0-0000-7000-8000-000000000001",
  "scope": { "kind": "collection" },
  "history": true,
  "registers": 4,
  "rows": 0,
  "media_files": 0
}
```

| Key | Meaning |
| --- | --- |
| `format` | Always `"fc-backup"`. Anything else is not one of our files. |
| `format_version` | Integer. A reader that only knows lower numbers must refuse the file and say that the app needs updating. |
| `app_version` | The version of the core that wrote it. For people, not for decisions. |
| `storage_version` | The storage version of the collection it came from. For people, not for decisions. |
| `created_ms` | Unix milliseconds when it was written. |
| `device` | The device ID of the collection it came from. |
| `scope` | `{"kind": "collection"}`, or `{"kind": "deck", "id": "<deck ID>", "name": "<deck path>"}`. |
| `history` | Whether review events are included (see below). |
| `registers`, `rows`, `media_files` | How many of each the file holds. |

Keys this version does not know are ignored, so a later version may add keys without changing
`format_version`. A change that an older reader would misread raises `format_version`.

## `changes.jsonl`

UTF-8, one JSON object per line, `\n` between lines, blank lines ignored. Registers come first and
then rows, but a reader must not depend on it. Every object has a `kind`. A line with a `kind` the
reader does not know is damage.

### Values

A value is a SQLite value:

| JSON | SQLite |
| --- | --- |
| `null` | NULL |
| an integer | INTEGER (64-bit) |
| a number with a fraction or exponent (`0.9`, `3.0`, `1e-3`) | REAL |
| a string | TEXT |
| `{"blob": "<hex>"}` | BLOB, lowercase hexadecimal, `""` for an empty blob |
| `{"real": "inf"}` or `{"real": "-inf"}` | the REAL values JSON cannot write |

IDs are 16-byte blobs in the database. As values (for example the `note_type` of a note) they are
`{"blob": "<32 hex digits>"}`. As the `id` or `device` of a line they are canonical lower-case
hyphenated UUID text (`0194e8a0-0000-7000-8000-000000000001`).

### A register

```json
{"kind":"register","entity":"note","id":"<ID>","field":"deleted","value":0,"hlc":1760000000000000,"device":"<ID>"}
```

One value of one thing, with the clock it was written under (ADR 0006, sections 2 and 3):

| Key | Meaning |
| --- | --- |
| `entity` | The type of thing: `note`, `card`, `deck`, ... (table below). |
| `id` | The thing's ID. |
| `field` | The register: a column name, a field ID for a note's value (UUID text), or a tag as written for `note_tag`. |
| `value` | The value. |
| `hlc` | The hybrid logical clock: an integer, Unix milliseconds shifted left 16 bits, plus a 16-bit counter. |
| `device` | The device ID that wrote it. The higher `(hlc, device)` wins when two values meet. |

### A row

```json
{"kind":"row","entity":"card_event","id":"<ID>","columns":[["card",{"blob":"..."}],["kind","answer"]]}
```

One immutable row of an append-only table, which never changes and merges by union. `columns` is a
list of `[name, value]` pairs, in the order written. A name appears at most once.

### The entities

The entity names and register names are part of the sync format and never change (ADR 0006,
section 10). What each means is in that ADR and in the module of the core that owns it.

| Entity | Registers (fields) |
| --- | --- |
| `note_type` | `name`, `kind`, `css`, `sort_field`, `deleted` |
| `note_type_field` | `note_type`, `name`, `position`, `deleted` |
| `template` | `note_type`, `name`, `position`, `front`, `back`, `deleted` |
| `note` | `note_type`, `deleted`; and one register per field, named by the field ID, holding the text |
| `note_tag` | one register per tag, named by the tag; the value is `"1"`, or empty when taken off |
| `card` | `note`, `template`, `ordinal`, `deck`, `suspended`, `buried_until`, `deleted` |
| `deck` | `name`, `parent`, `options_preset`, `limits_include_subdecks`, `deleted` |
| `options_preset` | `name`, `new_per_day`, `reviews_per_day`, `learning_steps`, `desired_retention`, `relearning_steps`, `fsrs_parameters`, `space_siblings`, `deleted` |
| `collection_setting` | `key`, `value` |
| `saved_search` | `name`, `query`, `sort`, `mode`, `deleted` |
| `media_file` | `hash` (SHA-256 hex, which names the `media/` entry), `size`, `deleted` |
| `requirement` | `feature`, `active`: features a reader must understand to use the data |
| `card_event` (rows) | one answer or its undo: `card`, `kind`, `time_ms`, `utc_offset`, `device`, `previous`, `day`, `rating`, `duration_ms`, `preset`, `desired_retention`, `parameters`, `steps`, `state_before`, `state`, `step`, `stability`, `difficulty`, `due_day`, `due_ms`, `target` (ADR 0007) |
| `fsrs_parameter_set` (rows) | `values_f32`: 21 FSRS parameters that events point at |

A reader that meets an entity or a field it does not know **must keep it** and pass it on (ADR 0008,
part 6), not drop it. The core does: a collection restored from a file written by a newer version
keeps the data for that version.

Deletion is never removal: a deleted note, card or deck is a register `deleted` set to `1`, so the
trash can bring it back, and a backup holds the trash too.

## Scopes

**`collection`**: everything the collection has: every register (including the trash, the settings,
the saved searches and the built-in note types and decks), every media file whose bytes are on the
device, and, with `history`, every review event and parameter set. Without `history`, no event and
no parameter set.

**`deck`**: a deck and the decks inside it, with:

- their live cards, and the live notes of those cards (with their tags and field values);
- those notes' note types, with all their fields and templates;
- the decks' option presets;
- the media files that those notes and note types name;
- the `requirement` registers;
- with `history`, the events of those cards and the parameter sets those events use.

Deleted notes and cards, other decks, settings and saved searches are left out. A note whose cards
are in more than one deck comes with the cards that are in the exported decks only.

## Reading a file back

**Restore** (`restore_backup`, whole-collection files only; ADR 0006, section 11):

1. Anything that is not a valid file is refused, and nothing is changed: not a zip, no manifest, a
   `format` that is not ours, a newer `format_version`, a line that is not valid, a `media/` entry
   whose bytes do not match its name.
2. The collection gets a new device ID.
3. In one transaction: every register whose value differs from the file's is written again as a
   **new change** under the new device, so a sync sends it and it is not overwritten by the other
   devices' older writes. Rows (events, parameter sets) are added, never taken away. Media bytes
   missing here are stored.
4. Everything live in the collection that the file has no register for goes to the trash (`deleted`
   set), and so does a tag a note in the file did not have. Built-in rows that were never edited are
   never trashed. Settings the file does not have are left alone.
5. Cards are brought in line with their notes and note types, and the schedule of every card with a
   new event is folded again from all its events.

**Import** (`import_backup`, deck files and whole files): like a merge. A value from the file is
taken only when its clock is higher than the collection's, and it is then written as a new change.
Nothing is trashed and nothing is overwritten by an older value. Importing the same file again does
nothing.

Both finish with a report (what was written, what was already the same, what was trashed, media
added, anything that did not fit its column and was skipped).

## Limits of version 1

- The changes are read and written as a whole, so the memory needed grows with the collection
  (media is read one file at a time). Measured sizes are in ADR 0006, build notes, step 1.13a.
- A register that has no clock (it holds its column default) is not in a file, so a restore does not
  put such a register back to its default if it was changed since. This is rare: every ordinary
  write has a clock.
- A media file that the collection knows but whose bytes have not been downloaded is listed in its
  `media_file` registers and has no `media/` entry. The export reports how many.
- Anki `.apkg` import is a different feature (Phase 5) and has its own reader.

## A minimal file

A zip with two entries. `manifest.json`:

```json
{
  "format": "fc-backup",
  "format_version": 1,
  "app_version": "0.0.0",
  "storage_version": 11,
  "created_ms": 1760000000000,
  "device": "0194e8a0-0000-7000-8000-000000000001",
  "scope": { "kind": "collection" },
  "history": false,
  "registers": 4,
  "rows": 0,
  "media_files": 0
}
```

`changes.jsonl` (a deck called "Hand made", in four lines):

```jsonl
{"kind":"register","entity":"deck","id":"0194e8a0-0000-7000-8000-0000000000d1","field":"name","value":"Hand made","hlc":1760000000000000,"device":"0194e8a0-0000-7000-8000-000000000001"}
{"kind":"register","entity":"deck","id":"0194e8a0-0000-7000-8000-0000000000d1","field":"parent","value":{"blob":""},"hlc":1760000000000000,"device":"0194e8a0-0000-7000-8000-000000000001"}
{"kind":"register","entity":"deck","id":"0194e8a0-0000-7000-8000-0000000000d1","field":"options_preset","value":{"blob":""},"hlc":1760000000000000,"device":"0194e8a0-0000-7000-8000-000000000001"}
{"kind":"register","entity":"deck","id":"0194e8a0-0000-7000-8000-0000000000d1","field":"deleted","value":0,"hlc":1760000000000000,"device":"0194e8a0-0000-7000-8000-000000000001"}
```

The core's tests read exactly this file (`backup::tests`).

## Looking inside by hand

```sh
unzip -p backup.fcbackup manifest.json
unzip -p backup.fcbackup changes.jsonl | jq -c 'select(.entity == "note")' | head
unzip -p backup.fcbackup media/<sha256> > picture.bin
```
