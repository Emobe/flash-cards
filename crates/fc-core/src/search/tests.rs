//! Search: the grammar, every filter, boolean logic, sorting, paging and saved searches.

use std::sync::Arc;

use super::parse::{Node, Term, parse};
use super::*;
use crate::clock::{Host, ManualClock};
use crate::collection::{Collection, MIGRATIONS, Schema};
use crate::notetype::builtin;
use crate::scheduling::Rating;
use crate::study::answer_tests::{DAY, DAY0, HOUR, MINUTE};
use crate::sync::{SYNCED_TABLES, check_schema};

fn host(clock: &Arc<ManualClock>) -> Host {
    Host {
        clock: clock.clone(),
        installation_id: Id::from_bytes(*b"installation-one"),
    }
}

fn setup() -> (Collection, Arc<ManualClock>) {
    let clock = Arc::new(ManualClock::new(DAY0 + 9 * HOUR));
    let c = Collection::create(":memory:", host(&clock)).unwrap();
    (c, clock)
}

fn fields(c: &Collection, note_type: Id) -> Vec<Id> {
    c.note_type(note_type)
        .unwrap()
        .unwrap()
        .fields
        .iter()
        .map(|f| f.id)
        .collect()
}

/// A Basic note in `deck`, a millisecond after the last. Returns the note and its card.
fn basic(c: &Collection, clock: &ManualClock, deck: Id, front: &str, back: &str) -> (Id, Id) {
    let f = fields(c, builtin::basic());
    let added = c
        .add_note_to_deck(deck, builtin::basic(), &[(f[0], front), (f[1], back)])
        .unwrap();
    clock.advance(1);
    (added.id, added.cards[0])
}

