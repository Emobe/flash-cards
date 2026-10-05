use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use rusqlite::Transaction;
use rusqlite::types::Value;

use super::*;
use crate::clock::{Host, ManualClock};
use crate::collection::{Collection, MIGRATIONS, Migration, Schema};
use crate::sync::{SYNCED_TABLES, check_schema, install_guard};

const START: i64 = 1_700_000_000_000;

fn host(clock: &Arc<ManualClock>) -> Host {
    Host {
        clock: clock.clone(),
        installation_id: Id::from_bytes(*b"installation-one"),
    }
}

fn collection() -> (Collection, Arc<ManualClock>) {
    let clock = Arc::new(ManualClock::new(START));
    (Collection::create(":memory:", host(&clock)).unwrap(), clock)
}

fn names(items: impl IntoIterator<Item = String>) -> Vec<String> {
    items.into_iter().collect()
}

fn field_names(collection: &Collection, note_type: Id) -> Vec<String> {
    let found = collection.note_type(note_type).unwrap().unwrap();
    names(found.fields.into_iter().map(|f| f.name))
}

fn template_names(collection: &Collection, note_type: Id) -> Vec<String> {
    let found = collection.note_type(note_type).unwrap().unwrap();
    names(found.templates.into_iter().map(|t| t.name))
}

fn text(value: &str) -> Value {
    Value::Text(value.to_owned())
}

/// Every register of every row of the three tables has a clock row.
fn assert_every_register_has_a_clock(collection: &Collection) {
    for table in [NOTE_TYPE, FIELD, TEMPLATE] {
        let ids: Vec<Id> = collection
            .conn
            .prepare(&format!("SELECT id FROM {}", table.table))
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        for id in ids {
            for register in table.registers {
                assert!(
                    collection
                        .register_clock(table.entity, id, register)
                        .unwrap()
                        .is_some(),
                    "{} {id} has no clock for {register}",
                    table.entity
                );
            }
        }
    }
}

// The built-ins.

#[test]
fn the_three_built_in_note_types_exist_in_a_new_collection() {
    let (collection, _) = collection();
    let types = collection.note_types().unwrap();
    let by_name: Vec<_> = types.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(by_name, ["Basic", "Basic and reversed", "Cloze"]);

    let basic = collection.note_type(builtin::basic()).unwrap().unwrap();
    assert_eq!(basic.kind, Kind::Standard);
    assert_eq!(
        names(basic.fields.iter().map(|f| f.name.clone())),
        ["Front", "Back"]
    );
    assert_eq!(basic.templates.len(), 1);
    assert_eq!(basic.templates[0].front, "{{Front}}");
    assert!(basic.templates[0].back.contains("{{FrontSide}}"));
    assert_eq!(basic.sort_field, Some(basic.fields[0].id));
    assert!(!basic.css.is_empty());

    let reversed = collection
        .note_type(builtin::basic_and_reversed())
        .unwrap()
        .unwrap();
    assert_eq!(reversed.kind, Kind::Standard);
    assert_eq!(
        names(reversed.templates.iter().map(|t| t.name.clone())),
        ["Card 1", "Card 2"]
    );
    assert_eq!(reversed.templates[1].front, "{{Back}}");

    let cloze = collection.note_type(builtin::cloze()).unwrap().unwrap();
    assert_eq!(cloze.kind, Kind::Cloze);
    assert_eq!(
        names(cloze.fields.iter().map(|f| f.name.clone())),
        ["Text", "Extra"]
    );
    assert_eq!(cloze.templates.len(), 1);
    assert_eq!(cloze.templates[0].front, "{{cloze:Text}}");
}

#[test]
fn built_in_ids_are_the_same_in_every_collection() {
    let (one, _) = collection();
    let (two, _) = collection();
    let ids = |c: &Collection| -> Vec<(Id, Vec<Id>, Vec<Id>)> {
        c.note_types()
            .unwrap()
            .into_iter()
            .map(|t| {
                (
                    t.id,
                    t.fields.iter().map(|f| f.id).collect(),
                    t.templates.iter().map(|x| x.id).collect(),
                )
            })
            .collect()
    };
    assert_eq!(ids(&one), ids(&two));
    // Fixed forever: a change here would split every collection's built-ins from the others.
    assert_eq!(
        builtin::basic().to_string(),
        "258ea705-9fd3-5924-a095-1a3e87540656"
    );
    assert_eq!(
        builtin::basic_and_reversed().to_string(),
        "5c260f66-b578-5ae8-bb39-31985c711e0b"
    );
    assert_eq!(
        builtin::cloze().to_string(),
        "e4fdaba7-2be6-5421-aa83-f72819eff2ca"
    );
    assert_ne!(builtin::basic(), builtin::basic_and_reversed());
    assert_ne!(builtin::basic(), builtin::cloze());
}

