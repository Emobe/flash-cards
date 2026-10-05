use std::sync::Arc;

use rusqlite::types::Value;

use super::*;
use crate::clock::{Host, ManualClock};
use crate::collection::Collection;
use crate::id::Id;
use crate::notetype::{Field, Kind, NoteType, NoteTypeError, TEMPLATE, Template, builtin};

fn collection() -> Collection {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let host = Host {
        clock,
        installation_id: Id::from_bytes(*b"installation-one"),
    };
    Collection::create(":memory:", host).unwrap()
}

fn field_ids(collection: &Collection, note_type: Id) -> Vec<Id> {
    let found = collection.note_type(note_type).unwrap().unwrap();
    found.fields.iter().map(|f| f.id).collect()
}

fn template_ids(collection: &Collection, note_type: Id) -> Vec<Id> {
    let found = collection.note_type(note_type).unwrap().unwrap();
    found.templates.iter().map(|t| t.id).collect()
}

/// Writes template text without the check that saving does, as a merge from another device could.
fn force_text(collection: &Collection, template: Id, front: &str, back: &str) {
    collection
        .write(|w| {
            w.set(
                TEMPLATE.entity,
                template,
                "front",
                Value::Text(front.into()),
            )?;
            w.set(TEMPLATE.entity, template, "back", Value::Text(back.into()))
        })
        .unwrap();
}

/// A note type and template made by hand, for rendering without a collection.
fn note_type(css: &str, front: &str, back: &str, fields: &[&str]) -> (NoteType, Template) {
    let template = Template {
        id: Id::from_bytes([2; 16]),
        name: "Card 1".into(),
        front: front.into(),
        back: back.into(),
    };
    let found = NoteType {
        id: Id::from_bytes([1; 16]),
        name: "Test".into(),
        kind: Kind::Standard,
        css: css.into(),
        sort_field: None,
        fields: fields
            .iter()
            .enumerate()
            .map(|(i, name)| Field {
                id: Id::from_bytes([10 + i as u8; 16]),
                name: (*name).into(),
            })
            .collect(),
        templates: vec![template.clone()],
        deleted: false,
    };
    (found, template)
}

fn render_with(
    css: &str,
    front: &str,
    back: &str,
    values: &[(&str, &str)],
    ordinal: u32,
) -> Result<RenderedCard, TemplateProblem> {
    let names: Vec<&str> = values.iter().map(|(n, _)| *n).collect();
    let (found, template) = note_type(css, front, back, &names);
    let value = |name: &str| values.iter().find(|(n, _)| *n == name).map(|(_, v)| *v);
    render(&found, &template, &value, ordinal)
}

/// The text between `<body class="card">` and `</body>`.
fn body(document: &str) -> &str {
    let start = document.find("<body class=\"card\">").unwrap() + "<body class=\"card\">".len();
    &document[start..document.rfind("</body>").unwrap()]
}

// Substitution, sections and FrontSide.

#[test]
fn a_field_is_inserted_as_written() {
    let card = render_with(
        "",
        "{{Front}}",
        "{{Back}}",
        &[("Front", "<b>pies</b>"), ("Back", "dog")],
        0,
    )
    .unwrap();
    assert_eq!(body(&card.front), "<b>pies</b>");
    assert_eq!(body(&card.back), "dog");
}

#[test]
fn a_field_the_note_type_lacks_reads_as_empty() {
    let card = render_with(
        "",
        "a{{Nope}}b",
        "{{#Nope}}x{{/Nope}}{{^Nope}}y{{/Nope}}",
        &[("Front", "f")],
        0,
    )
    .unwrap();
    assert_eq!(body(&card.front), "ab");
    assert_eq!(body(&card.back), "y");
}

#[test]
fn sections_show_by_whether_the_field_has_content() {
    let template = "{{#Extra}}[{{Extra}}]{{/Extra}}{{^Extra}}none{{/Extra}}";
    for (extra, expected) in [
        ("hi", "[hi]"),
        ("", "none"),
        ("<br>", "none"),
        ("&nbsp;", "none"),
    ] {
        let card = render_with("", template, "", &[("Extra", extra)], 0).unwrap();
        assert_eq!(body(&card.front), expected, "{extra:?}");
    }
    let card = render_with(
        "",
        "{{#A}}a{{#B}}b{{/B}}{{/A}}",
        "",
        &[("A", "1"), ("B", "")],
        0,
    )
    .unwrap();
    assert_eq!(body(&card.front), "a");
}

