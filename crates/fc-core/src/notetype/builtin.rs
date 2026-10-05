//! The built-in note types: Basic, Basic and reversed, and Cloze (`PRODUCT.md`).
//!
//! Their IDs are fixed (UUIDv5 of a name), so two collections created separately share them and
//! merge into one set instead of two (ADR 0006, section 1). They are ordinary note types once they
//! exist: they can be renamed, edited and deleted.
//!
//! They are written by migration 3, with the lowest possible clock (see `sync::seed_row`). **The
//! content below is frozen once a version has shipped it**: a device that later changed it would
//! differ silently from the others, because every device writes the same clock. A change to a
//! built-in is a new migration that writes with real clocks, and only where the user has not edited
//! the row since.
//!
//! The template text uses the syntax step 1.4 will define: `{{Field}}` substitutes a field,
//! `{{FrontSide}}` is the front of the card on the back, and `{{cloze:Field}}` renders a cloze.

use rusqlite::Transaction;
use rusqlite::types::Value;

use super::{FIELD, Kind, NOTE_TYPE, TEMPLATE};
use crate::id::Id;
use crate::sync::seed_row;

/// "fc-builtin-ids-1": the namespace of every built-in ID.
pub(crate) const NAMESPACE: Id = Id::from_bytes(*b"fc-builtin-ids-1");

/// Positions of the first three fields or templates. Fixed text, never recomputed: see above.
pub(crate) const POSITIONS: [&str; 3] = ["V", "l", "t"];

pub(crate) struct TemplateDef {
    pub name: &'static str,
    pub front: &'static str,
    pub back: &'static str,
}

pub(crate) struct Def {
    key: &'static str,
    pub name: &'static str,
    pub kind: Kind,
    pub fields: &'static [&'static str],
    pub templates: &'static [TemplateDef],
}

pub(crate) const CSS: &str = ".card {\n  font-family: sans-serif;\n  font-size: 1.25rem;\n  text-align: center;\n}\n.cloze {\n  font-weight: bold;\n}\n";

pub(crate) const BASIC: Def = Def {
    key: "basic",
    name: "Basic",
    kind: Kind::Standard,
    fields: &["Front", "Back"],
    templates: &[TemplateDef {
        name: "Card 1",
        front: "{{Front}}",
        back: "{{FrontSide}}\n\n<hr id=answer>\n\n{{Back}}",
    }],
};

pub(crate) const BASIC_AND_REVERSED: Def = Def {
    key: "basic-and-reversed",
    name: "Basic and reversed",
    kind: Kind::Standard,
    fields: &["Front", "Back"],
    templates: &[
        TemplateDef {
            name: "Card 1",
            front: "{{Front}}",
            back: "{{FrontSide}}\n\n<hr id=answer>\n\n{{Back}}",
        },
        TemplateDef {
            name: "Card 2",
            front: "{{Back}}",
            back: "{{FrontSide}}\n\n<hr id=answer>\n\n{{Front}}",
        },
    ],
};

pub(crate) const CLOZE: Def = Def {
    key: "cloze",
    name: "Cloze",
    kind: Kind::Cloze,
    fields: &["Text", "Extra"],
    templates: &[TemplateDef {
        name: "Cloze",
        front: "{{cloze:Text}}",
        back: "{{cloze:Text}}\n\n<br>\n\n{{Extra}}",
    }],
};

const BUILT_INS: [&Def; 3] = [&BASIC, &BASIC_AND_REVERSED, &CLOZE];

fn id_of(key: &str, part: &str) -> Id {
    Id::new_v5(NAMESPACE, format!("{key}/{part}").as_bytes())
}

/// The ID of the built-in Basic note type.
pub fn basic() -> Id {
    id_of(BASIC.key, "note_type")
}

/// The ID of the built-in Basic and reversed note type.
pub fn basic_and_reversed() -> Id {
    id_of(BASIC_AND_REVERSED.key, "note_type")
}

/// The ID of the built-in Cloze note type.
pub fn cloze() -> Id {
    id_of(CLOZE.key, "note_type")
}

fn text(value: &str) -> Value {
    Value::Text(value.to_owned())
}

/// Writes the built-ins. Called by migration 3 only.
pub(crate) fn seed(tx: &Transaction) -> rusqlite::Result<()> {
    for def in BUILT_INS {
        let note_type = id_of(def.key, "note_type");
        let first_field = id_of(def.key, "field/0");
        seed_row(
            tx,
            &NOTE_TYPE,
            note_type,
            &[
                ("name", text(def.name)),
                ("kind", text(def.kind.as_text())),
                ("css", text(CSS)),
                ("sort_field", Value::Blob(first_field.as_bytes().to_vec())),
                ("deleted", Value::Integer(0)),
            ],
        )?;
        for (index, name) in def.fields.iter().enumerate() {
            seed_row(
                tx,
                &FIELD,
                id_of(def.key, &format!("field/{index}")),
                &[
                    ("note_type", Value::Blob(note_type.as_bytes().to_vec())),
                    ("name", text(name)),
                    ("position", text(POSITIONS[index])),
                    ("deleted", Value::Integer(0)),
                ],
            )?;
        }
        for (index, template) in def.templates.iter().enumerate() {
            seed_row(
                tx,
                &TEMPLATE,
                id_of(def.key, &format!("template/{index}")),
                &[
                    ("note_type", Value::Blob(note_type.as_bytes().to_vec())),
                    ("name", text(template.name)),
                    ("position", text(POSITIONS[index])),
                    ("front", text(template.front)),
                    ("back", text(template.back)),
                    ("deleted", Value::Integer(0)),
                ],
            )?;
        }
    }
    Ok(())
}