#[test]
fn built_ins_carry_the_lowest_clock_and_are_not_pushed() {
    let (collection, _) = collection();
    let clock = collection
        .register_clock(NOTE_TYPE.entity, builtin::basic(), "name")
        .unwrap()
        .unwrap();
    assert_eq!(clock.hlc, crate::sync::Hlc::ZERO);
    assert_eq!(clock.device, Id::from_bytes([0; 16]));
    assert!(clock.pushed);
    assert_every_register_has_a_clock(&collection);
}

#[test]
fn a_real_edit_of_a_built_in_beats_its_seed_and_is_pushed_later() {
    let (collection, _) = collection();
    collection
        .rename_note_type(builtin::basic(), "Vocabulary")
        .unwrap();
    let clock = collection
        .register_clock(NOTE_TYPE.entity, builtin::basic(), "name")
        .unwrap()
        .unwrap();
    assert!(clock.hlc > crate::sync::Hlc::ZERO);
    assert!(!clock.pushed);
    // The other registers keep the seed clock, so only what was edited needs pushing.
    let css = collection
        .register_clock(NOTE_TYPE.entity, builtin::basic(), "css")
        .unwrap()
        .unwrap();
    assert!(css.pushed);
}

#[test]
fn a_new_collection_has_a_valid_schema_with_the_note_type_tables_classified() {
    let (collection, _) = collection();
    check_schema(&collection.conn, SYNCED_TABLES).unwrap();
    for table in [NOTE_TYPE, FIELD, TEMPLATE] {
        assert!(SYNCED_TABLES.iter().any(|t| t.table == table.table));
    }
}