#[test]
fn the_back_includes_the_front() {
    let card = render_with(
        "",
        "Q: {{Front}}",
        "{{FrontSide}}<hr id=answer>{{Back}}",
        &[("Front", "pies"), ("Back", "dog")],
        0,
    )
    .unwrap();
    assert_eq!(body(&card.back), "Q: pies<hr id=answer>dog");
}

#[test]
fn filters_apply_from_the_field_outwards() {
    let values = [("Word", "<b>pies</b> & co")];
    let text = render_with("", "{{text:Word}}", "", &values, 0).unwrap();
    assert_eq!(body(&text.front), "pies & co");
    // `hint:text:Word` makes the text first, then the hint block.
    let both = render_with("", "{{hint:text:Word}}", "", &values, 0).unwrap();
    assert_eq!(
        body(&both.front),
        "<details class=\"hint\"><summary>Word</summary>pies & co</details>"
    );
    let empty = render_with("", "{{hint:Word}}", "", &[("Word", "<br>")], 0).unwrap();
    assert_eq!(body(&empty.front), "");
}

#[test]
fn a_hint_label_is_escaped() {
    let card = render_with("", "{{hint:A\"<b}}", "", &[("A\"<b", "x")], 0).unwrap();
    assert_eq!(
        body(&card.front),
        "<details class=\"hint\"><summary>A&quot;&lt;b</summary>x</details>"
    );
}

#[test]
fn spaces_in_field_names_work() {
    let card = render_with(
        "",
        "{{ Add reverse }}|{{#Add reverse}}y{{/Add reverse}}",
        "",
        &[("Add reverse", "1")],
        0,
    )
    .unwrap();
    assert_eq!(body(&card.front), "1|y");
}

// Cloze.

#[test]
fn cloze_cards_render_front_and_back() {
    let values = [
        ("Text", "{{c1::Warszawa}} is in {{c2::Polska::country}}"),
        ("Extra", "capital"),
    ];
    let first = render_with(
        "",
        "{{cloze:Text}}",
        "{{cloze:Text}}<br>{{Extra}}",
        &values,
        1,
    )
    .unwrap();
    assert_eq!(
        body(&first.front),
        "<span class=\"cloze\">[...]</span> is in Polska"
    );
    assert_eq!(
        body(&first.back),
        "<span class=\"cloze\">Warszawa</span> is in Polska<br>capital"
    );
    let second = render_with("", "{{cloze:Text}}", "{{cloze:Text}}", &values, 2).unwrap();
    assert_eq!(
        body(&second.front),
        "Warszawa is in <span class=\"cloze\">[country]</span>"
    );
}

#[test]
fn a_field_without_the_cloze_filter_keeps_its_markers() {
    let card = render_with("", "{{Text}}", "", &[("Text", "{{c1::a}}")], 1).unwrap();
    assert_eq!(body(&card.front), "{{c1::a}}");
}

// Media.

#[test]
fn media_is_listed_from_both_sides_without_repeats() {
    let card = render_with(
        "",
        "{{Front}}",
        "{{FrontSide}}{{Back}}[sound:pies.mp3]",
        &[
            ("Front", "<img src=\"pies.png\">"),
            ("Back", "<img src=\"pies.png\"><img src=\"https://example.com/x.png\"><video src=\"v.mp4\"></video>"),
        ],
        0,
    )
    .unwrap();
    assert_eq!(card.media, ["pies.png", "v.mp4", "pies.mp3"]);
    assert!(
        card.back
            .contains("<audio controls src=\"pies.mp3\"></audio>")
    );
}

#[test]
fn a_sound_in_the_template_itself_counts() {
    let card = render_with("", "[sound:a.mp3]{{Front}}", "", &[("Front", "x")], 0).unwrap();
    assert_eq!(card.media, ["a.mp3"]);
}

// The document.

#[test]
fn a_side_is_a_complete_document_with_the_css() {
    let card = render_with(
        ".card { color: red; }",
        "{{Front}}",
        "",
        &[("Front", "x")],
        0,
    )
    .unwrap();
    assert!(
        card.front
            .starts_with("<!doctype html><html><head><meta charset=\"utf-8\">")
    );
    assert!(card.front.contains("<style>.card { color: red; }</style>"));
    assert!(card.front.ends_with("</body></html>"));
}

#[test]
fn css_cannot_close_its_own_style_element() {
    for css in ["</style><script>x</script>", "a</STYLE >b", "</sty"] {
        let card = render_with(css, "x", "", &[], 0).unwrap();
        let head = &card.front[..card.front.find("<body").unwrap()];
        assert_eq!(head.matches("</style").count(), 1, "{css}: {head}");
        assert!(head.ends_with("</style></head>"), "{css}: {head}");
    }
}

