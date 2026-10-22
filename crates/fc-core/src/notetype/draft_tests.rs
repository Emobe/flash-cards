//! Saving a note type as a draft (ADR 0015, step 3.4a).

use super::*;
use crate::collection::Collection;
use crate::sync::Selection;
use crate::sync::merge_tests::{Dev, START, assert_same, digest, pair, settle};

fn dev() -> Dev {
    Dev::new(1, START)
}

/// A note type with fields A, B, C, D, E and one template that shows A.
fn lettered(c: &Collection) -> Id {
    let id = c.create_note_type("Letters", Kind::Standard).unwrap();
    let found = c.note_type(id).unwrap().unwrap();
    c.rename_field(id, found.fields[0].id, "A").unwrap();
    c.rename_field(id, found.fields[1].id, "B").unwrap();
    for name in ["C", "D", "E"] {
        c.add_field(id, name).unwrap();
    }
    let found = c.note_type(id).unwrap().unwrap();
    c.set_template_text(id, found.templates[0].id, "{{A}}", "{{FrontSide}}<hr>{{B}}")
        .unwrap();
    id
}

fn draft(c: &Collection, id: Id) -> Draft {
    Draft::of(&c.note_type(id).unwrap().unwrap())
}

fn names(c: &Collection, id: Id) -> Vec<String> {
    c.note_type(id)
        .unwrap()
        .unwrap()
        .fields
        .into_iter()
        .map(|f| f.name)
        .collect()
}