#[test]
fn a_version_2_collection_gets_the_built_ins_and_keeps_its_data() {
    const V2: Schema = Schema {
        migrations: &[MIGRATIONS[0], MIGRATIONS[1]],
        tables: &[SYNCED_TABLES[0]],
    };
    let path = TempDb::new();
    let clock = Arc::new(ManualClock::new(START));
    let old = Collection::create_with(path.path(), V2, host(&clock)).unwrap();
    old.require_feature("kept").unwrap();
    old.conn
        .execute("INSERT INTO meta VALUES ('word', 'czesc')", [])
        .unwrap();
    old.close().unwrap();

    let upgraded = Collection::open(path.path(), host(&clock)).unwrap();
    let info = upgraded.info().unwrap();
    assert_eq!(info.schema_version, info.supported_schema_version);
    assert_eq!(upgraded.note_types().unwrap().len(), 3);
    assert_eq!(upgraded.unsupported_features().unwrap(), ["kept"]);
    let word: String = upgraded
        .conn
        .query_row("SELECT value FROM meta WHERE key = 'word'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(word, "czesc");
    check_schema(&upgraded.conn, SYNCED_TABLES).unwrap();
    assert_every_register_has_a_clock(&upgraded);
}

#[test]
fn built_in_content_is_the_same_whichever_way_it_was_created() {
    let (new, _) = collection();
    const V2: Schema = Schema {
        migrations: &[MIGRATIONS[0], MIGRATIONS[1]],
        tables: &[SYNCED_TABLES[0]],
    };
    let path = TempDb::new();
    let clock = Arc::new(ManualClock::new(START));
    Collection::create_with(path.path(), V2, host(&clock))
        .unwrap()
        .close()
        .unwrap();
    let upgraded = Collection::open(path.path(), host(&clock)).unwrap();
    assert_eq!(new.note_types().unwrap(), upgraded.note_types().unwrap());
}

#[test]
fn built_in_positions_are_valid_and_in_order() {
    for position in builtin::POSITIONS {
        assert!(position::is_valid(position));
    }
    assert!(builtin::POSITIONS.windows(2).all(|w| w[0] < w[1]));
}

// Note types.

#[test]
fn a_note_type_can_be_created_renamed_and_deleted() {
    let (collection, _) = collection();
    let id = collection
        .create_note_type("  Polish words  ", Kind::Standard)
        .unwrap();
    let created = collection.note_type(id).unwrap().unwrap();
    assert_eq!(created.name, "Polish words");
    assert_eq!(
        names(created.fields.iter().map(|f| f.name.clone())),
        ["Front", "Back"]
    );
    assert_eq!(created.templates.len(), 1);
    assert_ne!(id, builtin::basic());

    collection.rename_note_type(id, "Vocabulary").unwrap();
    assert_eq!(
        collection.note_type(id).unwrap().unwrap().name,
        "Vocabulary"
    );
    assert_eq!(collection.note_types().unwrap().len(), 4);

    collection.delete_note_type(id).unwrap();
    assert_eq!(collection.note_types().unwrap().len(), 3);
    let trash = collection.deleted_note_types().unwrap();
    assert_eq!(trash.len(), 1);
    assert_eq!(trash[0].id, id);
    assert!(trash[0].deleted);
    // Its fields and templates went with it, unchanged.
    assert_eq!(trash[0].fields.len(), 2);
    assert_eq!(trash[0].templates.len(), 1);

    collection.restore_note_type(id).unwrap();
    assert_eq!(collection.note_types().unwrap().len(), 4);
    assert!(collection.deleted_note_types().unwrap().is_empty());
    assert_every_register_has_a_clock(&collection);
}

#[test]
fn a_cloze_note_type_starts_with_a_cloze_template() {
    let (collection, _) = collection();
    let id = collection
        .create_note_type("Sentences", Kind::Cloze)
        .unwrap();
    let created = collection.note_type(id).unwrap().unwrap();
    assert_eq!(created.kind, Kind::Cloze);
    assert_eq!(
        names(created.fields.iter().map(|f| f.name.clone())),
        ["Text", "Extra"]
    );
    assert_eq!(created.templates[0].front, "{{cloze:Text}}");
}

#[test]
fn empty_names_are_refused_and_nothing_is_written() {
    let (collection, _) = collection();
    assert_eq!(
        collection.create_note_type("   ", Kind::Standard),
        Err(NoteTypeError::EmptyName)
    );
    assert_eq!(
        collection.rename_note_type(builtin::basic(), ""),
        Err(NoteTypeError::EmptyName)
    );
    assert_eq!(collection.note_types().unwrap().len(), 3);
    assert!(
        collection
            .register_clock(NOTE_TYPE.entity, builtin::basic(), "name")
            .unwrap()
            .unwrap()
            .pushed
    );
}

#[test]
fn a_deleted_note_type_cannot_be_edited_but_deleting_and_restoring_are_repeatable() {
    let (collection, _) = collection();
    let id = collection.create_note_type("Temp", Kind::Standard).unwrap();
    collection.delete_note_type(id).unwrap();
    collection.delete_note_type(id).unwrap();
    assert_eq!(
        collection.rename_note_type(id, "X"),
        Err(NoteTypeError::NotFound)
    );
    assert_eq!(
        collection.add_field(id, "Extra"),
        Err(NoteTypeError::NotFound)
    );
    collection.restore_note_type(id).unwrap();
    collection.restore_note_type(id).unwrap();
    let missing = Id::from_bytes([9; 16]);
    assert_eq!(
        collection.delete_note_type(missing),
        Err(NoteTypeError::NotFound)
    );
    assert_eq!(
        collection.restore_note_type(missing),
        Err(NoteTypeError::NotFound)
    );
}

#[test]
fn css_is_stored_as_written() {
    let (collection, _) = collection();
    collection
        .set_note_type_css(builtin::basic(), ".card { color: red; }")
        .unwrap();
    assert_eq!(
        collection.note_type(builtin::basic()).unwrap().unwrap().css,
        ".card { color: red; }"
    );
}

// Fields.

#[test]
fn fields_are_added_at_the_end_renamed_and_reordered() {
    let (collection, _) = collection();
    let nt = collection
        .create_note_type("Words", Kind::Standard)
        .unwrap();
    let extra = collection.add_field(nt, "Example").unwrap();
    let notes = collection.add_field(nt, "Notes").unwrap();
    assert_eq!(
        field_names(&collection, nt),
        ["Front", "Back", "Example", "Notes"]
    );

    collection.rename_field(nt, extra, "Sentence").unwrap();
    assert_eq!(
        field_names(&collection, nt),
        ["Front", "Back", "Sentence", "Notes"]
    );

    collection.move_field(nt, notes, 0).unwrap();
    assert_eq!(
        field_names(&collection, nt),
        ["Notes", "Front", "Back", "Sentence"]
    );
    collection.move_field(nt, notes, 99).unwrap();
    assert_eq!(
        field_names(&collection, nt),
        ["Front", "Back", "Sentence", "Notes"]
    );
    collection.move_field(nt, extra, 1).unwrap();
    assert_eq!(
        field_names(&collection, nt),
        ["Front", "Sentence", "Back", "Notes"]
    );
    assert_every_register_has_a_clock(&collection);
}

#[test]
fn field_ids_survive_rename_and_reorder() {
    let (collection, _) = collection();
    let before = collection.note_type(builtin::basic()).unwrap().unwrap();
    let (front, back) = (before.fields[0].id, before.fields[1].id);
    collection
        .rename_field(builtin::basic(), front, "Polish")
        .unwrap();
    collection.move_field(builtin::basic(), front, 1).unwrap();
    let after = collection.note_type(builtin::basic()).unwrap().unwrap();
    assert_eq!(after.fields[0].id, back);
    assert_eq!(after.fields[1].id, front);
    assert_eq!(after.fields[1].name, "Polish");
}

#[test]
fn moving_a_field_writes_only_that_field() {
    let (collection, _) = collection();
    let nt = collection
        .create_note_type("Words", Kind::Standard)
        .unwrap();
    for name in ["A", "B", "C"] {
        collection.add_field(nt, name).unwrap();
    }
    let fields = collection.note_type(nt).unwrap().unwrap().fields;
    let clocks = |collection: &Collection| -> Vec<_> {
        fields
            .iter()
            .map(|f| {
                collection
                    .register_clock(FIELD.entity, f.id, "position")
                    .unwrap()
                    .unwrap()
                    .hlc
            })
            .collect()
    };
    let before = clocks(&collection);
    collection.move_field(nt, fields[4].id, 1).unwrap();
    let after = clocks(&collection);
    let changed: Vec<usize> = (0..fields.len())
        .filter(|&i| before[i] != after[i])
        .collect();
    assert_eq!(changed, [4]);
}

#[test]
fn moving_a_field_to_where_it_is_writes_nothing() {
    let (collection, _) = collection();
    let nt = collection
        .create_note_type("Words", Kind::Standard)
        .unwrap();
    let fields = collection.note_type(nt).unwrap().unwrap().fields;
    let before = collection
        .register_clock(FIELD.entity, fields[0].id, "position")
        .unwrap();
    collection.move_field(nt, fields[0].id, 0).unwrap();
    assert_eq!(
        collection
            .register_clock(FIELD.entity, fields[0].id, "position")
            .unwrap(),
        before
    );
}

#[test]
fn field_names_must_be_unique_ignoring_case_within_a_note_type() {
    let (collection, _) = collection();
    let nt = collection
        .create_note_type("Words", Kind::Standard)
        .unwrap();
    assert_eq!(
        collection.add_field(nt, "front"),
        Err(NoteTypeError::NameTaken("front".to_owned()))
    );
    let fields = collection.note_type(nt).unwrap().unwrap().fields;
    assert_eq!(
        collection.rename_field(nt, fields[1].id, "FRONT"),
        Err(NoteTypeError::NameTaken("FRONT".to_owned()))
    );
    // The same name in another note type, and changing only the case of its own, are fine.
    collection.rename_field(nt, fields[0].id, "FRONT").unwrap();
    let other = collection
        .create_note_type("Others", Kind::Standard)
        .unwrap();
    collection.add_field(other, "Notes").unwrap();
    collection.add_field(nt, "Notes").unwrap();
}

#[test]
fn a_field_cannot_be_used_through_another_note_type() {
    let (collection, _) = collection();
    let nt = collection
        .create_note_type("Words", Kind::Standard)
        .unwrap();
    let foreign = collection
        .note_type(builtin::basic())
        .unwrap()
        .unwrap()
        .fields[0]
        .id;
    assert_eq!(
        collection.rename_field(nt, foreign, "X"),
        Err(NoteTypeError::NotFound)
    );
    assert_eq!(
        collection.move_field(nt, foreign, 0),
        Err(NoteTypeError::NotFound)
    );
    assert_eq!(
        collection.remove_field(nt, foreign),
        Err(NoteTypeError::NotFound)
    );
    assert_eq!(
        collection.restore_field(nt, foreign),
        Err(NoteTypeError::NotFound)
    );
    assert_eq!(
        collection.set_sort_field(nt, foreign),
        Err(NoteTypeError::NotFound)
    );
}

#[test]
fn a_removed_field_is_hidden_and_can_be_restored_in_its_old_place() {
    let (collection, _) = collection();
    let nt = collection
        .create_note_type("Words", Kind::Standard)
        .unwrap();
    let example = collection.add_field(nt, "Example").unwrap();
    collection.remove_field(nt, example).unwrap();
    assert_eq!(field_names(&collection, nt), ["Front", "Back"]);
    let removed = collection.removed_fields(nt).unwrap();
    assert_eq!(
        removed,
        [Field {
            id: example,
            name: "Example".to_owned()
        }]
    );
    // Removing twice is harmless.
    collection.remove_field(nt, example).unwrap();

    let middle = collection.add_field(nt, "Audio").unwrap();
    collection.move_field(nt, middle, 1).unwrap();
    collection.restore_field(nt, example).unwrap();
    assert_eq!(
        field_names(&collection, nt),
        ["Front", "Audio", "Back", "Example"]
    );
    assert!(collection.removed_fields(nt).unwrap().is_empty());
}

#[test]
fn the_last_field_cannot_be_removed() {
    let (collection, _) = collection();
    let nt = collection
        .create_note_type("Words", Kind::Standard)
        .unwrap();
    let fields = collection.note_type(nt).unwrap().unwrap().fields;
    collection.remove_field(nt, fields[0].id).unwrap();
    assert_eq!(
        collection.remove_field(nt, fields[1].id),
        Err(NoteTypeError::LastField)
    );
    assert_eq!(field_names(&collection, nt), ["Back"]);
}

#[test]
fn a_field_cannot_come_back_under_a_name_that_is_now_taken() {
    let (collection, _) = collection();
    let nt = collection
        .create_note_type("Words", Kind::Standard)
        .unwrap();
    let example = collection.add_field(nt, "Example").unwrap();
    collection.remove_field(nt, example).unwrap();
    collection.add_field(nt, "example").unwrap();
    assert_eq!(
        collection.restore_field(nt, example),
        Err(NoteTypeError::NameTaken("Example".to_owned()))
    );
    collection
        .rename_field(
            nt,
            collection.note_type(nt).unwrap().unwrap().fields[2].id,
            "Other",
        )
        .unwrap();
    collection.restore_field(nt, example).unwrap();
}

#[test]
fn the_sort_field_is_chosen_and_falls_back_when_it_is_removed() {
    let (collection, _) = collection();
    let nt = collection
        .create_note_type("Words", Kind::Standard)
        .unwrap();
    let fields = collection.note_type(nt).unwrap().unwrap().fields;
    assert_eq!(
        collection.note_type(nt).unwrap().unwrap().sort_field,
        Some(fields[0].id)
    );
    collection.set_sort_field(nt, fields[1].id).unwrap();
    assert_eq!(
        collection.note_type(nt).unwrap().unwrap().sort_field,
        Some(fields[1].id)
    );
    collection.remove_field(nt, fields[1].id).unwrap();
    assert_eq!(
        collection.note_type(nt).unwrap().unwrap().sort_field,
        Some(fields[0].id)
    );
    // The choice is kept, so restoring the field makes it the sort field again.
    collection.restore_field(nt, fields[1].id).unwrap();
    assert_eq!(
        collection.note_type(nt).unwrap().unwrap().sort_field,
        Some(fields[1].id)
    );
    // A removed field cannot be chosen.
    collection.remove_field(nt, fields[0].id).unwrap();
    assert_eq!(
        collection.set_sort_field(nt, fields[0].id),
        Err(NoteTypeError::NotFound)
    );
}

// Templates.

#[test]
fn templates_are_added_edited_renamed_and_reordered() {
    let (collection, _) = collection();
    let nt = collection
        .create_note_type("Words", Kind::Standard)
        .unwrap();
    let reverse = collection
        .add_template(nt, "Reverse", "{{Back}}", "{{FrontSide}}{{Front}}")
        .unwrap();
    assert_eq!(template_names(&collection, nt), ["Card 1", "Reverse"]);

    collection.rename_template(nt, reverse, "Card 2").unwrap();
    collection
        .set_template_text(nt, reverse, "{{Back}}!", "{{Front}}!")
        .unwrap();
    let found = collection.note_type(nt).unwrap().unwrap();
    assert_eq!(found.templates[1].name, "Card 2");
    assert_eq!(found.templates[1].front, "{{Back}}!");
    assert_eq!(found.templates[1].back, "{{Front}}!");

    collection.move_template(nt, reverse, 0).unwrap();
    assert_eq!(template_names(&collection, nt), ["Card 2", "Card 1"]);
    assert_every_register_has_a_clock(&collection);
}

#[test]
fn template_names_must_be_unique_within_a_note_type() {
    let (collection, _) = collection();
    let nt = collection
        .create_note_type("Words", Kind::Standard)
        .unwrap();
    assert_eq!(
        collection.add_template(nt, "card 1", "", ""),
        Err(NoteTypeError::NameTaken("card 1".to_owned()))
    );
    assert_eq!(
        collection.add_template(nt, " ", "", ""),
        Err(NoteTypeError::EmptyName)
    );
}

#[test]
fn a_removed_template_is_restored_with_its_id_and_text() {
    let (collection, _) = collection();
    let nt = collection
        .create_note_type("Words", Kind::Standard)
        .unwrap();
    let reverse = collection
        .add_template(nt, "Reverse", "{{Back}}", "{{Front}}")
        .unwrap();
    collection.remove_template(nt, reverse).unwrap();
    assert_eq!(template_names(&collection, nt), ["Card 1"]);
    let removed = collection.removed_templates(nt).unwrap();
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].id, reverse);
    assert_eq!(removed[0].front, "{{Back}}");

    collection.restore_template(nt, reverse).unwrap();
    let found = collection.note_type(nt).unwrap().unwrap();
    assert_eq!(found.templates[1].id, reverse);
    assert_eq!(found.templates[1].back, "{{Front}}");
}