// Safety.

#[test]
fn a_value_is_never_read_as_template_syntax() {
    let values = [
        (
            "Front",
            "{{Back}} {{#Back}}x{{/Back}} {{FrontSide}} {{cloze:Back}}",
        ),
        ("Back", "secret"),
    ];
    let card = render_with("", "{{Front}}", "{{FrontSide}}|{{Front}}", &values, 0).unwrap();
    assert_eq!(body(&card.front), values[0].1);
    assert_eq!(body(&card.back), format!("{0}|{0}", values[0].1));
    assert!(!body(&card.front).contains("secret"));
}

#[test]
fn rendering_adds_no_active_content_of_its_own() {
    let card = render_with(
        "",
        "{{Front}} {{hint:Front}} {{text:Front}} {{cloze:Front}}",
        "{{FrontSide}}[sound:a.mp3]",
        &[("Front", "plain")],
        1,
    )
    .unwrap();
    for side in [&card.front, &card.back] {
        let lower = side.to_ascii_lowercase();
        for word in [
            "<script",
            "onerror",
            "onload",
            "javascript:",
            "http:",
            "https:",
            "<iframe",
            "<link",
            "<meta http-equiv",
        ] {
            assert!(!lower.contains(word), "{word} in {side}");
        }
    }
}

#[test]
fn text_filter_output_is_safe_to_put_in_html() {
    let card = render_with(
        "",
        "{{text:A}}",
        "",
        &[("A", "<script>alert(1)</script>1 < 2 <b")],
        0,
    )
    .unwrap();
    assert!(!body(&card.front).contains("<script"));
    assert!(!body(&card.front).contains('<'));
}

// Mistakes.

#[test]
fn a_mistake_is_an_error_that_says_where_and_what_to_do() {
    let problem = render_with(
        "",
        "Q\n{{#Extra}}{{Front}}",
        "{{Back}}",
        &[("Front", "x")],
        0,
    )
    .unwrap_err();
    assert_eq!(problem.side, Side::Front);
    assert_eq!(problem.template, "Card 1");
    let message = problem.to_string();
    assert_eq!(
        message,
        "The front of the card template \"Card 1\" has a mistake: line 2, column 1: the section \
         {{#Extra}} is never closed. Add {{/Extra}} where it ends."
    );
    let back = render_with("", "{{Front}}", "{{/X}}", &[("Front", "x")], 0).unwrap_err();
    assert_eq!(back.side, Side::Back);
}

#[test]
fn every_mistake_has_a_message_that_names_it() {
    for (text, side, expected) in [
        ("a {{b", Side::Front, "has no matching"),
        ("{{}}", Side::Front, "no field name"),
        ("{{#A}}", Side::Front, "{{#A}} is never closed"),
        ("{{^A}}", Side::Front, "{{^A}} is never closed"),
        (
            "{{/A}}",
            Side::Front,
            "{{/A}} closes a section that was never opened",
        ),
        ("{{foo:A}}", Side::Front, "\"foo:\" is not a filter"),
        ("{{FrontSide}}", Side::Front, "for the back of a card"),
    ] {
        let errors = check(text, side);
        assert_eq!(errors.len(), 1, "{text}");
        assert!(
            errors[0].to_string().contains(expected),
            "{text}: {}",
            errors[0]
        );
    }
    assert!(check("{{FrontSide}}", Side::Back).is_empty());
}

#[test]
fn unknown_fields_are_listed_not_refused() {
    let known = ["Front", "Back"];
    assert_eq!(
        unknown_fields(
            "{{Front}}{{#Extra}}{{cloze:Text}}{{/Extra}}{{FrontSide}}",
            &known
        ),
        ["Extra", "Text"]
    );
    assert!(unknown_fields("{{Front}}{{{{", &known).is_empty());
}

#[test]
fn renaming_changes_only_the_tags_that_name_the_field() {
    let text = "{{Front}} Front {{ hint:Front }} {{#Front}}x{{/Front}} {{Back}} {{FrontSide}}";
    assert_eq!(
        rename_field(text, "Front", "Question"),
        "{{Question}} Front {{ hint:Question }} {{#Question}}x{{/Question}} {{Back}} {{FrontSide}}"
    );
    assert_eq!(rename_field("{{Front", "Front", "Q"), "{{Front");
    assert_eq!(
        rename_field("{{#Front}}{{foo:Front}}", "Front", "Q"),
        "{{#Q}}{{foo:Q}}"
    );
    assert_eq!(rename_field("{{Add reverse}}", "Add reverse", "R"), "{{R}}");
}