/// What most tests search: the Front of every card the query finds, in alphabetical order.
fn titles(c: &Collection, query: &str) -> Vec<String> {
    let page = c
        .search(
            query,
            &SearchOptions {
                limit: MAX_LIMIT,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(page.total as usize, page.rows.len());
    let mut titles: Vec<String> = page.rows.into_iter().map(|r| r.title).collect();
    titles.sort();
    titles
}

fn sorted(c: &Collection, query: &str, sort: Sort, mode: Mode) -> Vec<String> {
    c.search(
        query,
        &SearchOptions {
            sort,
            mode,
            limit: MAX_LIMIT,
            ..Default::default()
        },
    )
    .unwrap()
    .rows
    .into_iter()
    .map(|r| r.title)
    .collect()
}

fn failure(c: &Collection, query: &str) -> SearchError {
    c.search(query, &SearchOptions::default()).unwrap_err()
}

struct Fixture {
    c: Collection,
    clock: Arc<ManualClock>,
    polish: Id,
    animals: Id,
    kot: (Id, Id),
    pies: (Id, Id),
}

/// A small Polish collection:
///
/// - `kot` in Polish::Animals (tags `animals`, `lang::polish`), answered Good twice: a review card.
/// - `pies` in Polish::Animals, answered Again once: learning.
/// - `Łódź` in Polish (tag `lang::polish`), and `<b>dom</b>&nbsp;mały` in the Default deck.
/// - `chat` in French (tag `lang::french`).
/// - `gato`, a Basic and reversed note in Spanish, which has two cards.
/// - a Cloze note in the Default deck with two cards.
fn fixture() -> Fixture {
    let (c, clock) = setup();
    let polish = c.create_deck("Polish", None).unwrap();
    let animals = c.create_deck("Animals", Some(polish)).unwrap();
    let french = c.create_deck("French", None).unwrap();
    let spanish = c.create_deck("Spanish", None).unwrap();
    let kot = basic(&c, &clock, animals, "kot", "cat");
    let pies = basic(&c, &clock, animals, "pies", "dog");
    let lodz = basic(&c, &clock, polish, "Łódź", "a city");
    basic(&c, &clock, default_deck(), "<b>dom</b>&nbsp;mały", "house");
    let chat = basic(&c, &clock, french, "chat", "cat");
    c.add_tags(&[kot.0], &["animals", "lang::polish"]).unwrap();
    c.add_tags(&[lodz.0], &["lang::polish"]).unwrap();
    c.add_tags(&[chat.0], &["lang::french"]).unwrap();
    let f = fields(&c, builtin::basic_and_reversed());
    c.add_note_to_deck(
        spanish,
        builtin::basic_and_reversed(),
        &[(f[0], "gato"), (f[1], "cat")],
    )
    .unwrap();
    clock.advance(1);
    let f = fields(&c, builtin::cloze());
    c.add_note_to_deck(
        default_deck(),
        builtin::cloze(),
        &[(f[0], "The {{c1::cat}} sat on the {{c2::mat}}")],
    )
    .unwrap();
    clock.advance(1);
    c.answer(kot.1, Rating::Good, 1_000).unwrap();
    clock.advance(11 * MINUTE);
    c.answer(kot.1, Rating::Good, 1_000).unwrap();
    clock.advance(1);
    c.answer(pies.1, Rating::Again, 1_000).unwrap();
    Fixture {
        c,
        clock,
        polish,
        animals,
        kot,
        pies,
    }
}

use crate::deck::default_deck;

// ---- The grammar ----

#[test]
fn an_empty_query_finds_every_card() {
    let f = fixture();
    assert_eq!(titles(&f.c, "").len(), 9);
    assert_eq!(titles(&f.c, "   "), titles(&f.c, ""));
}

fn tree(query: &str) -> Node {
    parse(query).unwrap()
}

fn text(word: &str) -> Node {
    Node::Term(Term::Text(word.to_owned()))
}

#[test]
fn and_binds_tighter_than_or_and_brackets_group() {
    assert_eq!(
        tree("a b or c"),
        Node::Or(vec![Node::And(vec![text("a"), text("b")]), text("c")])
    );
    assert_eq!(
        tree("a (b or c)"),
        Node::And(vec![text("a"), Node::Or(vec![text("b"), text("c")])])
    );
    assert_eq!(tree("a and b"), tree("a b"));
    assert_eq!(tree("a OR b"), tree("a or b"));
}

#[test]
fn not_and_minus_apply_to_one_term_or_group() {
    assert_eq!(tree("-a"), Node::Not(Box::new(text("a"))));
    assert_eq!(tree("not a"), tree("-a"));
    assert_eq!(
        tree("-(a or b)"),
        Node::Not(Box::new(Node::Or(vec![text("a"), text("b")])))
    );
    assert_eq!(
        tree("- -a"),
        Node::And(vec![text("-"), Node::Not(Box::new(text("a")))]),
        "a dash with a space after it is text"
    );
}

#[test]
fn quotes_make_keywords_and_spaces_into_text() {
    assert_eq!(tree("\"or\""), text("or"));
    assert_eq!(tree("\"two words\""), text("two words"));
    assert_eq!(tree("\"a:b\""), text("a:b"));
    assert_eq!(
        tree("say \\\"hi\\\""),
        Node::And(vec![text("say"), text("\"hi\"")])
    );
    assert_eq!(
        tree("\"my field\":x"),
        Node::Term(Term::Field {
            name: "my field".to_owned(),
            text: "x".to_owned()
        })
    );
    assert_eq!(
        tree("deck:\"My Deck\""),
        Node::Term(Term::Deck {
            pattern: "My Deck".to_owned(),
            subdecks: true
        })
    );
}

#[test]
fn filter_names_ignore_case_and_unknown_names_are_fields() {
    assert_eq!(tree("DECK:a"), tree("deck:a"));
    assert!(matches!(tree("front:a"), Node::Term(Term::Field { .. })));
}

fn error_position(query: &str) -> usize {
    match parse(query).unwrap_err() {
        SearchError::Syntax { position, .. } => position,
        other => panic!("{other:?}"),
    }
}

#[test]
fn syntax_errors_point_at_the_problem() {
    assert_eq!(error_position("a \"b"), 2);
    assert_eq!(error_position("a (b or c"), 2);
    assert_eq!(error_position("a b)"), 3);
    assert_eq!(error_position("a or"), 4);
    assert_eq!(error_position("or a"), 0);
    assert_eq!(error_position("a and"), 2);
    assert_eq!(error_position("a or or b"), 5);
    assert_eq!(error_position("(a -)"), 4);
    assert_eq!(error_position(":x"), 0);
    assert_eq!(error_position("a \"\""), 2);
    assert_eq!(error_position("a is:fresh"), 2);
    assert_eq!(error_position("a due:soon"), 2);
    assert_eq!(error_position("a added:0"), 2);
    assert_eq!(error_position("a rated:7:9"), 2);
    assert_eq!(error_position("a deck:"), 2);
    assert_eq!(error_position("a lapses:many"), 2);
    assert_eq!(error_position("a due:.."), 2);
    assert_eq!(error_position("a added:2023-02-30"), 2);
}

#[test]
fn error_messages_say_what_to_do() {
    for query in [
        "is:fresh",
        "due:soon",
        "rated:7:9",
        "lapses:many",
        "(a",
        "a)",
    ] {
        let message = parse(query).unwrap_err().to_string();
        assert!(message.contains("at character"), "{message}");
        assert!(message.len() > 30, "{message}");
    }
    assert!(check_query("deck:Polish tag:a or -b").is_ok());
    assert!(check_query("(").is_err());
}

#[test]
fn dates_become_day_numbers() {
    assert_eq!(super::parse::date_number(1970, 1, 1), Some(0));
    assert_eq!(super::parse::date_number(2023, 11, 14), Some(19_675));
    assert_eq!(super::parse::date_number(2024, 2, 29), Some(19_782));
    assert_eq!(super::parse::date_number(2023, 2, 29), None);
    assert_eq!(super::parse::date_number(1969, 12, 31), Some(-1));
}

// ---- Text and fields ----

#[test]
fn text_matches_any_field_ignoring_case_in_every_alphabet() {
    let f = fixture();
    assert_eq!(titles(&f.c, "kot"), ["kot"]);
    assert_eq!(titles(&f.c, "KOT"), ["kot"]);
    assert_eq!(titles(&f.c, "łódź"), ["Łódź"]);
    assert_eq!(titles(&f.c, "ŁÓDŹ"), ["Łódź"]);
    assert_eq!(titles(&f.c, "city"), ["Łódź"], "the back field too");
    assert_eq!(titles(&f.c, "ódź"), ["Łódź"], "inside a word");
}

#[test]
fn text_reads_what_a_person_reads_not_the_html() {
    let f = fixture();
    assert_eq!(
        titles(&f.c, "dom mały"),
        ["dom mały"],
        "across a tag and a space entity"
    );
    assert!(titles(&f.c, "<b>").is_empty());
    assert!(titles(&f.c, "nbsp").is_empty());
}

#[test]
fn text_has_wildcards_and_quotes() {
    let f = fixture();
    assert_eq!(titles(&f.c, "k*t"), ["kot"]);
    assert_eq!(titles(&f.c, "\"a city\""), ["Łódź"]);
    assert_eq!(
        titles(&f.c, "a city"),
        ["Łódź"],
        "two words, each found anywhere"
    );
}

#[test]
fn several_words_must_all_be_found_but_may_be_in_different_fields() {
    let f = fixture();
    assert_eq!(titles(&f.c, "kot cat"), ["kot"]);
    assert!(titles(&f.c, "kot dog").is_empty());
}

#[test]
fn a_named_field_matches_inside_that_field_only() {
    let f = fixture();
    assert_eq!(titles(&f.c, "front:kot"), ["kot"]);
    assert!(titles(&f.c, "back:kot").is_empty());
    assert_eq!(
        titles(&f.c, "BACK:cat").len(),
        4,
        "kot, chat and the two cards of gato (the cloze has no Back)"
    );
    assert_eq!(titles(&f.c, "front:\"ł*ź\""), ["Łódź"]);
}

#[test]
fn an_empty_field_can_be_found() {
    let f = fixture();
    let (_, card) = basic(&f.c, &f.clock, default_deck(), "no back", "");
    let found = f.c.search("back:", &SearchOptions::default()).unwrap();
    assert_eq!(found.rows.len(), 1);
    assert_eq!(found.rows[0].card, card);
    assert_eq!(
        titles(&f.c, "-back:").len(),
        9,
        "every other card has a Back or is not a card with that field... "
    );
}

#[test]
fn an_unknown_field_is_an_error_with_a_hint_for_a_misspelt_filter() {
    let f = fixture();
    match failure(&f.c, "decks:Polish") {
        SearchError::UnknownField { name, suggestion } => {
            assert_eq!(name, "decks");
            assert_eq!(suggestion, Some("deck"));
        }
        other => panic!("{other:?}"),
    }
    match failure(&f.c, "nothing:x") {
        SearchError::UnknownField { suggestion, .. } => assert_eq!(suggestion, None),
        other => panic!("{other:?}"),
    }
    let message = failure(&f.c, "decks:Polish").to_string();
    assert!(
        message.contains("deck:") && message.contains("quotes"),
        "{message}"
    );
    assert_eq!(titles(&f.c, "\"nothing:x\""), Vec::<String>::new());
}

#[test]
fn accents_do_not_matter_anywhere() {
    let f = fixture();
    assert_eq!(titles(&f.c, "lodz"), ["Łódź"]);
    assert_eq!(titles(&f.c, "LODZ"), ["Łódź"]);
    assert_eq!(titles(&f.c, "front:lodz"), ["Łódź"]);
    assert_eq!(titles(&f.c, "lódz"), ["Łódź"], "a half-right guess");
    let (note, card) = basic(&f.c, &f.clock, default_deck(), "ręka", "hand");
    f.c.add_tags(&[note], &["żółty"]).unwrap();
    assert_eq!(titles(&f.c, "reka"), ["ręka"]);
    assert_eq!(titles(&f.c, "tag:zolty"), ["ręka"]);
    assert_eq!(titles(&f.c, "tag:ŻÓŁTY"), ["ręka"]);
    let deck = f.c.create_deck("Książki", None).unwrap();
    f.c.move_cards(&[card], deck).unwrap();
    assert_eq!(titles(&f.c, "deck:ksiazki"), ["ręka"]);
    assert_eq!(titles(&f.c, "deck:ksi"), ["ręka"]);
}

// ---- Decks, tags, note types, templates ----

#[test]
fn a_deck_includes_what_is_inside_it() {
    let f = fixture();
    assert_eq!(titles(&f.c, "deck:Polish"), ["kot", "pies", "Łódź"]);
    assert_eq!(titles(&f.c, "deckonly:Polish"), ["Łódź"]);
    assert_eq!(titles(&f.c, "deck:polish::animals"), ["kot", "pies"]);
    assert_eq!(titles(&f.c, "deckonly:Polish::Animals"), ["kot", "pies"]);
    assert_eq!(
        titles(&f.c, "deck:Animals"),
        ["kot", "pies"],
        "any part of the path"
    );
    assert_eq!(titles(&f.c, "deck:nimal"), ["kot", "pies"]);
    assert_eq!(titles(&f.c, "deckonly:Animals"), ["kot", "pies"]);
    assert!(titles(&f.c, "deck:Nope").is_empty());
}

#[test]
fn the_default_deck_holds_what_has_no_deck() {
    let f = fixture();
    assert_eq!(titles(&f.c, "deck:default").len(), 3);
    // A card with no deck register reads as being in the Default deck.
    f.c.conn
        .execute("INSERT INTO write_guard (id) VALUES (1)", [])
        .unwrap();
    f.c.conn
        .execute("UPDATE card SET deck = x'' WHERE id = ?1", [f.pies.1])
        .unwrap();
    f.c.conn.execute("DELETE FROM write_guard", []).unwrap();
    assert_eq!(titles(&f.c, "deck:default").len(), 4);
    assert_eq!(titles(&f.c, "deck:Polish"), ["kot", "Łódź"]);
}

#[test]
fn deck_names_take_wildcards_and_quotes() {
    let f = fixture();
    f.c.create_deck("My Deck", None).unwrap();
    assert_eq!(titles(&f.c, "deck:*").len(), 9);
    assert_eq!(titles(&f.c, "deck:Fr*"), ["chat"]);
    assert_eq!(titles(&f.c, "deck:\"my deck\""), Vec::<String>::new());
    assert_eq!(titles(&f.c, "deck:*::animals"), ["kot", "pies"]);
}

#[test]
fn a_moved_card_follows_its_deck() {
    let f = fixture();
    f.c.move_cards(&[f.pies.1], f.polish).unwrap();
    assert_eq!(titles(&f.c, "deck:Polish::Animals"), ["kot"]);
    assert_eq!(titles(&f.c, "deckonly:Polish"), ["pies", "Łódź"]);
    let _ = f.animals;
}

#[test]
fn a_tag_includes_the_tags_inside_it() {
    let f = fixture();
    assert_eq!(titles(&f.c, "tag:lang"), ["chat", "kot", "Łódź"]);
    assert_eq!(titles(&f.c, "tag:lang::polish"), ["kot", "Łódź"]);
    assert_eq!(titles(&f.c, "tag:LANG::French"), ["chat"]);
    assert_eq!(titles(&f.c, "tag:animals"), ["kot"]);
    assert_eq!(
        titles(&f.c, "tag:lan"),
        ["chat", "kot", "Łódź"],
        "any part of a tag"
    );
    assert_eq!(titles(&f.c, "tag:polish"), ["kot", "Łódź"]);
    assert_eq!(titles(&f.c, "tag:l*"), ["chat", "kot", "Łódź"]);
}

#[test]
fn no_tag_at_all_is_a_negated_wildcard() {
    let f = fixture();
    assert_eq!(titles(&f.c, "-tag:*").len(), 6);
    assert_eq!(titles(&f.c, "tag:*").len(), 3);
}

#[test]
fn a_removed_tag_no_longer_matches() {
    let f = fixture();
    f.c.remove_tags(&[f.kot.0], &["animals"]).unwrap();
    assert!(titles(&f.c, "tag:animals").is_empty());
    assert_eq!(titles(&f.c, "tag:lang::polish"), ["kot", "Łódź"]);
}

#[test]
fn note_types_and_templates() {
    let f = fixture();
    assert_eq!(
        titles(&f.c, "note:Basic").len(),
        7,
        "Basic and Basic and reversed"
    );
    assert_eq!(titles(&f.c, "note:basic").len(), 7, "ignoring case");
    assert_eq!(titles(&f.c, "note:reversed").len(), 2, "part of a name");
    assert_eq!(titles(&f.c, "note:\"Basic and reversed*\"").len(), 2);
    assert_eq!(titles(&f.c, "note:Basic*").len(), 7);
    assert_eq!(titles(&f.c, "note:Cloze").len(), 2);
    assert!(titles(&f.c, "note:Nope").is_empty());
    // The second template of a standard type, and cloze number 2.
    assert_eq!(titles(&f.c, "card:2").len(), 2);
    assert_eq!(
        titles(&f.c, "card:1").len(),
        7,
        "every first template and c1"
    );
    assert_eq!(titles(&f.c, "card:3").len(), 0);
    assert_eq!(
        titles(&f.c, "\"card:Card 2\"").len(),
        0,
        "quoted: it is text"
    );
    assert_eq!(titles(&f.c, "card:\"Card 2\"").len(), 1);
}

// ---- States ----

#[test]
fn card_states() {
    let f = fixture();
    assert_eq!(titles(&f.c, "is:review"), ["kot"]);
    assert_eq!(titles(&f.c, "is:learning"), ["pies"]);
    assert_eq!(titles(&f.c, "is:new").len(), 7);
    assert_eq!(titles(&f.c, "is:learn"), ["pies"], "short for learning");
}

#[test]
fn relearning_cards_are_learning_cards() {
    let f = fixture();
    f.clock.advance(3 * DAY);
    f.c.answer(f.kot.1, Rating::Again, 1_000).unwrap();
    assert_eq!(titles(&f.c, "is:learning"), ["kot", "pies"]);
    assert!(titles(&f.c, "is:review").is_empty());
}

#[test]
fn due_cards_are_the_overdue_and_today_in_every_state() {
    let f = fixture();
    assert!(
        titles(&f.c, "is:due").is_empty(),
        "kot is due in two days, pies in a minute"
    );
    f.clock.advance(2 * MINUTE);
    assert_eq!(titles(&f.c, "is:due"), ["pies"]);
    f.clock.advance(3 * DAY);
    assert_eq!(titles(&f.c, "is:due"), ["kot", "pies"]);
}

#[test]
fn due_leaves_out_suspended_and_buried_cards() {
    let f = fixture();
    f.clock.advance(3 * DAY);
    assert_eq!(titles(&f.c, "is:due"), ["kot", "pies"]);
    f.c.suspend_cards(&[f.pies.1]).unwrap();
    assert_eq!(titles(&f.c, "is:due"), ["kot"]);
    f.c.bury_cards(&[f.kot.1]).unwrap();
    assert!(titles(&f.c, "is:due").is_empty());
    assert_eq!(
        titles(&f.c, "is:suspended or is:buried"),
        ["kot", "pies"],
        "still found"
    );
    f.c.unsuspend_cards(&[f.pies.1]).unwrap();
    assert_eq!(titles(&f.c, "is:due"), ["pies"]);
    f.clock.advance(DAY);
    assert_eq!(
        titles(&f.c, "is:due"),
        ["kot", "pies"],
        "burying lasts until tomorrow"
    );
}

#[test]
fn suspended_and_buried_cards() {
    let f = fixture();
    f.c.suspend_cards(&[f.pies.1]).unwrap();
    f.c.bury_cards(&[f.kot.1]).unwrap();
    assert_eq!(titles(&f.c, "is:suspended"), ["pies"]);
    assert_eq!(titles(&f.c, "is:buried"), ["kot"]);
    assert_eq!(titles(&f.c, "-is:suspended -is:buried").len(), 7);
    f.clock.advance(DAY);
    assert!(
        titles(&f.c, "is:buried").is_empty(),
        "burying lasts until tomorrow"
    );
}

// ---- Days ----

#[test]
fn due_days_count_from_today() {
    let f = fixture();
    // kot graduated at 09:11 on day 0 with a two-day interval, so it is due on day 2 (fuzz does
    // not apply below three days). pies is learning, due a minute after its answer, today.
    let kot_due = f.c.card_schedule(f.kot.1).unwrap().due_day.unwrap() - DAY0 / DAY;
    assert_eq!(kot_due, 2);
    assert_eq!(titles(&f.c, "due:2"), ["kot"]);
    assert_eq!(
        titles(&f.c, "due:0"),
        ["pies"],
        "a learning card by the day its time falls in"
    );
    assert_eq!(titles(&f.c, "due:0..2"), ["kot", "pies"]);
    assert_eq!(titles(&f.c, "due:..1"), ["pies"]);
    assert_eq!(titles(&f.c, "due:1.."), ["kot"]);
    assert!(titles(&f.c, "due:-1").is_empty());
    assert_eq!(titles(&f.c, "due:2023-11-16"), ["kot"]);
    f.clock.advance(5 * DAY);
    assert_eq!(titles(&f.c, "due:-3"), ["kot"], "overdue by three days");
    assert_eq!(titles(&f.c, "due:..0"), ["kot", "pies"]);
}

#[test]
fn added_counts_days_back_from_today() {
    let (c, clock) = setup();
    basic(&c, &clock, default_deck(), "old", "x");
    clock.advance(10 * DAY);
    basic(&c, &clock, default_deck(), "new", "x");
    assert_eq!(titles(&c, "added:1"), ["new"]);
    assert_eq!(
        titles(&c, "added:10"),
        ["new"],
        "the last ten days: today and nine before"
    );
    assert_eq!(titles(&c, "added:11"), ["new", "old"]);
    assert_eq!(titles(&c, "added:-12..-5"), ["old"]);
    assert_eq!(titles(&c, "added:2023-11-14"), ["old"]);
    assert_eq!(titles(&c, "added:2023-11-15..2023-11-30"), ["new"]);
    assert_eq!(titles(&c, "added:..2023-11-30"), ["new", "old"]);
    assert!(titles(&c, "added:2023-11-01..2023-11-13").is_empty());
}

#[test]
fn rated_and_introduced_read_the_review_history() {
    let f = fixture();
    assert_eq!(titles(&f.c, "rated:1"), ["kot", "pies"]);
    assert_eq!(titles(&f.c, "rated:1:1"), ["pies"], "answered Again");
    assert_eq!(titles(&f.c, "rated:1:again"), ["pies"]);
    assert_eq!(titles(&f.c, "rated:1:3"), ["kot"]);
    assert_eq!(titles(&f.c, "introduced:1"), ["kot", "pies"]);
    f.clock.advance(3 * DAY);
    f.c.answer(f.kot.1, Rating::Hard, 1_000).unwrap();
    assert_eq!(titles(&f.c, "rated:1"), ["kot"], "only today");
    assert_eq!(titles(&f.c, "rated:4"), ["kot", "pies"]);
    assert_eq!(titles(&f.c, "rated:1:2"), ["kot"]);
    assert_eq!(
        titles(&f.c, "introduced:1"),
        Vec::<String>::new(),
        "kot was introduced on day 0"
    );
    assert_eq!(titles(&f.c, "introduced:4"), ["kot", "pies"]);
    assert_eq!(
        titles(&f.c, "rated:-3..-3"),
        ["kot", "pies"],
        "three days ago"
    );
}

#[test]
fn an_undone_answer_does_not_count() {
    let f = fixture();
    // The last answer made was pies's Again.
    f.c.undo_answer().unwrap().unwrap();
    assert!(titles(&f.c, "rated:1:1").is_empty());
    assert_eq!(titles(&f.c, "introduced:1"), ["kot"]);
    // Then kot's second Good: it was answered once more before that.
    f.c.undo_answer().unwrap().unwrap();
    assert_eq!(titles(&f.c, "rated:1:3"), ["kot"]);
    assert_eq!(titles(&f.c, "reviews:1"), ["kot"]);
    f.c.undo_answer().unwrap().unwrap();
    assert!(titles(&f.c, "rated:1").is_empty());
    assert!(titles(&f.c, "introduced:1").is_empty());
}

#[test]
fn numbers_compare_scheduling_facts() {
    let f = fixture();
    f.clock.advance(3 * DAY);
    f.c.answer(f.kot.1, Rating::Again, 1_000).unwrap();
    assert_eq!(titles(&f.c, "lapses:1"), ["kot"]);
    assert_eq!(titles(&f.c, "lapses:0").len(), 8, "new cards have none");
    assert_eq!(titles(&f.c, "lapses:>=1"), ["kot"]);
    assert_eq!(titles(&f.c, "reviews:3"), ["kot"]);
    assert_eq!(titles(&f.c, "reviews:1..3"), ["kot", "pies"]);
    assert_eq!(titles(&f.c, "reviews:>1"), ["kot"]);
    assert_eq!(titles(&f.c, "reviews:=0").len(), 7);
    let d = f.c.card_schedule(f.kot.1).unwrap().memory.unwrap();
    let below = format!("difficulty:<{}", d.difficulty + 0.5);
    assert!(titles(&f.c, &below).contains(&"kot".to_owned()));
    assert!(
        !titles(&f.c, &format!("difficulty:>{}", d.difficulty + 0.5)).contains(&"kot".to_owned())
    );
    assert_eq!(titles(&f.c, "stability:>0").len(), 2);
    assert!(
        titles(&f.c, "-difficulty:>0").len() >= 7,
        "new cards have no difficulty, so they are not above anything"
    );
}

// ---- Boolean logic ----

#[test]
fn or_not_and_brackets_combine_filters() {
    let f = fixture();
    assert_eq!(titles(&f.c, "kot or pies"), ["kot", "pies"]);
    assert_eq!(titles(&f.c, "deck:Polish -kot"), ["pies", "Łódź"]);
    assert_eq!(titles(&f.c, "deck:Polish not kot"), ["pies", "Łódź"]);
    assert_eq!(
        titles(&f.c, "deck:Polish::Animals (is:review or is:learning)"),
        ["kot", "pies"]
    );
    assert_eq!(
        titles(&f.c, "deck:Polish::Animals is:review or chat"),
        ["chat", "kot"]
    );
    assert_eq!(titles(&f.c, "-(deck:Polish or deck:Default)").len(), 3);
    assert_eq!(titles(&f.c, "tag:lang -tag:lang::polish"), ["chat"]);
    assert_eq!(titles(&f.c, "kot and tag:animals and deck:Polish"), ["kot"]);
    assert!(titles(&f.c, "kot pies").is_empty());
}

#[test]
fn negating_a_filter_that_can_be_empty_keeps_the_cards_it_does_not_apply_to() {
    let f = fixture();
    assert_eq!(titles(&f.c, "-is:review").len(), 8);
    assert_eq!(titles(&f.c, "-due:2").len(), 8);
    assert_eq!(titles(&f.c, "-rated:1").len(), 7);
    assert_eq!(titles(&f.c, "-lapses:>0").len(), 9);
}

// ---- Cards, notes and what is excluded ----

#[test]
fn a_deleted_note_is_not_found_and_comes_back_when_restored() {
    let f = fixture();
    f.c.delete_note(f.kot.0).unwrap();
    assert!(titles(&f.c, "kot").is_empty());
    assert_eq!(titles(&f.c, "").len(), 8);
    f.c.restore_note(f.kot.0).unwrap();
    assert_eq!(titles(&f.c, "kot"), ["kot"]);
}

#[test]
fn notes_mode_gives_one_row_per_note() {
    let f = fixture();
    let page = |query: &str| {
        f.c.search(
            query,
            &SearchOptions {
                mode: Mode::Notes,
                limit: MAX_LIMIT,
                ..Default::default()
            },
        )
        .unwrap()
    };
    assert_eq!(page("").total, 7);
    assert_eq!(page("").rows.len(), 7);
    assert_eq!(page("note:\"Basic and reversed\"").total, 1);
    assert_eq!(page("note:Cloze").total, 1);
    assert_eq!(page("card:2").total, 2, "a note counts if any card matches");
    assert_eq!(f.c.search("", &SearchOptions::default()).unwrap().total, 9);
}

#[test]
fn rows_say_what_a_list_needs() {
    let f = fixture();
    let rows =
        f.c.search(
            "kot or pies",
            &SearchOptions {
                sort: Sort::ascending(SortKey::SortField),
                ..Default::default()
            },
        )
        .unwrap()
        .rows;
    assert_eq!(rows[0].title, "kot");
    assert_eq!(rows[0].card, f.kot.1);
    assert_eq!(rows[0].note, f.kot.0);
    assert_eq!(rows[0].note_type, builtin::basic());
    assert_eq!(rows[0].deck, f.animals);
    assert_eq!(rows[0].state, crate::scheduling::CardState::Review);
    assert_eq!(rows[0].due_day, Some(DAY0 / DAY + 2));
    assert_eq!(rows[1].state, crate::scheduling::CardState::Learning);
    assert!(rows[1].due_ms.is_some());
    assert!(!rows[0].suspended && !rows[0].buried);
    f.c.suspend_cards(&[f.kot.1]).unwrap();
    let rows = f.c.search("kot", &SearchOptions::default()).unwrap().rows;
    assert!(rows[0].suspended);
}

#[test]
fn the_title_is_plain_text() {
    let f = fixture();
    let rows = f.c.search("dom", &SearchOptions::default()).unwrap().rows;
    assert_eq!(rows[0].title, "dom mały");
}

// ---- Sorting and paging ----

#[test]
fn sort_by_created_follows_when_notes_were_added() {
    let f = fixture();
    let all = |sort| sorted(&f.c, "note:Basic -note:reversed", sort, Mode::Cards);
    assert_eq!(
        all(Sort::ascending(SortKey::Created)),
        ["kot", "pies", "Łódź", "dom mały", "chat"]
    );
    assert_eq!(
        all(Sort::descending(SortKey::Created)),
        ["chat", "dom mały", "Łódź", "pies", "kot"]
    );
    assert_eq!(
        all(Sort::default()),
        all(Sort::descending(SortKey::Created))
    );
}

#[test]
fn sort_by_the_sort_field_ignores_case_and_ignores_tags() {
    let f = fixture();
    assert_eq!(
        sorted(
            &f.c,
            "note:Basic -note:reversed",
            Sort::ascending(SortKey::SortField),
            Mode::Cards
        ),
        ["chat", "dom mały", "kot", "Łódź", "pies"],
        "Ł sorts with L, not after Z"
    );
    assert_eq!(
        sorted(
            &f.c,
            "note:Basic -note:reversed",
            Sort::descending(SortKey::SortField),
            Mode::Cards
        )[0],
        "pies"
    );
}

#[test]
fn sort_by_due_puts_cards_without_a_due_date_last_in_both_directions() {
    let f = fixture();
    let asc = sorted(
        &f.c,
        "note:Basic -note:reversed",
        Sort::ascending(SortKey::Due),
        Mode::Cards,
    );
    assert_eq!(
        &asc[..2],
        ["pies", "kot"],
        "a minute from now, then in two days"
    );
    let desc = sorted(
        &f.c,
        "note:Basic -note:reversed",
        Sort::descending(SortKey::Due),
        Mode::Cards,
    );
    assert_eq!(&desc[..2], ["kot", "pies"]);
    assert_eq!(asc[2..].len(), 3);
    assert_eq!(desc[2..].len(), 3);
    assert_eq!(asc[2..], desc[2..], "new cards in ID order either way");
}

#[test]
fn sort_by_deck_follows_the_deck_list() {
    let f = fixture();
    let order = sorted(
        &f.c,
        "note:Basic -note:reversed",
        Sort::ascending(SortKey::Deck),
        Mode::Cards,
    );
    // Default, French, Polish, Polish::Animals (two cards), Spanish is not Basic.
    assert_eq!(order[0], "dom mały");
    assert_eq!(order[1], "chat");
    assert_eq!(order[2], "Łódź");
    let mut last = order[3..].to_vec();
    last.sort();
    assert_eq!(last, ["kot", "pies"]);
}

#[test]
fn sort_by_note_type_and_by_numbers() {
    let f = fixture();
    let by_type = sorted(&f.c, "", Sort::ascending(SortKey::NoteType), Mode::Cards);
    assert_eq!(by_type.len(), 9);
    assert_eq!(
        sorted(
            &f.c,
            "is:review or is:learning",
            Sort::descending(SortKey::Reviews),
            Mode::Cards
        ),
        ["kot", "pies"]
    );
    assert_eq!(
        sorted(
            &f.c,
            "is:review or is:learning",
            Sort::ascending(SortKey::Reviews),
            Mode::Cards
        ),
        ["pies", "kot"]
    );
    let by_stability = sorted(&f.c, "", Sort::descending(SortKey::Stability), Mode::Cards);
    assert_eq!(by_stability[0], "kot", "the card with a memory comes first");
    for key in [SortKey::Difficulty, SortKey::Lapses] {
        assert_eq!(sorted(&f.c, "", Sort::ascending(key), Mode::Cards).len(), 9);
        assert_eq!(
            sorted(&f.c, "", Sort::descending(key), Mode::Notes).len(),
            7
        );
    }
}

#[test]
fn a_shuffle_is_the_same_for_the_same_seed() {
    let f = fixture();
    let shuffled = |seed| {
        f.c.search(
            "",
            &SearchOptions {
                sort: Sort::ascending(SortKey::Random),
                seed,
                limit: MAX_LIMIT,
                ..Default::default()
            },
        )
        .unwrap()
        .rows
        .into_iter()
        .map(|r| r.card)
        .collect::<Vec<_>>()
    };
    assert_eq!(shuffled(1), shuffled(1));
    let (a, b) = (shuffled(1), shuffled(2));
    assert_ne!(a, b);
    let (mut sa, mut sb) = (a, b);
    sa.sort();
    sb.sort();
    assert_eq!(sa, sb, "the same cards in another order");
}

#[test]
fn notes_mode_sorts_by_what_the_note_has_best() {
    let f = fixture();
    let by_due = sorted(&f.c, "", Sort::ascending(SortKey::Due), Mode::Notes);
    assert_eq!(&by_due[..2], ["pies", "kot"]);
    assert_eq!(by_due.len(), 7);
    let by_created = sorted(&f.c, "", Sort::ascending(SortKey::Created), Mode::Notes);
    assert_eq!(by_created[0], "kot");
    assert_eq!(by_created[5], "gato");
    assert!(by_created[6].starts_with("The"));
}

#[test]
fn paging_returns_pages_of_one_ordering_and_the_whole_total() {
    let f = fixture();
    let page = |offset, limit| {
        f.c.search(
            "",
            &SearchOptions {
                sort: Sort::ascending(SortKey::Created),
                offset,
                limit,
                ..Default::default()
            },
        )
        .unwrap()
    };
    let all = page(0, 100);
    assert_eq!(all.total, 9);
    let first = page(0, 4);
    let second = page(4, 4);
    let third = page(8, 4);
    assert_eq!(first.total, 9);
    assert_eq!(second.total, 9);
    let joined: Vec<_> = first
        .rows
        .into_iter()
        .chain(second.rows)
        .chain(third.rows)
        .collect();
    assert_eq!(joined, all.rows);
    assert!(page(20, 4).rows.is_empty());
    let count_only = page(0, 0);
    assert_eq!(count_only.total, 9);
    assert!(count_only.rows.is_empty());
}

#[test]
fn a_page_is_never_longer_than_the_limit_allows() {
    let (c, clock) = setup();
    for i in 0..3 {
        basic(&c, &clock, default_deck(), &format!("n{i}"), "b");
    }
    let page = c
        .search(
            "",
            &SearchOptions {
                limit: u32::MAX,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(page.rows.len(), 3);
}

#[test]
fn sorts_read_and_show_as_text() {
    assert_eq!(Sort::parse("due").unwrap(), Sort::ascending(SortKey::Due));
    assert_eq!(
        Sort::parse("Due:DESC").unwrap(),
        Sort::descending(SortKey::Due)
    );
    assert_eq!(Sort::default().to_string(), "created:desc");
    for key in SortKey::ALL {
        for descending in [false, true] {
            let sort = Sort { key, descending };
            assert_eq!(Sort::parse(&sort.to_string()).unwrap(), sort);
        }
    }
    assert!(matches!(
        Sort::parse("size"),
        Err(SearchError::UnknownSort(_))
    ));
    assert!(matches!(
        Sort::parse("due:up"),
        Err(SearchError::UnknownSort(_))
    ));
}

// ---- Saved searches ----

#[test]
fn a_saved_search_is_kept_run_changed_and_deleted() {
    let f = fixture();
    let id =
        f.c.save_search(
            "Polish verbs",
            "deck:Polish is:review",
            Sort::ascending(SortKey::Due),
            Mode::Notes,
        )
        .unwrap();
    let saved = f.c.saved_search(id).unwrap().unwrap();
    assert_eq!(saved.name, "Polish verbs");
    assert_eq!(saved.query, "deck:Polish is:review");
    assert_eq!(saved.sort, Sort::ascending(SortKey::Due));
    assert_eq!(saved.mode, Mode::Notes);
    assert_eq!(f.c.saved_searches().unwrap(), vec![saved]);

    let page = f.c.run_saved_search(id, 0, 10).unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.rows[0].title, "kot");

    f.c.update_saved_search(
        id,
        &SavedSearchChange {
            name: Some("Reviews".to_owned()),
            query: Some("is:review or is:learning".to_owned()),
            ..Default::default()
        },
    )
    .unwrap();
    let saved = f.c.saved_search(id).unwrap().unwrap();
    assert_eq!((saved.name.as_str(), saved.mode), ("Reviews", Mode::Notes));
    assert_eq!(f.c.run_saved_search(id, 0, 10).unwrap().total, 2);

    f.c.delete_saved_search(id).unwrap();
    assert!(f.c.saved_searches().unwrap().is_empty());
    assert_eq!(
        f.c.run_saved_search(id, 0, 10).unwrap_err(),
        SearchError::NotFound
    );
    assert_eq!(
        f.c.delete_saved_search(id).unwrap_err(),
        SearchError::NotFound
    );
}

#[test]
fn saved_searches_are_listed_by_name_ignoring_case() {
    let f = fixture();
    for name in ["b", "A", "c"] {
        f.c.save_search(name, "", Sort::default(), Mode::Cards)
            .unwrap();
    }
    let names: Vec<String> =
        f.c.saved_searches()
            .unwrap()
            .into_iter()
            .map(|s| s.name)
            .collect();
    assert_eq!(names, ["A", "b", "c"]);
}

#[test]
fn a_saved_search_needs_a_free_name_and_a_query_that_reads() {
    let f = fixture();
    let id =
        f.c.save_search("  Due  ", "is:due", Sort::default(), Mode::Cards)
            .unwrap();
    assert_eq!(
        f.c.saved_search(id).unwrap().unwrap().name,
        "Due",
        "trimmed"
    );
    assert_eq!(
        f.c.save_search("due", "", Sort::default(), Mode::Cards)
            .unwrap_err(),
        SearchError::NameTaken("due".to_owned())
    );
    assert_eq!(
        f.c.save_search("  ", "", Sort::default(), Mode::Cards)
            .unwrap_err(),
        SearchError::EmptyName
    );
    assert!(matches!(
        f.c.save_search("Bad", "(", Sort::default(), Mode::Cards),
        Err(SearchError::Syntax { .. })
    ));
    let other =
        f.c.save_search("Other", "", Sort::default(), Mode::Cards)
            .unwrap();
    assert_eq!(
        f.c.update_saved_search(
            other,
            &SavedSearchChange {
                name: Some("DUE".to_owned()),
                ..Default::default()
            }
        )
        .unwrap_err(),
        SearchError::NameTaken("DUE".to_owned())
    );
    assert!(matches!(
        f.c.update_saved_search(
            other,
            &SavedSearchChange {
                query: Some("a)".to_owned()),
                ..Default::default()
            }
        ),
        Err(SearchError::Syntax { .. })
    ));
    // Renaming a search to its own name (other case) is allowed.
    f.c.update_saved_search(
        id,
        &SavedSearchChange {
            name: Some("DUE".to_owned()),
            ..Default::default()
        },
    )
    .unwrap();
}

#[test]
fn changing_one_part_writes_one_register() {
    let f = fixture();
    let id =
        f.c.save_search("S", "kot", Sort::default(), Mode::Cards)
            .unwrap();
    let clock = |field| {
        f.c.register_clock("saved_search", id, field)
            .unwrap()
            .unwrap()
    };
    let (name, query) = (clock("name"), clock("query"));
    f.clock.advance(5);
    f.c.update_saved_search(
        id,
        &SavedSearchChange {
            query: Some("pies".to_owned()),
            ..Default::default()
        },
    )
    .unwrap();
    f.c.update_saved_search(
        id,
        &SavedSearchChange {
            name: Some("S".to_owned()),
            mode: Some(Mode::Cards),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(clock("name"), name, "the same name writes nothing");
    assert!(clock("query").hlc > query.hlc);
}

#[test]
fn a_saved_search_that_a_newer_app_wrote_still_reads() {
    let f = fixture();
    let id =
        f.c.save_search("S", "kot", Sort::default(), Mode::Cards)
            .unwrap();
    f.c.write(|w| {
        w.set(
            "saved_search",
            id,
            "sort",
            rusqlite::types::Value::Text("tomorrow:sideways".to_owned()),
        )?;
        w.set(
            "saved_search",
            id,
            "mode",
            rusqlite::types::Value::Text("cubes".to_owned()),
        )
    })
    .unwrap();
    let saved = f.c.saved_search(id).unwrap().unwrap();
    assert_eq!((saved.sort, saved.mode), (Sort::default(), Mode::Cards));
    assert_eq!(f.c.run_saved_search(id, 0, 10).unwrap().total, 1);
}

#[test]
fn a_saved_search_whose_query_stopped_reading_reports_why() {
    let f = fixture();
    let id =
        f.c.save_search("S", "kot", Sort::default(), Mode::Cards)
            .unwrap();
    f.c.write(|w| {
        w.set(
            "saved_search",
            id,
            "query",
            rusqlite::types::Value::Text("is:fresh".to_owned()),
        )
    })
    .unwrap();
    assert!(matches!(
        f.c.run_saved_search(id, 0, 10),
        Err(SearchError::Syntax { .. })
    ));
}

// ---- Storage ----

#[test]
fn the_schema_check_passes_with_saved_searches() {
    let (c, _) = setup();
    check_schema(&c.conn, SYNCED_TABLES).unwrap();
    c.save_search("S", "", Sort::default(), Mode::Cards)
        .unwrap();
    assert!(
        c.conn.execute("DELETE FROM saved_search", []).is_err(),
        "never hard-deleted"
    );
    assert!(
        c.conn
            .execute("INSERT INTO saved_search (id) VALUES (x'00')", [])
            .is_err(),
        "written only through the write path"
    );
}

#[test]
fn a_version_8_collection_upgrades_and_keeps_its_cards() {
    let v8 = Schema {
        migrations: &MIGRATIONS[..8],
        tables: &SYNCED_TABLES[..SYNCED_TABLES.len() - 1],
    };
    let path = std::env::temp_dir().join(format!("fc-search-v8-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let location = path.to_str().unwrap();
    let clock = Arc::new(ManualClock::new(DAY0 + 9 * HOUR));
    let old = Collection::create_with(location, v8, host(&clock)).unwrap();
    let (_, card) = basic(&old, &clock, default_deck(), "kot", "cat");
    old.close().unwrap();

    let upgraded = Collection::open(location, host(&clock)).unwrap();
    assert_eq!(upgraded.info().unwrap().schema_version, 11);
    check_schema(&upgraded.conn, SYNCED_TABLES).unwrap();
    assert_eq!(
        upgraded
            .search("kot", &SearchOptions::default())
            .unwrap()
            .rows[0]
            .card,
        card
    );
    assert!(upgraded.saved_searches().unwrap().is_empty());
    upgraded
        .save_search("S", "kot", Sort::default(), Mode::Cards)
        .unwrap();
    upgraded.close().unwrap();
    let _ = std::fs::remove_file(&path);
}

// ---- Days in other time zones, and boolean logic against a model ----

#[test]
fn days_follow_the_time_zone_and_the_start_hour() {
    let f = fixture();
    // The clock reads about 09:11 UTC on day 0.
    assert_eq!(titles(&f.c, "due:2"), ["kot"]);
    assert_eq!(titles(&f.c, "rated:1"), ["kot", "pies"]);
    assert_eq!(titles(&f.c, "added:1").len(), 9);

    // At UTC+15:00 it is 00:11 on day 1 already.
    f.clock.set_utc_offset_minutes(15 * 60);
    assert_eq!(titles(&f.c, "due:1"), ["kot"]);
    assert_eq!(
        titles(&f.c, "due:0"),
        ["pies"],
        "its due time is in local day 1"
    );
    assert!(
        titles(&f.c, "rated:1").is_empty(),
        "the answers were given on day 0"
    );
    assert_eq!(titles(&f.c, "rated:2"), ["kot", "pies"]);
    // 09:00 UTC is local midnight there, so the notes were added at the start of that day.
    assert_eq!(titles(&f.c, "added:1").len(), 9);
    assert!(titles(&f.c, "added:-1..-1").is_empty());

    // With the day starting at 04:00, 00:11 still belongs to day 0.
    f.c.set_day_start_hour(4).unwrap();
    assert_eq!(titles(&f.c, "due:2"), ["kot"]);
    assert_eq!(titles(&f.c, "due:0"), ["pies"]);
    assert_eq!(titles(&f.c, "rated:1"), ["kot", "pies"]);
    assert_eq!(
        titles(&f.c, "added:1").len(),
        9,
        "00:11 is before 04:00, so it is still day 0"
    );
    assert!(titles(&f.c, "added:-1..-1").is_empty());
}

#[test]
fn a_moment_just_before_the_day_starts_is_the_day_before() {
    let (c, clock) = setup();
    c.set_day_start_hour(4).unwrap();
    clock.set(DAY0 + 3 * HOUR + 59 * MINUTE);
    basic(&c, &clock, default_deck(), "late", "x");
    clock.set(DAY0 + 4 * HOUR);
    basic(&c, &clock, default_deck(), "early", "x");
    assert_eq!(titles(&c, "added:1"), ["early"], "today began at 04:00");
    assert_eq!(titles(&c, "added:2"), ["early", "late"]);
    assert_eq!(titles(&c, "added:-1..-1"), ["late"], "yesterday");
}

/// A model of the boolean logic: every random combination of filters gives what set arithmetic on
/// the filters' own results gives, so `and`, `or`, `not` and brackets cannot disagree with the
/// filters they join.
#[test]
fn boolean_queries_agree_with_set_arithmetic() {
    use std::collections::BTreeSet;

    let f = fixture();
    let atoms = [
        "kot",
        "cat",
        "deck:Polish",
        "tag:lang",
        "is:new",
        "is:review",
        "note:Basic -note:reversed",
        "due:0..2",
        "rated:1",
        "front:p*",
        "-tag:*",
    ];
    let cards = |query: &str| -> BTreeSet<Id> {
        f.c.search(
            query,
            &SearchOptions {
                limit: MAX_LIMIT,
                ..Default::default()
            },
        )
        .unwrap()
        .rows
        .into_iter()
        .map(|r| r.card)
        .collect()
    };
    let everything = cards("");
    let sets: Vec<BTreeSet<Id>> = atoms.iter().map(|a| cards(a)).collect();

    // A small deterministic generator.
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = move |n: usize| -> usize {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state % n as u64) as usize
    };
    fn build(
        depth: usize,
        next: &mut impl FnMut(usize) -> usize,
        atoms: &[&str],
        sets: &[BTreeSet<Id>],
        everything: &BTreeSet<Id>,
    ) -> (String, BTreeSet<Id>) {
        if depth == 0 || next(4) == 0 {
            let i = next(atoms.len());
            return (atoms[i].to_owned(), sets[i].clone());
        }
        match next(4) {
            0 => {
                let (q, s) = build(depth - 1, next, atoms, sets, everything);
                (
                    format!("-({q})"),
                    everything.difference(&s).copied().collect(),
                )
            }
            1 => {
                let (q1, s1) = build(depth - 1, next, atoms, sets, everything);
                let (q2, s2) = build(depth - 1, next, atoms, sets, everything);
                (
                    format!("({q1}) or ({q2})"),
                    s1.union(&s2).copied().collect(),
                )
            }
            2 => {
                let (q1, s1) = build(depth - 1, next, atoms, sets, everything);
                let (q2, s2) = build(depth - 1, next, atoms, sets, everything);
                (
                    format!("({q1}) ({q2})"),
                    s1.intersection(&s2).copied().collect(),
                )
            }
            _ => {
                let (q1, s1) = build(depth - 1, next, atoms, sets, everything);
                let (q2, s2) = build(depth - 1, next, atoms, sets, everything);
                let (q3, s3) = build(depth - 1, next, atoms, sets, everything);
                // and binds tighter than or: a b or c
                let left: BTreeSet<Id> = s1.intersection(&s2).copied().collect();
                (
                    format!("({q1}) and ({q2}) or ({q3})"),
                    left.union(&s3).copied().collect(),
                )
            }
        }
    }
    for _ in 0..300 {
        let (query, expected) = build(3, &mut next, &atoms, &sets, &everything);
        assert_eq!(cards(&query), expected, "{query}");
    }
}