#[test]
fn the_last_template_cannot_be_removed() {
    let (collection, _) = collection();
    let nt = collection
        .create_note_type("Words", Kind::Standard)
        .unwrap();
    let only = collection.note_type(nt).unwrap().unwrap().templates[0].id;
    assert_eq!(
        collection.remove_template(nt, only),
        Err(NoteTypeError::LastTemplate)
    );
    assert_eq!(template_names(&collection, nt), ["Card 1"]);
}

#[test]
fn a_cloze_note_type_keeps_exactly_one_template() {
    let (collection, _) = collection();
    let only = collection
        .note_type(builtin::cloze())
        .unwrap()
        .unwrap()
        .templates[0]
        .id;
    assert_eq!(
        collection.add_template(builtin::cloze(), "Second", "", ""),
        Err(NoteTypeError::ClozeTemplate)
    );
    assert_eq!(
        collection.remove_template(builtin::cloze(), only),
        Err(NoteTypeError::LastTemplate)
    );
    // Editing its text and name is fine.
    collection
        .set_template_text(builtin::cloze(), only, "{{cloze:Text}}!", "x")
        .unwrap();
    collection
        .rename_template(builtin::cloze(), only, "Fill in")
        .unwrap();
}

// Positions after a merge.

#[test]
fn equal_positions_are_ordered_by_id_and_a_move_renumbers_when_there_is_no_room() {
    let (collection, _) = collection();
    let nt = collection
        .create_note_type("Words", Kind::Standard)
        .unwrap();
    let a = collection.add_field(nt, "A").unwrap();
    let b = collection.add_field(nt, "B").unwrap();
    // As if two devices had put different fields in the same place.
    collection
        .write(|w| {
            for id in [a, b] {
                w.set(FIELD.entity, id, "position", text("m"))?;
            }
            Ok(())
        })
        .unwrap();
    let tied = collection.note_type(nt).unwrap().unwrap().fields;
    let tied_ids: Vec<Id> = tied
        .iter()
        .map(|f| f.id)
        .filter(|id| *id == a || *id == b)
        .collect();
    let mut by_id = vec![a, b];
    by_id.sort();
    assert_eq!(tied_ids, by_id, "ties are broken by ID");

    let front = tied[0].id;
    let before: Vec<Id> = tied.iter().map(|f| f.id).collect();
    // Moving into the tie has no room, so the list is renumbered. The order is what was asked.
    collection.move_field(nt, a, 3).unwrap();
    let after = collection.note_type(nt).unwrap().unwrap().fields;
    let mut expected: Vec<Id> = before.into_iter().filter(|id| *id != a).collect();
    expected.insert(3, a);
    assert_eq!(after.iter().map(|f| f.id).collect::<Vec<_>>(), expected);
    assert_eq!(after[0].id, front);
    let positions: Vec<String> = collection
        .conn
        .prepare("SELECT position FROM note_type_field WHERE note_type = ?1 AND deleted = 0 ORDER BY position, id")
        .unwrap()
        .query_map([nt], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(positions.windows(2).all(|w| w[0] < w[1]), "no ties left");
    assert_every_register_has_a_clock(&collection);
}

#[test]
fn a_position_that_arrived_malformed_is_repaired_by_the_next_change() {
    let (collection, _) = collection();
    let nt = collection
        .create_note_type("Words", Kind::Standard)
        .unwrap();
    let fields = collection.note_type(nt).unwrap().unwrap().fields;
    collection
        .write(|w| w.set(FIELD.entity, fields[1].id, "position", text("")))
        .unwrap();
    // Empty sorts first, so the order is Back, Front. Adding a field must still work.
    assert_eq!(field_names(&collection, nt), ["Back", "Front"]);
    collection.add_field(nt, "Example").unwrap();
    assert_eq!(field_names(&collection, nt), ["Back", "Front", "Example"]);
}

#[test]
fn rows_without_a_live_note_type_are_ignored() {
    let (collection, _) = collection();
    let orphan = Id::from_bytes([7; 16]);
    collection
        .write(|w| {
            w.insert(
                FIELD.entity,
                orphan,
                vec![
                    ("note_type", Value::Blob(vec![1; 16])),
                    ("name", text("Orphan")),
                    ("position", text("V")),
                    ("deleted", Value::Integer(0)),
                ],
            )
        })
        .unwrap();
    for nt in collection.note_types().unwrap() {
        assert!(nt.fields.iter().all(|f| f.id != orphan));
    }
}

#[test]
fn an_unknown_kind_reads_as_standard() {
    let (collection, _) = collection();
    let nt = collection
        .create_note_type("Future", Kind::Standard)
        .unwrap();
    collection
        .write(|w| w.set(NOTE_TYPE.entity, nt, "kind", text("hypercard")))
        .unwrap();
    assert_eq!(
        collection.note_type(nt).unwrap().unwrap().kind,
        Kind::Standard
    );
}

// The write path and the guards.

#[test]
fn raw_sql_cannot_write_the_note_type_tables() {
    let (collection, _) = collection();
    for sql in [
        "UPDATE note_type SET name = 'x'",
        "INSERT INTO note_type (id) VALUES (x'00')",
        "UPDATE note_type_field SET name = 'x'",
        "DELETE FROM note_type_field",
        "UPDATE template SET front = 'x'",
        "DELETE FROM template",
        "DELETE FROM note_type",
    ] {
        assert!(
            collection.conn.execute(sql, []).is_err(),
            "{sql} was allowed"
        );
    }
}

#[test]
fn every_operation_leaves_a_clock_for_every_register() {
    let (collection, _) = collection();
    let nt = collection
        .create_note_type("Words", Kind::Standard)
        .unwrap();
    let field = collection.add_field(nt, "Example").unwrap();
    collection.rename_field(nt, field, "Sentence").unwrap();
    collection.move_field(nt, field, 0).unwrap();
    collection.remove_field(nt, field).unwrap();
    collection.restore_field(nt, field).unwrap();
    let template = collection
        .add_template(nt, "Reverse", "{{Back}}", "{{Front}}")
        .unwrap();
    collection.remove_template(nt, template).unwrap();
    collection.restore_template(nt, template).unwrap();
    collection.set_note_type_css(nt, "x").unwrap();
    collection.delete_note_type(nt).unwrap();
    collection.restore_note_type(nt).unwrap();
    assert_every_register_has_a_clock(&collection);
    check_schema(&collection.conn, SYNCED_TABLES).unwrap();
}

#[test]
fn an_edit_is_stamped_after_everything_the_device_has_seen() {
    let (collection, clock) = collection();
    let nt = collection
        .create_note_type("Words", Kind::Standard)
        .unwrap();
    // The device's clock goes backwards, as when the user changes it.
    clock.set(START - 3_600_000);
    collection.rename_note_type(nt, "Renamed").unwrap();
    let created = collection
        .register_clock(NOTE_TYPE.entity, nt, "kind")
        .unwrap()
        .unwrap();
    let renamed = collection
        .register_clock(NOTE_TYPE.entity, nt, "name")
        .unwrap()
        .unwrap();
    assert!(renamed.hlc > created.hlc);
}

// What changing a note type does to notes. Notes arrive in step 1.3, so these tests use a stand-in
// table that stores values the way notes will: one row per (note, field ID). Step 1.3 repeats them
// with real notes. What they pin down is that no note type operation touches note data, and that
// what the person sees of a note follows the note type's live fields.

const STAND_IN: SyncedTable = SyncedTable {
    entity: "stand_in_value",
    table: "stand_in_value",
    registers: &["note", "field", "value"],
};

fn stand_in_migration(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE stand_in_value (
            id BLOB PRIMARY KEY NOT NULL,
            note BLOB NOT NULL DEFAULT x'',
            field BLOB NOT NULL DEFAULT x'',
            value TEXT NOT NULL DEFAULT ''
        ) WITHOUT ROWID",
    )?;
    install_guard(tx, &STAND_IN)
}