#[test]
fn nothing_panics_on_garbage() {
    let pieces = [
        "{{",
        "}}",
        "{",
        "}",
        "#",
        "^",
        "/",
        ":",
        "::",
        "cloze",
        "text",
        "hint",
        "Front",
        "Back",
        "FrontSide",
        " ",
        "\n",
        "<b>",
        "</b>",
        "<",
        ">",
        "[sound:",
        "]",
        "c1::",
        "{{c1::",
        "é",
        "ż",
        "\"",
        "=",
        "src=",
    ];
    let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for _ in 0..3000 {
        let mut text = String::new();
        for _ in 0..(next() % 14) {
            text.push_str(pieces[(next() % pieces.len() as u64) as usize]);
        }
        let mut other = String::new();
        for _ in 0..(next() % 14) {
            other.push_str(pieces[(next() % pieces.len() as u64) as usize]);
        }
        let _ = check(&text, Side::Front);
        let _ = check(&text, Side::Back);
        let _ = unknown_fields(&text, &["Front"]);
        let _ = rename_field(&text, "Front", "Q");
        let front = parse_front(&text);
        let values = [("Front", other.as_str()), ("Back", text.as_str())];
        let lookup = |name: &str| values.iter().find(|(n, _)| *n == name).map(|(_, v)| *v);
        let _ = front_shows_content(&front, &lookup);
        let _ = cloze_numbers(&front, &lookup);
        let _ = render_with("", &text, &other, &values, (next() % 4) as u32);
        let _ = render_with(&text, "{{cloze:Front}}", "{{cloze:Back}}", &values, 1);
    }
}

// Through a collection.

#[test]
fn the_built_in_types_render() {
    let collection = collection();
    let fields = field_ids(&collection, builtin::basic_and_reversed());
    let note = collection
        .add_note(
            builtin::basic_and_reversed(),
            &[(fields[0], "pies"), (fields[1], "dog")],
        )
        .unwrap();
    assert_eq!(note.cards.len(), 2);
    let first = collection.render_card(note.cards[0]).unwrap();
    assert_eq!(body(&first.front), "pies");
    assert_eq!(body(&first.back), "pies\n\n<hr id=answer>\n\ndog");
    assert!(first.front.contains(".card {"));
    let second = collection.render_card(note.cards[1]).unwrap();
    assert_eq!(body(&second.front), "dog");
    assert_eq!(body(&second.back), "dog\n\n<hr id=answer>\n\npies");
    assert!(first.media.is_empty());
}

#[test]
fn a_cloze_note_renders_one_card_per_number() {
    let collection = collection();
    let fields = field_ids(&collection, builtin::cloze());
    let note = collection
        .add_note(
            builtin::cloze(),
            &[
                (fields[0], "{{c1::Warszawa}} is in {{c2::Polska}}"),
                (fields[1], "Europe"),
            ],
        )
        .unwrap();
    let cards = collection.cards_of_note(note.id).unwrap();
    assert_eq!(cards.iter().map(|c| c.ordinal).collect::<Vec<_>>(), [1, 2]);
    let first = collection.render_card(cards[0].id).unwrap();
    assert_eq!(
        body(&first.front),
        "<span class=\"cloze\">[...]</span> is in Polska"
    );
    assert_eq!(
        body(&first.back),
        "<span class=\"cloze\">Warszawa</span> is in Polska\n\n<br>\n\nEurope"
    );
    let second = collection.render_card(cards[1].id).unwrap();
    assert_eq!(
        body(&second.front),
        "Warszawa is in <span class=\"cloze\">[...]</span>"
    );
}

#[test]
fn a_note_type_css_reaches_the_card() {
    let collection = collection();
    collection
        .set_note_type_css(builtin::basic(), ".card { color: blue; }")
        .unwrap();
    let fields = field_ids(&collection, builtin::basic());
    let note = collection
        .add_note(builtin::basic(), &[(fields[0], "a"), (fields[1], "b")])
        .unwrap();
    let card = collection.render_card(note.cards[0]).unwrap();
    assert!(card.front.contains("<style>.card { color: blue; }</style>"));
}

