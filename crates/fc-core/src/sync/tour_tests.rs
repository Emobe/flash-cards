//! The audit of ADR 0008 part 10 and the convergence test of part 11: every public operation that
//! writes leaves everything tracked, and three collections that make random changes end identical.

use rusqlite::types::Value;

use super::merge_tests::{Dev, START, assert_same, digest, pair, settle};
use super::*;
use crate::collection::{Collection, Migration, Schema};
use crate::deck::PresetChange;
use crate::id::Id;
use crate::notetype::{Kind, builtin};
use crate::scheduling::{Rating, default_parameters};
use crate::search::{Mode, SavedSearchChange, Sort};
use crate::study::answer_tests::{HOUR, MINUTE};

// ---- Invariants ----

/// Every register of every row has a clock or holds its column default, and every append-only row
/// written on this device is in `unpushed_row` (ADR 0008, part 10).
fn check_invariants(c: &Collection) {
    for table in c.schema.tables {
        for register in table.registers {
            let (default, _): (Option<String>, i64) = c
                .conn
                .query_row(
                    "SELECT dflt_value, 0 FROM pragma_table_info(?1) WHERE name = ?2",
                    [table.table, register],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            let default = default.unwrap_or_else(|| "NULL".to_owned());
            let bad: i64 = c
                .conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM \"{t}\" r
                         WHERE NOT EXISTS (SELECT 1 FROM register_clock k
                                           WHERE k.entity_type = ?1 AND k.entity_id = r.id AND k.field = ?2)
                           AND r.\"{register}\" IS NOT ({default})",
                        t = table.table
                    ),
                    [table.entity, register],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(bad, 0, "{}.{register}: a value with no clock", table.entity);
        }
    }
    for table in DYNAMIC_TABLES {
        let mut statement = c
            .conn
            .prepare(&format!(
                "SELECT \"{}\", \"{}\" FROM \"{}\"",
                table.owner, table.key, table.table
            ))
            .unwrap();
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, Id>(0)?, row.get::<_, Value>(1)?))
            })
            .unwrap();
        for row in rows {
            let (owner, key) = row.unwrap();
            let register = match key {
                Value::Text(text) => text,
                Value::Blob(bytes) => Id::from_slice(&bytes).unwrap().to_string(),
                other => panic!("{other:?}"),
            };
            assert!(
                c.register_clock(table.entity, owner, &register)
                    .unwrap()
                    .is_some(),
                "{}: `{register}` has no clock",
                table.entity
            );
        }
    }
    let me = c.device_id().unwrap();
    let unlisted: i64 = c
        .conn
        .query_row(
            "SELECT COUNT(*) FROM card_event e WHERE e.device = ?1 AND NOT EXISTS
               (SELECT 1 FROM unpushed_row u WHERE u.entity_type = 'card_event' AND u.row_id = e.id)",
            [me],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(unlisted, 0, "events written here must be unpushed");
}

// ---- The tour ----