const WITH_STAND_IN: Schema = Schema {
    migrations: &[
        MIGRATIONS[0],
        MIGRATIONS[1],
        MIGRATIONS[2],
        Migration {
            version: 4,
            apply: stand_in_migration,
        },
    ],
    tables: &[
        SYNCED_TABLES[0],
        SYNCED_TABLES[1],
        SYNCED_TABLES[2],
        SYNCED_TABLES[3],
        STAND_IN,
    ],
};

/// A collection, and a note of the built-in Basic type with one value per field.
fn with_a_note() -> (Collection, Id) {
    let clock = Arc::new(ManualClock::new(START));
    let collection = Collection::create_with(":memory:", WITH_STAND_IN, host(&clock)).unwrap();
    let note = Id::from_bytes([5; 16]);
    let fields = collection
        .note_type(builtin::basic())
        .unwrap()
        .unwrap()
        .fields;
    for (field, value) in fields.iter().zip(["pies", "dog"]) {
        collection
            .write(|w| {
                let id = w.new_id()?;
                w.insert(
                    STAND_IN.entity,
                    id,
                    vec![
                        ("note", Value::Blob(note.as_bytes().to_vec())),
                        ("field", Value::Blob(field.id.as_bytes().to_vec())),
                        ("value", text(value)),
                    ],
                )
            })
            .unwrap();
    }
    (collection, note)
}