#[test]
fn a_card_that_is_gone_is_not_found() {
    let collection = collection();
    let fields = field_ids(&collection, builtin::basic());
    let note = collection
        .add_note(builtin::basic(), &[(fields[0], "a"), (fields[1], "b")])
        .unwrap();
    let card = note.cards[0];
    collection.delete_note(note.id).unwrap();
    assert_eq!(collection.render_card(card), Err(RenderError::NotFound));
    collection.restore_note(note.id).unwrap();
    assert!(collection.render_card(card).is_ok());
    assert_eq!(
        collection.render_card(Id::from_bytes([9; 16])),
        Err(RenderError::NotFound)
    );
    let template = template_ids(&collection, builtin::basic())[0];
    collection
        .add_template(builtin::basic(), "Two", "{{Back}}", "{{Front}}")
        .unwrap();
    collection
        .remove_template(builtin::basic(), template)
        .unwrap();
    assert_eq!(collection.render_card(card), Err(RenderError::NotFound));
}

#[test]
fn a_template_broken_by_a_merge_gives_a_clear_error_but_keeps_its_cards() {
    let collection = collection();
    let fields = field_ids(&collection, builtin::basic());
    let note = collection
        .add_note(builtin::basic(), &[(fields[0], "pies"), (fields[1], "dog")])
        .unwrap();
    let template = template_ids(&collection, builtin::basic())[0];
    force_text(
        &collection,
        template,
        "{{#Front}}{{Front}}",
        "{{FrontSide}}{{Back}}",
    );
    let error = collection.render_card(note.cards[0]).unwrap_err();
    assert!(matches!(&error, RenderError::Template(p) if p.side == Side::Front));
    assert!(
        error
            .to_string()
            .ends_with("Fix the template to see this card.")
    );
    // Editing the note reads the front leniently, so the card stays.
    let change = collection
        .set_note_fields(note.id, &[(fields[1], "psa")])
        .unwrap();
    assert!(change.removed_cards.is_empty());
    assert_eq!(collection.cards_of_note(note.id).unwrap().len(), 1);
    force_text(&collection, template, "{{Front}}{{/Gone}}", "{{Back}}");
    collection
        .set_note_fields(note.id, &[(fields[0], "kot")])
        .unwrap();
    assert_eq!(collection.cards_of_note(note.id).unwrap().len(), 1);
}

#[test]
fn saving_a_template_with_a_mistake_is_refused_and_changes_nothing() {
    let collection = collection();
    let template = template_ids(&collection, builtin::basic())[0];
    let before = collection.note_type(builtin::basic()).unwrap().unwrap();
    let error = collection
        .set_template_text(
            builtin::basic(),
            template,
            "{{Front}}",
            "{{FrontSide}}{{#Back}}",
        )
        .unwrap_err();
    let NoteTypeError::Template(problem) = &error else {
        panic!("{error:?}");
    };
    assert_eq!(
        (problem.side, problem.template.as_str()),
        (Side::Back, "Card 1")
    );
    assert!(error.to_string().ends_with("Nothing was saved."));
    assert_eq!(
        collection.note_type(builtin::basic()).unwrap().unwrap(),
        before
    );

    let error = collection
        .add_template(builtin::basic(), "Two", "{{FrontSide}}", "{{Back}}")
        .unwrap_err();
    assert!(
        error.to_string().contains("for the back of a card"),
        "{error}"
    );
    assert_eq!(
        collection.note_type(builtin::basic()).unwrap().unwrap(),
        before
    );
}

#[test]
fn saving_a_template_that_names_a_missing_field_works() {
    let collection = collection();
    let template = template_ids(&collection, builtin::basic())[0];
    collection
        .set_template_text(
            builtin::basic(),
            template,
            "{{Front}}{{Missing}}",
            "{{Back}}",
        )
        .unwrap();
    let fields = field_ids(&collection, builtin::basic());
    let note = collection
        .add_note(builtin::basic(), &[(fields[0], "a"), (fields[1], "b")])
        .unwrap();
    let card = collection.render_card(note.cards[0]).unwrap();
    assert_eq!(body(&card.front), "a");
}

#[test]
fn renaming_a_field_keeps_the_card_rendering() {
    let collection = collection();
    let fields = field_ids(&collection, builtin::basic());
    let note = collection
        .add_note(builtin::basic(), &[(fields[0], "pies"), (fields[1], "dog")])
        .unwrap();
    collection
        .rename_field(builtin::basic(), fields[0], "Polish")
        .unwrap();
    let card = collection.render_card(note.cards[0]).unwrap();
    assert_eq!(body(&card.front), "pies");
}