fn clocks(c: &Collection) -> String {
    c.conn
        .prepare("SELECT entity_type, entity_id, field, hlc FROM register_clock ORDER BY 1, 2, 3")
        .unwrap()
        .query_map([], |row| {
            Ok(format!(
                "{}|{}|{}|{}",
                row.get::<_, String>(0)?,
                row.get::<_, Id>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?
            ))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect::<Vec<_>>()
        .join("\n")
}

fn live_cards(c: &Collection, note: Id) -> Vec<Id> {
    c.cards_of_note(note)
        .unwrap()
        .into_iter()
        .map(|x| x.id)
        .collect()
}

#[test]
fn saving_an_unchanged_draft_writes_nothing() {
    let d = dev();
    let id = lettered(&d.c);
    let before = clocks(&d.c);
    d.clock.advance(1000);
    let outcome = d.c.save_note_type(id, &draft(&d.c, id)).unwrap();
    assert_eq!(outcome, SaveOutcome::default());
    assert_eq!(clocks(&d.c), before);
}

#[test]
fn each_kind_of_change_alone_writes_only_its_registers() {
    let d = dev();
    let id = lettered(&d.c);
    let base = clocks(&d.c);
    let changed = |d: &Dev| -> Vec<String> {
        let after = clocks(&d.c);
        let old: Vec<&str> = base.lines().collect();
        after
            .lines()
            .filter(|l| !old.contains(l))
            .map(|l| l.split('|').nth(2).unwrap().to_owned())
            .collect()
    };

    d.clock.advance(1000);
    let mut edit = draft(&d.c, id);
    edit.name = "Renamed".into();
    d.c.save_note_type(id, &edit).unwrap();
    assert_eq!(changed(&d), ["name"]);

    d.clock.advance(1000);
    let mut edit = draft(&d.c, id);
    edit.css = ".card { color: red }".into();
    d.c.save_note_type(id, &edit).unwrap();
    assert!(changed(&d).contains(&"css".to_owned()));
    assert_eq!(
        d.c.note_type(id).unwrap().unwrap().css,
        ".card { color: red }"
    );

    let mut edit = draft(&d.c, id);
    edit.fields[2].name = "Gee".into();
    d.c.save_note_type(id, &edit).unwrap();
    assert_eq!(names(&d.c, id), ["A", "B", "Gee", "D", "E"]);

    let mut edit = draft(&d.c, id);
    edit.templates[0].back = "{{B}}".into();
    d.c.save_note_type(id, &edit).unwrap();
    assert_eq!(
        d.c.note_type(id).unwrap().unwrap().templates[0].back,
        "{{B}}"
    );
}

#[test]
fn everything_together_in_one_save() {
    let d = dev();
    let id = lettered(&d.c);
    let found = d.c.note_type(id).unwrap().unwrap();
    let mut edit = Draft::of(&found);
    edit.name = "All".into();
    edit.fields.remove(4); // E
    edit.fields.swap(0, 1); // B A C D
    edit.fields.push(DraftField {
        id: None,
        name: "F".into(),
    });
    edit.sort_field = Some(4); // the new F
    edit.templates.push(DraftTemplate {
        id: None,
        name: "Reverse".into(),
        front: "{{B}}".into(),
        back: "{{A}}".into(),
    });
    d.c.save_note_type(id, &edit).unwrap();
    let now = d.c.note_type(id).unwrap().unwrap();
    assert_eq!(now.name, "All");
    assert_eq!(names(&d.c, id), ["B", "A", "C", "D", "F"]);
    assert_eq!(now.sort_field, Some(now.fields[4].id));
    assert_eq!(now.templates.len(), 2);
    assert_eq!(d.c.removed_fields(id).unwrap()[0].name, "E");
}

#[test]
fn a_mistake_writes_nothing() {
    let d = dev();
    let id = lettered(&d.c);
    let before = digest(&d.c);
    let ok = draft(&d.c, id);

    let mut bad = ok.clone();
    bad.name = "  ".into();
    assert_eq!(d.c.save_note_type(id, &bad), Err(NoteTypeError::EmptyName));
    let mut bad = ok.clone();
    bad.fields[1].name = "a".into();
    assert!(matches!(
        d.c.save_note_type(id, &bad),
        Err(NoteTypeError::NameTaken(_))
    ));
    let mut bad = ok.clone();
    bad.fields.clear();
    assert_eq!(d.c.save_note_type(id, &bad), Err(NoteTypeError::LastField));
    let mut bad = ok.clone();
    bad.templates.clear();
    assert_eq!(
        d.c.save_note_type(id, &bad),
        Err(NoteTypeError::LastTemplate)
    );
    let mut bad = ok.clone();
    bad.templates[0].front = "{{#A}}".into();
    assert!(matches!(
        d.c.save_note_type(id, &bad),
        Err(NoteTypeError::Template(_))
    ));
    let mut bad = ok.clone();
    bad.fields[0].id = Some(Id::from_bytes([9; 16]));
    assert_eq!(d.c.save_note_type(id, &bad), Err(NoteTypeError::NotFound));
    let mut bad = ok;
    bad.sort_field = Some(99);
    assert_eq!(d.c.save_note_type(id, &bad), Err(NoteTypeError::NotFound));
    assert_same(&digest(&d.c), &before, "after refused saves");
}

#[test]
fn a_cloze_type_keeps_exactly_one_template() {
    let d = dev();
    let id = builtin::cloze();
    let mut edit = draft(&d.c, id);
    edit.templates.push(DraftTemplate {
        id: None,
        name: "Two".into(),
        front: "{{cloze:Text}}".into(),
        back: "{{cloze:Text}}".into(),
    });
    assert_eq!(
        d.c.save_note_type(id, &edit),
        Err(NoteTypeError::ClozeTemplate)
    );
}

#[test]
fn a_front_that_empties_sends_cards_to_the_trash_and_undo_brings_back_the_same_ones() {
    let d = dev();
    let id = lettered(&d.c);
    let fields: Vec<Id> =
        d.c.note_type(id)
            .unwrap()
            .unwrap()
            .fields
            .iter()
            .map(|f| f.id)
            .collect();
    let note =
        d.c.add_note(id, &[(fields[0], "front"), (fields[1], "back")])
            .unwrap()
            .id;
    let cards = live_cards(&d.c, note);
    assert_eq!(cards.len(), 1);

    let mut edit = draft(&d.c, id);
    edit.templates[0].front = "{{E}}".into();
    let plan = d.c.note_type_consequences(id, &edit).unwrap();
    assert_eq!(
        (plan.cards_added, plan.cards_removed, plan.notes_affected),
        (0, 1, 1)
    );
    assert_eq!(live_cards(&d.c, note), cards, "the dry run wrote nothing");

    let done = d.c.save_note_type(id, &edit).unwrap();
    assert_eq!(
        (done.cards_added, done.cards_removed, done.notes_affected),
        (plan.cards_added, plan.cards_removed, plan.notes_affected)
    );
    assert!(live_cards(&d.c, note).is_empty());

    d.c.restore_note_type_registers(id, &done.undo).unwrap();
    assert_eq!(live_cards(&d.c, note), cards);
    assert_eq!(
        d.c.note_type(id).unwrap().unwrap().templates[0].front,
        "{{A}}"
    );
}

#[test]
fn undo_of_a_save_that_added_things_removes_them() {
    let d = dev();
    let id = lettered(&d.c);
    let before = draft(&d.c, id);
    let mut edit = before.clone();
    edit.name = "Other".into();
    edit.fields.push(DraftField {
        id: None,
        name: "F".into(),
    });
    edit.fields.swap(0, 3);
    edit.templates.push(DraftTemplate {
        id: None,
        name: "Second".into(),
        front: "{{B}}".into(),
        back: "{{A}}".into(),
    });
    edit.sort_field = Some(5);
    let done = d.c.save_note_type(id, &edit).unwrap();
    d.c.restore_note_type_registers(id, &done.undo).unwrap();
    assert_eq!(draft(&d.c, id), before);
}

#[test]
fn removing_and_restoring_through_drafts() {
    let d = dev();
    let id = lettered(&d.c);
    let full = draft(&d.c, id);
    let mut less = full.clone();
    let gone = less.fields.remove(2);
    d.c.save_note_type(id, &less).unwrap();
    assert_eq!(names(&d.c, id), ["A", "B", "D", "E"]);
    // A draft that still holds the removed field's ID restores it, at the place the draft gives.
    let mut back = less.clone();
    back.fields.insert(0, gone);
    d.c.save_note_type(id, &back).unwrap();
    assert_eq!(names(&d.c, id), ["C", "A", "B", "D", "E"]);
}

#[test]
fn a_rename_in_the_draft_keeps_the_values() {
    let d = dev();
    let id = lettered(&d.c);
    let fields: Vec<Id> =
        d.c.note_type(id)
            .unwrap()
            .unwrap()
            .fields
            .iter()
            .map(|f| f.id)
            .collect();
    let note = d.c.add_note(id, &[(fields[0], "hello")]).unwrap().id;
    let mut edit = draft(&d.c, id);
    edit.fields[0].name = "Word".into();
    edit.templates[0].front = "{{Word}}".into();
    d.c.save_note_type(id, &edit).unwrap();
    assert_eq!(live_cards(&d.c, note).len(), 1);
    assert_eq!(d.c.note(note).unwrap().unwrap().fields[0].value, "hello");
}

#[test]
fn a_rename_also_rewrites_the_templates_the_draft_leaves_out() {
    let d = dev();
    let id = lettered(&d.c);
    let mut with_two = draft(&d.c, id);
    with_two.templates.push(DraftTemplate {
        id: None,
        name: "Second".into(),
        front: "{{B}}".into(),
        back: "{{A}}".into(),
    });
    d.c.save_note_type(id, &with_two).unwrap();
    let second = d.c.note_type(id).unwrap().unwrap().templates[1].id;
    let mut edit = draft(&d.c, id);
    edit.templates.pop();
    d.c.save_note_type(id, &edit).unwrap();
    let mut edit = draft(&d.c, id);
    edit.fields[1].name = "Beta".into();
    d.c.save_note_type(id, &edit).unwrap();
    let removed = d.c.removed_templates(id).unwrap();
    assert_eq!(removed[0].id, second);
    assert_eq!(removed[0].front, "{{Beta}}");
}

#[test]
fn reordering_one_field_writes_one_position() {
    let d = dev();
    let id = lettered(&d.c);
    let base = clocks(&d.c);
    d.clock.advance(1000);
    let mut edit = draft(&d.c, id);
    let moved = edit.fields.remove(4);
    edit.fields.insert(1, moved);
    d.c.save_note_type(id, &edit).unwrap();
    assert_eq!(names(&d.c, id), ["A", "E", "B", "C", "D"]);
    let old: Vec<&str> = base.lines().collect();
    let changed: Vec<String> = clocks(&d.c)
        .lines()
        .filter(|l| !old.contains(l))
        .map(str::to_owned)
        .collect();
    assert_eq!(changed.len(), 1, "{changed:?}");
    assert!(changed[0].contains("|position|"));
}

#[test]
fn two_devices_reordering_different_fields_converge_and_keep_both_moves() {
    for flip in [false, true] {
        let (a, b) = pair();
        let id = lettered(&a.c);
        let (x, y) = (&a, &b);
        settle(&[x, y]);
        a.clock.advance(1000);
        b.clock.advance(2000);
        let mut edit = draft(&a.c, id);
        let e = edit.fields.remove(4);
        edit.fields.insert(0, e); // E A B C D
        a.c.save_note_type(id, &edit).unwrap();
        let mut edit = draft(&b.c, id);
        let a_field = edit.fields.remove(0);
        edit.fields.push(a_field); // B C D E A
        b.c.save_note_type(id, &edit).unwrap();
        if flip {
            settle(&[y, x]);
        } else {
            settle(&[x, y]);
        }
        assert_same(&digest(&a.c), &digest(&b.c), "devices after merge");
        let order = names(&a.c, id);
        let at = |n: &str| order.iter().position(|f| f == n).unwrap();
        assert!(at("E") < at("B"), "E stayed before the others: {order:?}");
        assert_eq!(order.len(), 5);
    }
}

#[test]
fn a_replay_of_every_change_converges() {
    let d = dev();
    let id = lettered(&d.c);
    let mut edit = draft(&d.c, id);
    edit.name = "X".into();
    edit.fields.remove(3);
    edit.fields.swap(0, 2);
    edit.templates[0].front = "{{C}}".into();
    d.c.save_note_type(id, &edit).unwrap();
    let fresh = Dev::new(2, START);
    fresh
        .c
        .merge(&d.c.changes(Selection::All).unwrap())
        .unwrap();
    assert_same(&digest(&d.c), &digest(&fresh.c), "a new device");
}

#[test]
fn consequences_count_added_cards_when_a_template_is_added() {
    let d = dev();
    let id = builtin::basic();
    let fields: Vec<Id> =
        d.c.note_type(id)
            .unwrap()
            .unwrap()
            .fields
            .iter()
            .map(|f| f.id)
            .collect();
    for n in 0..3 {
        d.c.add_note(id, &[(fields[0], &format!("q{n}")), (fields[1], "a")])
            .unwrap();
    }
    let mut edit = draft(&d.c, id);
    edit.templates.push(DraftTemplate {
        id: None,
        name: "Reverse".into(),
        front: "{{Back}}".into(),
        back: "{{Front}}".into(),
    });
    let plan = d.c.note_type_consequences(id, &edit).unwrap();
    assert_eq!(
        (plan.cards_added, plan.cards_removed, plan.notes_affected),
        (3, 0, 3)
    );
    let done = d.c.save_note_type(id, &edit).unwrap();
    assert_eq!(done.cards_added, 3);
}

#[test]
fn a_copy_has_new_ids_and_the_same_content() {
    let d = dev();
    let id = lettered(&d.c);
    let mut edit = draft(&d.c, id);
    edit.sort_field = Some(2);
    d.c.save_note_type(id, &edit).unwrap();
    let copy = d.c.copy_note_type(id, "Letters 2").unwrap();
    let (a, b) = (
        d.c.note_type(id).unwrap().unwrap(),
        d.c.note_type(copy).unwrap().unwrap(),
    );
    assert_ne!(a.id, b.id);
    assert_eq!(b.name, "Letters 2");
    assert_eq!(a.css, b.css);
    assert_eq!(
        a.fields.iter().map(|f| &f.name).collect::<Vec<_>>(),
        b.fields.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
    assert_eq!(b.sort_field, Some(b.fields[2].id));
    assert_ne!(a.fields[0].id, b.fields[0].id);
    assert_eq!(a.templates[0].front, b.templates[0].front);
    assert_eq!(d.c.copy_note_type(id, " "), Err(NoteTypeError::EmptyName));
    assert_eq!(
        d.c.copy_note_type(Id::from_bytes([3; 16]), "x"),
        Err(NoteTypeError::NotFound)
    );
}

#[test]
fn preview_with_no_note_shows_field_names() {
    let d = dev();
    let id = builtin::basic();
    let p = d.c.preview_card(id, &draft(&d.c, id), 0, None, 0).unwrap();
    let card = p.rendered.unwrap();
    assert!(card.front.contains("(Front)"));
    assert!(card.back.contains("(Back)"));
    assert!(p.makes_card && p.problems.is_empty() && p.unknown_fields.is_empty());
}

#[test]
fn preview_uses_a_real_note_and_the_unsaved_draft() {
    let d = dev();
    let id = builtin::basic();
    let fields: Vec<Id> =
        d.c.note_type(id)
            .unwrap()
            .unwrap()
            .fields
            .iter()
            .map(|f| f.id)
            .collect();
    let note =
        d.c.add_note(id, &[(fields[0], "kot"), (fields[1], "cat")])
            .unwrap()
            .id;
    let mut edit = draft(&d.c, id);
    edit.templates[0].front = "<b>{{Front}}</b>".into();
    edit.css = ".card{color:blue}".into();
    let p = d.c.preview_card(id, &edit, 0, Some(note), 0).unwrap();
    let card = p.rendered.unwrap();
    assert!(card.front.contains("<b>kot</b>") && card.front.contains("color:blue"));
    // Nothing was saved.
    assert_ne!(
        d.c.note_type(id).unwrap().unwrap().templates[0].front,
        "<b>{{Front}}</b>"
    );
}

#[test]
fn preview_says_when_the_note_makes_no_card_and_names_unknown_fields() {
    let d = dev();
    let id = builtin::basic();
    let fields: Vec<Id> =
        d.c.note_type(id)
            .unwrap()
            .unwrap()
            .fields
            .iter()
            .map(|f| f.id)
            .collect();
    let note = d.c.add_note(id, &[(fields[0], "kot")]).unwrap().id;
    let mut edit = draft(&d.c, id);
    edit.templates[0].front = "{{Back}} {{Nope}}".into();
    let p = d.c.preview_card(id, &edit, 0, Some(note), 0).unwrap();
    assert!(!p.makes_card);
    assert_eq!(p.unknown_fields, ["Nope"]);
    assert!(p.rendered.is_some());
}

#[test]
fn preview_of_a_mistake_answers_the_problem_and_no_card() {
    let d = dev();
    let id = builtin::basic();
    let mut edit = draft(&d.c, id);
    edit.templates[0].front = "{{#Front}}".into();
    edit.templates[0].back = "{{/Front}}".into();
    let p = d.c.preview_card(id, &edit, 0, None, 0).unwrap();
    assert!(p.rendered.is_none());
    assert_eq!(p.problems.len(), 2);
    assert_eq!(
        d.c.preview_card(id, &edit, 5, None, 0),
        Err(NoteTypeError::NotFound)
    );
}

#[test]
fn preview_of_a_cloze_note_per_number() {
    let d = dev();
    let id = builtin::cloze();
    let fields: Vec<Id> =
        d.c.note_type(id)
            .unwrap()
            .unwrap()
            .fields
            .iter()
            .map(|f| f.id)
            .collect();
    let note =
        d.c.add_note(id, &[(fields[0], "{{c1::one}} and {{c2::two}}")])
            .unwrap()
            .id;
    let edit = draft(&d.c, id);
    let one = d.c.preview_card(id, &edit, 0, Some(note), 1).unwrap();
    let two = d.c.preview_card(id, &edit, 0, Some(note), 2).unwrap();
    let three = d.c.preview_card(id, &edit, 0, Some(note), 3).unwrap();
    assert!(one.makes_card && two.makes_card && !three.makes_card);
    assert_ne!(one.rendered.unwrap().front, two.rendered.unwrap().front);
}