/// Everything stored for notes, as it is in the table.
fn stored(collection: &Collection) -> Vec<(Vec<u8>, String)> {
    collection
        .conn
        .prepare("SELECT field, value FROM stand_in_value ORDER BY value")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

/// What a note shows: its values for the live fields of its note type, in field order.
fn shown(collection: &Collection, note_type: Id) -> Vec<(String, String)> {
    let found = collection.note_type(note_type).unwrap().unwrap();
    found
        .fields
        .iter()
        .filter_map(|field| {
            collection
                .conn
                .query_row(
                    "SELECT value FROM stand_in_value WHERE field = ?1",
                    [field.id],
                    |row| row.get::<_, String>(0),
                )
                .ok()
                .map(|value| (field.name.clone(), value))
        })
        .collect()
}

fn pair(name: &str, value: &str) -> (String, String) {
    (name.to_owned(), value.to_owned())
}

#[test]
fn renaming_or_reordering_a_field_leaves_note_values_untouched_and_attached_to_it() {
    let (collection, _) = with_a_note();
    let before = stored(&collection);
    let fields = collection
        .note_type(builtin::basic())
        .unwrap()
        .unwrap()
        .fields;
    collection
        .rename_field(builtin::basic(), fields[0].id, "Polish")
        .unwrap();
    collection
        .move_field(builtin::basic(), fields[0].id, 1)
        .unwrap();
    assert_eq!(stored(&collection), before);
    assert_eq!(
        shown(&collection, builtin::basic()),
        [pair("Back", "dog"), pair("Polish", "pies")]
    );
}

#[test]
fn removing_a_field_hides_its_values_and_restoring_it_brings_them_back() {
    let (collection, _) = with_a_note();
    let before = stored(&collection);
    let fields = collection
        .note_type(builtin::basic())
        .unwrap()
        .unwrap()
        .fields;
    collection
        .remove_field(builtin::basic(), fields[1].id)
        .unwrap();
    assert_eq!(stored(&collection), before, "the value is kept");
    assert_eq!(
        shown(&collection, builtin::basic()),
        [pair("Front", "pies")]
    );
    collection
        .restore_field(builtin::basic(), fields[1].id)
        .unwrap();
    assert_eq!(
        shown(&collection, builtin::basic()),
        [pair("Front", "pies"), pair("Back", "dog")]
    );
}

#[test]
fn a_field_added_later_starts_empty_and_a_removed_name_can_be_reused_for_a_new_field() {
    let (collection, _) = with_a_note();
    let fields = collection
        .note_type(builtin::basic())
        .unwrap()
        .unwrap()
        .fields;
    collection
        .remove_field(builtin::basic(), fields[1].id)
        .unwrap();
    let new_back = collection.add_field(builtin::basic(), "Back").unwrap();
    assert_ne!(new_back, fields[1].id);
    // The new field is a different field: it does not pick up the removed one's value.
    assert_eq!(
        shown(&collection, builtin::basic()),
        [pair("Front", "pies")]
    );
    // Restoring the old one now collides with the new name, which the person resolves.
    assert_eq!(
        collection.restore_field(builtin::basic(), fields[1].id),
        Err(NoteTypeError::NameTaken("Back".to_owned()))
    );
}

#[test]
fn deleting_and_restoring_a_note_type_leaves_note_values_untouched() {
    let (collection, _) = with_a_note();
    let before = stored(&collection);
    collection.delete_note_type(builtin::basic()).unwrap();
    assert_eq!(stored(&collection), before);
    assert!(
        collection
            .note_types()
            .unwrap()
            .iter()
            .all(|t| t.id != builtin::basic())
    );
    collection.restore_note_type(builtin::basic()).unwrap();
    assert_eq!(stored(&collection), before);
    assert_eq!(
        shown(&collection, builtin::basic()),
        [pair("Front", "pies"), pair("Back", "dog")]
    );
}

#[test]
fn removing_and_restoring_a_template_changes_no_note_value() {
    let (collection, _) = with_a_note();
    let before = stored(&collection);
    let reversed = collection
        .note_type(builtin::basic_and_reversed())
        .unwrap()
        .unwrap();
    let second = reversed.templates[1].id;
    collection
        .remove_template(builtin::basic_and_reversed(), second)
        .unwrap();
    collection
        .restore_template(builtin::basic_and_reversed(), second)
        .unwrap();
    // The template keeps its ID, which is what lets step 1.3 give a returning card its old ID.
    let after = collection
        .note_type(builtin::basic_and_reversed())
        .unwrap()
        .unwrap();
    assert_eq!(after.templates[1].id, second);
    assert_eq!(stored(&collection), before);
}

/// A database file in the temp directory, removed when dropped.
struct TempDb(PathBuf);

impl TempDb {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        Self(std::env::temp_dir().join(format!("fc-notetype-{}-{n}.db", std::process::id())))
    }

    fn path(&self) -> &str {
        self.0.to_str().unwrap()
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        let _ = std::fs::remove_file(format!("{}-journal", self.path()));
    }
}