/// Every writing operation once, on `a`. Shared with the backup tests.
pub(crate) fn run_the_tour(a: &Dev) {
    let c = &a.c;
    let step = |n: i64| a.clock.advance(n * MINUTE);

    // Note types, fields and templates.
    let nt = c.create_note_type("Tour", Kind::Standard).unwrap();
    c.rename_note_type(nt, "Tour 2").unwrap();
    c.set_note_type_css(nt, ".card { color: red }").unwrap();
    let extra = c.add_field(nt, "Extra").unwrap();
    c.rename_field(nt, extra, "More").unwrap();
    c.move_field(nt, extra, 0).unwrap();
    let fields = c.note_type(nt).unwrap().unwrap().fields;
    c.set_sort_field(nt, fields[1].id).unwrap();
    let second = c
        .add_template(nt, "Reverse", "{{Back}}", "{{Front}}")
        .unwrap();
    c.rename_template(nt, second, "Reverse 2").unwrap();
    c.set_template_text(nt, second, "{{Back}} ?", "{{Front}} !")
        .unwrap();
    c.move_template(nt, second, 0).unwrap();
    let doomed = c
        .add_template(nt, "Doomed", "{{Front}} x", "{{Back}}")
        .unwrap();
    c.remove_template(nt, doomed).unwrap();
    c.restore_template(nt, doomed).unwrap();
    let doomed_field = c.add_field(nt, "Doomed").unwrap();
    c.remove_field(nt, doomed_field).unwrap();
    c.restore_field(nt, doomed_field).unwrap();
    let gone_type = c.create_note_type("Gone", Kind::Cloze).unwrap();
    c.delete_note_type(gone_type).unwrap();
    c.restore_note_type(gone_type).unwrap();
    step(1);

    // Notes, decks and presets.
    let fields = c.note_type(nt).unwrap().unwrap().fields;
    let values: Vec<(Id, String)> = fields
        .iter()
        .map(|f| (f.id, format!("v-{}", f.name)))
        .collect();
    let values: Vec<(Id, &str)> = values.iter().map(|(i, v)| (*i, v.as_str())).collect();
    let n1 = c.add_note(nt, &values).unwrap();
    let deck = c.create_deck("Tour deck", None).unwrap();
    let child = c.create_deck("Child", Some(deck)).unwrap();
    c.rename_deck(child, "Child 2").unwrap();
    c.set_deck_limits_include_subdecks(deck, false).unwrap();
    let n2 = c.add_note_to_deck(deck, nt, &values).unwrap();
    c.move_cards(&n2.cards, child).unwrap();
    c.move_deck(child, None).unwrap();
    let other = c.create_deck("Other", None).unwrap();
    c.delete_deck(other).unwrap();
    c.restore_deck(other).unwrap();
    c.set_note_fields(n1.id, &[(fields[0].id, "changed")])
        .unwrap();
    let n3 = c.add_note(nt, &values).unwrap();
    c.delete_note(n3.id).unwrap();
    c.restore_note(n3.id).unwrap();
    let preset = c.create_preset("Tour preset").unwrap();
    c.rename_preset(preset, "Tour preset 2").unwrap();
    let mut parameters = default_parameters().to_vec();
    parameters[0] += 0.1;
    c.set_preset_options(
        preset,
        &PresetChange {
            new_per_day: Some(7),
            reviews_per_day: Some(70),
            learning_steps: Some(vec![2, 20]),
            relearning_steps: Some(vec![15]),
            desired_retention: Some(0.85),
            space_siblings: Some(false),
            fsrs_parameters: Some(parameters),
        },
    )
    .unwrap();
    c.set_deck_preset(deck, preset).unwrap();
    let spare = c.create_preset("Spare").unwrap();
    c.delete_preset(spare).unwrap();
    c.restore_preset(spare).unwrap();
    step(1);

    // Tags.
    c.add_tags(&[n1.id, n2.id], &["alpha", "beta::gamma"])
        .unwrap();
    c.remove_tags(&[n2.id], &["alpha"]).unwrap();
    c.set_note_tags(n3.id, &["delta"]).unwrap();
    c.rename_tag("delta", "epsilon").unwrap();
    c.delete_tag("beta::gamma").unwrap();
    c.add_tags(&[n1.id], &["beta::gamma"]).unwrap();

    // Study.
    let cards = c.cards_of_note(n1.id).unwrap();
    let (first, second_card) = (cards[0].id, cards[1].id);
    c.answer(first, Rating::Good, 3_000).unwrap();
    step(11);
    c.answer(first, Rating::Again, 4_000).unwrap();
    step(1);
    c.undo_answer().unwrap().unwrap();
    c.answer(second_card, Rating::Easy, 1_000).unwrap();
    c.suspend_cards(&[first]).unwrap();
    c.unsuspend_cards(&[first]).unwrap();
    c.bury_cards(&[second_card]).unwrap();
    c.unbury_cards(&[second_card]).unwrap();
    c.bury_cards(&[first]).unwrap();
    c.unbury_deck(crate::deck::default_deck()).unwrap();
    c.set_day_start_hour(4).unwrap();

    // Searches.
    let search = c
        .save_search("Tour", "tag:alpha", Sort::default(), Mode::Cards)
        .unwrap();
    c.update_saved_search(
        search,
        &SavedSearchChange {
            name: Some("Tour 2".into()),
            query: Some("is:due".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let dead_search = c
        .save_search("Dead", "is:new", Sort::default(), Mode::Notes)
        .unwrap();
    c.delete_saved_search(dead_search).unwrap();

    // Media.
    let used = c.add_media("used.png", b"used bytes").unwrap();
    let unused = c.add_media("unused.png", b"unused bytes").unwrap();
    c.set_note_fields(
        n1.id,
        &[(fields[0].id, &format!("<img src=\"{}\">", used.name))],
    )
    .unwrap();
    c.delete_media(&unused.name).unwrap();
    c.restore_media(&unused.name).unwrap();
    c.delete_unused_media().unwrap();
    c.add_media("again.png", b"again").unwrap();
    let again = c.media_files().unwrap();
    c.delete_media(&format!("again-{}.png", &again.last().unwrap().hash[..16]))
        .ok();

    // Other.
    c.require_feature("future-thing").unwrap();
}

#[test]
fn the_tour_of_every_writing_operation_reproduces_in_an_empty_collection() {
    let (a, _) = pair();
    let c = &a.c;
    run_the_tour(&a);

    check_invariants(c);
    let fresh = Dev::new(2, START);
    fresh.c.merge(&c.changes(Selection::All).unwrap()).unwrap();
    check_invariants(&fresh.c);
    assert_same(&digest(&fresh.c), &digest(c), "tour");
    assert_eq!(fresh.c.unsupported_features().unwrap(), ["future-thing"]);
}

// ---- Random operations ----

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        (!items.is_empty()).then(|| &items[self.below(items.len())])
    }

    /// For lists of IDs and other small `Copy` values.
    fn pick_copy<T: Copy>(&mut self, items: &[T]) -> Option<T> {
        self.pick(items).copied()
    }
}

const WORDS: [&str; 6] = ["red", "green", "blue", "{{c1::cloze}} text", "", "plain"];
const TAGS: [&str; 5] = ["a", "b", "a::b", "c", "A"];

/// One random change through the public API. A refusal (the state does not allow it) is fine: the
/// operation is simply skipped.
fn random_op(d: &Dev, rng: &mut Rng) {
    let c = &d.c;
    let note_types: Vec<Id> = c.note_types().unwrap().iter().map(|t| t.id).collect();
    let notes: Vec<Id> = note_types
        .iter()
        .flat_map(|t| c.notes(*t).unwrap())
        .map(|n| n.id)
        .collect();
    let decks: Vec<Id> = c.decks().unwrap().iter().map(|k| k.id).collect();
    let presets: Vec<Id> = c.presets().unwrap().iter().map(|p| p.id).collect();
    // Only some operations need the cards, and reading them is the slow part.
    let cards = || -> Vec<Id> {
        notes
            .iter()
            .flat_map(|n| c.cards_of_note(*n).unwrap())
            .map(|k| k.id)
            .collect()
    };
    let word = WORDS[rng.below(WORDS.len())];
    let tag = TAGS[rng.below(TAGS.len())];
    let _ = match rng.below(46) {
        0..=4 => {
            let t = rng.pick_copy(&note_types).unwrap_or_else(builtin::basic);
            let fields = c
                .note_type(t)
                .ok()
                .flatten()
                .map(|t| t.fields)
                .unwrap_or_default();
            let values: Vec<(Id, &str)> = fields.iter().map(|f| (f.id, word)).collect();
            c.add_note(t, &values).map(drop).map_err(drop)
        }
        5..=8 => {
            let Some(n) = rng.pick_copy(&notes) else {
                return;
            };
            let Some(note) = c.note(n).ok().flatten() else {
                return;
            };
            let Some(f) = rng.pick(&note.fields) else {
                return;
            };
            c.set_note_fields(n, &[(f.field, word)])
                .map(drop)
                .map_err(drop)
        }
        9 => rng
            .pick_copy(&notes)
            .map_or(Ok(()), |n| c.delete_note(n).map_err(drop)),
        10 => {
            let dead = c.deleted_notes().unwrap();
            rng.pick_copy(&dead.iter().map(|n| n.id).collect::<Vec<_>>())
                .map_or(Ok(()), |n| c.restore_note(n).map(drop).map_err(drop))
        }
        11 => rng
            .pick_copy(&notes)
            .map_or(Ok(()), |n| c.add_tags(&[n], &[tag]).map(drop).map_err(drop)),
        12 => rng.pick_copy(&notes).map_or(Ok(()), |n| {
            c.remove_tags(&[n], &[tag]).map(drop).map_err(drop)
        }),
        13 => rng.pick_copy(&notes).map_or(Ok(()), |n| {
            c.set_note_tags(n, &[tag, TAGS[0]]).map_err(drop)
        }),
        14 => c
            .rename_tag(tag, TAGS[rng.below(TAGS.len())])
            .map(drop)
            .map_err(drop),
        15 => c.delete_tag(tag).map(drop).map_err(drop),
        16 => c
            .create_deck(word, rng.pick_copy(&decks))
            .map(drop)
            .map_err(drop),
        17 => rng
            .pick_copy(&decks)
            .map_or(Ok(()), |k| c.rename_deck(k, word).map_err(drop)),
        18 => {
            let (k, p) = (rng.pick_copy(&decks), rng.pick_copy(&decks));
            k.map_or(Ok(()), |k| {
                c.move_deck(k, p.filter(|p| *p != k)).map_err(drop)
            })
        }
        19 => rng
            .pick_copy(&decks)
            .map_or(Ok(()), |k| c.delete_deck(k).map_err(drop)),
        20 => {
            let dead = c.deleted_decks().unwrap();
            rng.pick_copy(&dead.iter().map(|k| k.id).collect::<Vec<_>>())
                .map_or(Ok(()), |k| c.restore_deck(k).map_err(drop))
        }
        21 => match (rng.pick_copy(&cards()), rng.pick_copy(&decks)) {
            (Some(k), Some(deck)) => c.move_cards(&[k], deck).map_err(drop),
            _ => Ok(()),
        },
        22 => rng.pick_copy(&decks).map_or(Ok(()), |k| {
            c.set_deck_limits_include_subdecks(k, rng.below(2) == 0)
                .map_err(drop)
        }),
        23 => c.create_preset(word).map(drop).map_err(drop),
        24 => rng.pick_copy(&presets).map_or(Ok(()), |p| {
            let change = PresetChange {
                new_per_day: Some(rng.below(40) as u32),
                desired_retention: Some(0.8 + rng.below(15) as f64 / 100.0),
                fsrs_parameters: (rng.below(3) == 0).then(|| {
                    let mut v = default_parameters().to_vec();
                    v[0] += rng.below(50) as f32 / 100.0;
                    v
                }),
                ..Default::default()
            };
            c.set_preset_options(p, &change).map_err(drop)
        }),
        25 => match (rng.pick_copy(&decks), rng.pick_copy(&presets)) {
            (Some(k), Some(p)) => c.set_deck_preset(k, p).map_err(drop),
            _ => Ok(()),
        },
        26 => rng
            .pick_copy(&presets)
            .map_or(Ok(()), |p| c.delete_preset(p).map_err(drop)),
        27 => {
            let dead = c.deleted_presets().unwrap();
            rng.pick_copy(&dead.iter().map(|p| p.id).collect::<Vec<_>>())
                .map_or(Ok(()), |p| c.restore_preset(p).map_err(drop))
        }
        28 => rng.pick_copy(&cards()).map_or(Ok(()), |k| {
            let rating = [Rating::Again, Rating::Hard, Rating::Good, Rating::Easy][rng.below(4)];
            c.answer(k, rating, 2_000).map(drop).map_err(drop)
        }),
        29 => c.undo_answer().map(drop).map_err(drop),
        30 => rng.pick_copy(&cards()).map_or(Ok(()), |k| {
            if rng.below(2) == 0 {
                c.suspend_cards(&[k])
            } else {
                c.unsuspend_cards(&[k])
            }
            .map_err(drop)
        }),
        31 => rng.pick_copy(&cards()).map_or(Ok(()), |k| {
            if rng.below(2) == 0 {
                c.bury_cards(&[k])
            } else {
                c.unbury_cards(&[k])
            }
            .map_err(drop)
        }),
        32 => rng
            .pick_copy(&decks)
            .map_or(Ok(()), |k| c.unbury_deck(k).map_err(drop)),
        33 => c.set_day_start_hour(rng.below(6) as u8).map_err(drop),
        34 => c
            .create_note_type(
                word,
                if rng.below(4) == 0 {
                    Kind::Cloze
                } else {
                    Kind::Standard
                },
            )
            .map(drop)
            .map_err(drop),
        35 => rng
            .pick_copy(&note_types)
            .map_or(Ok(()), |t| c.rename_note_type(t, word).map_err(drop)),
        36 => rng.pick_copy(&note_types).map_or(Ok(()), |t| {
            if rng.below(3) == 0 {
                c.delete_note_type(t)
            } else {
                c.set_note_type_css(t, word)
            }
            .map_err(drop)
        }),
        37 => {
            let dead = c.deleted_note_types().unwrap();
            rng.pick_copy(&dead.iter().map(|t| t.id).collect::<Vec<_>>())
                .map_or(Ok(()), |t| c.restore_note_type(t).map_err(drop))
        }
        38 => rng.pick_copy(&note_types).map_or(Ok(()), |t| {
            let found = c.note_type(t).unwrap().unwrap();
            match rng.below(5) {
                0 => c.add_field(t, word).map(drop),
                1 => rng
                    .pick(&found.fields)
                    .map_or(Ok(()), |f| c.remove_field(t, f.id)),
                2 => rng
                    .pick(&found.fields)
                    .map_or(Ok(()), |f| c.rename_field(t, f.id, word)),
                3 => rng
                    .pick(&found.fields)
                    .map_or(Ok(()), |f| c.move_field(t, f.id, rng.below(3))),
                _ => {
                    let removed = c.removed_fields(t).unwrap();
                    rng.pick(&removed)
                        .map_or(Ok(()), |f| c.restore_field(t, f.id))
                }
            }
            .map_err(drop)
        }),
        39 => rng.pick_copy(&note_types).map_or(Ok(()), |t| {
            let found = c.note_type(t).unwrap().unwrap();
            match rng.below(6) {
                0 => c.add_template(t, word, "{{Back}}", "{{Front}}").map(drop),
                1 => rng
                    .pick(&found.templates)
                    .map_or(Ok(()), |x| c.remove_template(t, x.id)),
                2 => rng
                    .pick(&found.templates)
                    .map_or(Ok(()), |x| c.rename_template(t, x.id, word)),
                3 => rng
                    .pick(&found.templates)
                    .map_or(Ok(()), |x| c.set_template_text(t, x.id, "{{Front}}", word)),
                4 => rng
                    .pick(&found.templates)
                    .map_or(Ok(()), |x| c.move_template(t, x.id, rng.below(3))),
                _ => {
                    let removed = c.removed_templates(t).unwrap();
                    rng.pick(&removed)
                        .map_or(Ok(()), |x| c.restore_template(t, x.id))
                }
            }
            .map_err(drop)
        }),
        40 => rng.pick_copy(&note_types).map_or(Ok(()), |t| {
            let found = c.note_type(t).unwrap().unwrap();
            rng.pick(&found.fields)
                .map_or(Ok(()), |f| c.set_sort_field(t, f.id).map_err(drop))
        }),
        41 => c
            .save_search(word, "is:due", Sort::default(), Mode::Cards)
            .map(drop)
            .map_err(drop),
        42 => {
            let searches = c.saved_searches().unwrap();
            rng.pick_copy(&searches.iter().map(|s| s.id).collect::<Vec<_>>())
                .map_or(Ok(()), |s| {
                    if rng.below(2) == 0 {
                        c.delete_saved_search(s)
                    } else {
                        c.update_saved_search(
                            s,
                            &SavedSearchChange {
                                name: Some(word.to_owned()),
                                ..Default::default()
                            },
                        )
                    }
                    .map_err(drop)
                })
        }
        43 => c
            .add_media("m.png", &[rng.below(4) as u8; 8])
            .map(drop)
            .map_err(drop),
        44 => {
            let files = c.media_files().unwrap();
            rng.pick_copy(&files.iter().map(|m| m.id).collect::<Vec<_>>())
                .map_or(Ok(()), |_| {
                    match rng.below(3) {
                        0 => c.delete_unused_media().map(drop),
                        _ => c.add_media("n.png", &[rng.below(4) as u8; 8]).map(drop),
                    }
                    .map_err(drop)
                })
        }
        _ => {
            // A note that names a media file, so "unused" changes.
            let files = c.media_files().unwrap();
            let Some(n) = rng.pick_copy(&notes) else {
                return;
            };
            let Some(note) = c.note(n).ok().flatten() else {
                return;
            };
            let (Some(f), Some(file)) = (note.fields.first(), files.first()) else {
                return;
            };
            let name = format!("n-{}.png", &file.hash[..16]);
            c.set_note_fields(n, &[(f.field, &format!("<img src=\"{name}\">"))])
                .map(drop)
                .map_err(drop)
        }
    };
}

// ---- Convergence ----

fn converge(seed: u64, operations: usize) {
    crate::id::seeded::seed(seed);
    let mut rng = Rng(seed);
    // Clocks that disagree by minutes and hours.
    let devs = [
        Dev::new(1, START),
        Dev::new(2, START + 7 * MINUTE),
        Dev::new(3, START - 3 * HOUR),
    ];
    for i in 0..operations {
        let d = &devs[rng.below(3)];
        d.clock.advance(1_000 + rng.below(2 * HOUR as usize) as i64);
        random_op(d, &mut rng);
        if i % 20 == 19 {
            let (from, to) = (rng.below(3), rng.below(3));
            if from != to {
                let changes = devs[from].c.changes(Selection::All).unwrap();
                devs[to].c.merge(&changes).unwrap();
            }
        }
        if i % 500 == 499 {
            for d in &devs {
                check_invariants(&d.c);
            }
        }
    }
    let all: Vec<&Dev> = devs.iter().collect();
    settle(&all);
    let first = digest(&devs[0].c);
    for d in &devs {
        check_invariants(&d.c);
        assert_same(&digest(&d.c), &first, &format!("seed {seed}"));
    }
    // Applying a batch again changes nothing.
    for a in &devs {
        for b in &devs {
            let report = b.c.merge(&a.c.changes(Selection::All).unwrap()).unwrap();
            assert_eq!(
                (
                    report.registers_applied,
                    report.rows_added,
                    report.notes_reconciled,
                    report.cards_rebuilt
                ),
                (0, 0, 0, 0),
                "seed {seed}"
            );
            assert!(report.rejected.is_empty(), "{:?}", report.rejected);
        }
    }
    assert_same(&digest(&devs[1].c), &first, "again");
    // Another order, into a collection that has nothing, gives the same result.
    let forward = Dev::new(4, START);
    let backward = Dev::new(5, START);
    for d in &devs {
        forward
            .c
            .merge(&d.c.changes(Selection::All).unwrap())
            .unwrap();
    }
    for d in devs.iter().rev() {
        backward
            .c
            .merge(&d.c.changes(Selection::All).unwrap())
            .unwrap();
    }
    assert_same(&digest(&forward.c), &first, "forward");
    assert_same(&digest(&backward.c), &first, "backward");
}

#[test]
fn three_collections_with_random_changes_end_identical() {
    converge(11, 2_000);
}

#[test]
fn three_collections_converge_for_other_seeds() {
    converge(12, 700);
    converge(13, 700);
    converge(14, 700);
}

// ---- Two app versions ----

struct TempDb(std::path::PathBuf);

impl TempDb {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self(std::env::temp_dir().join(format!("fc-merge-{}-{n}.db", std::process::id())))
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

use crate::collection::MIGRATIONS;

const OLD_CARD: SyncedTable = SyncedTable {
    entity: "card",
    table: "card",
    registers: &["note", "template", "ordinal", "deck", "deleted"],
};
const OLD_DECK: SyncedTable = SyncedTable {
    entity: "deck",
    table: "deck",
    registers: &["name", "parent", "options_preset", "deleted"],
};
const OLD_PRESET: SyncedTable = SyncedTable {
    entity: "options_preset",
    table: "options_preset",
    registers: &[
        "name",
        "new_per_day",
        "reviews_per_day",
        "learning_steps",
        "desired_retention",
        "relearning_steps",
        "fsrs_parameters",
        "deleted",
    ],
};

const OLD_TABLES: &[SyncedTable] = &[
    super::requires::TABLE,
    crate::notetype::NOTE_TYPE,
    crate::notetype::FIELD,
    crate::notetype::TEMPLATE,
    crate::note::NOTE,
    OLD_CARD,
    OLD_DECK,
    OLD_PRESET,
    crate::study::SETTING,
];

const OLD: Schema = Schema {
    migrations: &[
        MIGRATIONS[0],
        MIGRATIONS[1],
        MIGRATIONS[2],
        MIGRATIONS[3],
        MIGRATIONS[4],
        MIGRATIONS[5],
        MIGRATIONS[6],
        // The one table of version 11 that the old build needs to keep unknown rows.
        Migration {
            version: 8,
            apply: MIGRATIONS[10].apply,
        },
    ],
    tables: OLD_TABLES,
};

fn adopt_v8(tx: &rusqlite::Transaction) -> rusqlite::Result<()> {
    (MIGRATIONS[7].apply)(tx)?;
    adopt_unknown(tx, "card", "card", &["suspended", "buried_until"])?;
    adopt_unknown(tx, "deck", "deck", &["limits_include_subdecks"])?;
    adopt_unknown(tx, "options_preset", "options_preset", &["space_siblings"])?;
    Ok(())
}

fn adopt_v9(tx: &rusqlite::Transaction) -> rusqlite::Result<()> {
    (MIGRATIONS[8].apply)(tx)?;
    adopt_unknown(
        tx,
        "saved_search",
        "saved_search",
        crate::search::SAVED_SEARCH.registers,
    )?;
    Ok(())
}

fn adopt_v10(tx: &rusqlite::Transaction) -> rusqlite::Result<()> {
    (MIGRATIONS[9].apply)(tx)?;
    adopt_unknown(
        tx,
        "media_file",
        "media_file",
        crate::media::MEDIA_FILE.registers,
    )?;
    Ok(())
}

/// A later app that adds a column to the event table.
fn adopt_v11(tx: &rusqlite::Transaction) -> rusqlite::Result<()> {
    tx.execute_batch("ALTER TABLE card_event ADD COLUMN shape TEXT")?;
    adopt_unknown(tx, "card_event", "card_event", &["shape"])?;
    Ok(())
}

const UPGRADED: Schema = Schema {
    migrations: &[
        MIGRATIONS[0],
        MIGRATIONS[1],
        MIGRATIONS[2],
        MIGRATIONS[3],
        MIGRATIONS[4],
        MIGRATIONS[5],
        MIGRATIONS[6],
        Migration {
            version: 8,
            apply: MIGRATIONS[10].apply,
        },
        Migration {
            version: 9,
            apply: adopt_v8,
        },
        Migration {
            version: 10,
            apply: adopt_v9,
        },
        Migration {
            version: 11,
            apply: adopt_v10,
        },
        Migration {
            version: 12,
            apply: adopt_v11,
        },
    ],
    tables: SYNCED_TABLES,
};

fn host_of(dev: &Dev, n: u8) -> crate::clock::Host {
    crate::clock::Host {
        clock: dev.clock.clone(),
        installation_id: Id::from_bytes([n; 16]),
    }
}

#[test]
fn an_older_app_keeps_what_it_does_not_know_relays_it_and_adopts_it_after_an_upgrade() {
    let (full, _) = pair();
    // Everything the older app does not have: a register of a card, a deck, a preset, a whole table
    // of saved searches, media, and a column of events that only the newer app below has.
    let (note, card) = super::merge_tests::add(&full.c, "x", "y");
    full.c.suspend_cards(&[card]).unwrap();
    full.c
        .set_deck_limits_include_subdecks(crate::deck::default_deck(), false)
        .unwrap();
    full.c
        .set_preset_options(
            crate::deck::default_preset(),
            &PresetChange {
                space_siblings: Some(false),
                ..Default::default()
            },
        )
        .unwrap();
    full.c
        .save_search("Due", "is:due", Sort::default(), Mode::Cards)
        .unwrap();
    full.c.add_media("pic.png", b"bytes").unwrap();
    crate::study::answer_tests::good(&full.c, card);
    // Notes are left out: they make the merge read the deck table, which an old schema would
    // refuse for the new code under test. What it keeps as unknown does not depend on them.
    let mut all = full.c.changes(Selection::All).unwrap();
    all.registers.retain(|r| r.entity != "note");
    let mut event = all
        .rows
        .iter()
        .find(|r| r.entity == "card_event")
        .unwrap()
        .clone();
    event.id = Id::from_bytes([9; 16]);
    for (name, value) in &mut event.columns {
        if name == "time_ms" {
            *value = Value::Integer(START + 3 * HOUR);
        }
    }
    all.rows.push(event.clone());
    // A collection that knows everything and received the same.
    let reference = Dev::new(8, START);
    reference.c.merge(&all).unwrap();
    all.rows.pop();
    event
        .columns
        .push(("shape".into(), Value::Text("cube".into())));
    all.rows.push(event);

    let file = TempDb::new();
    let old = Collection::create_with(file.path(), OLD, host_of(&full, 6)).unwrap();
    let report = old.merge(&all).unwrap();
    assert!(report.unknown_registers > 0, "{report:?}");
    assert!(report.unknown_rows > 0);
    assert!(report.rejected.is_empty(), "{:?}", report.rejected);
    // What it knows is in its tables.
    let cards: i64 = old
        .conn
        .query_row("SELECT COUNT(*) FROM card WHERE note = ?1", [note], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(cards, 1);

    // It relays everything unchanged to a build that knows it all.
    let relayed = Dev::new(3, START);
    relayed
        .c
        .merge(&old.changes(Selection::All).unwrap())
        .unwrap();
    assert_same(&digest(&relayed.c), &digest(&reference.c), "relayed");
    let extra = old
        .changes(Selection::All)
        .unwrap()
        .rows
        .into_iter()
        .find(|r| r.id == Id::from_bytes([9; 16]))
        .unwrap();
    assert!(
        extra
            .columns
            .contains(&("shape".into(), Value::Text("cube".into())))
    );
    old.close().unwrap();

    // After an upgrade it has all of it in its tables, and nothing is left in the store.
    let upgraded = Collection::open_with(file.path(), UPGRADED, host_of(&full, 6)).unwrap();
    assert_eq!(upgraded.info().unwrap().schema_version, 12);
    assert_same(&digest(&upgraded), &digest(&reference.c), "upgraded");
    let kept: i64 = upgraded
        .conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM unknown_register) + (SELECT COUNT(*) FROM unknown_row_value)",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(kept, 0);
    let shape: String = upgraded
        .conn
        .query_row(
            "SELECT shape FROM card_event WHERE id = ?1",
            [Id::from_bytes([9; 16])],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(shape, "cube");
    // The update trigger of the event table is back.
    assert!(
        upgraded
            .conn
            .execute("UPDATE card_event SET day = 0", [])
            .is_err()
    );
}
