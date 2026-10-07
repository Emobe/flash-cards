//! The Add screen's methods (step 2.4a): note types, tags, the duplicate check, adding a note and
//! adding a media file. Thin wrappers over `fc_core::note`, `notetype`, `tag` and `media`.

use fc_core::Core;
use fc_core::media::MediaError;
use fc_core::note::{NewNote, NoteError};
use fc_core::notetype::Kind;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::collection::not_open;
use crate::study::{internal, parse_id};
use crate::{ApiError, ErrorKind, Method, OpContext};

fn note_error(what: &str, error: NoteError) -> ApiError {
    match error {
        NoteError::NotFound => ApiError::new(
            ErrorKind::NotFound,
            "The deck or note type was deleted. Choose another and try again.",
        ),
        NoteError::NoCards { cloze: true } => ApiError::invalid_input(
            "This note would make no cards. Add a cloze with the Cloze button.",
        ),
        NoteError::NoCards { cloze: false } => {
            ApiError::invalid_input("This note would make no cards. Fill in the front.")
        }
        NoteError::UnknownField | NoteError::Tag(_) => ApiError::invalid_input(error.to_string()),
        NoteError::Collection(inner) => match ApiError::from(inner) {
            api if api == ApiError::internal() => internal(what, &api),
            api => api,
        },
    }
}

// ---- Note types and tags --------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum NoteTypeKind {
    Standard,
    Cloze,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct FieldSummary {
    /// A UUID string.
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct NoteTypeSummary {
    /// A UUID string.
    pub id: String,
    pub name: String,
    pub kind: NoteTypeKind,
    /// In the order to show them.
    pub fields: Vec<FieldSummary>,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct NoteTypeList {
    /// Every note type that is not deleted, by name.
    pub note_types: Vec<NoteTypeSummary>,
}

pub struct GetNoteTypes;

impl Method for GetNoteTypes {
    const NAME: &'static str = "getNoteTypes";
    type Input = ();
    type Output = NoteTypeList;

    fn call(core: &Core, (): (), _: &OpContext) -> Result<NoteTypeList, ApiError> {
        let found = core
            .with_collection(|c| c.note_types())
            .ok_or_else(|| not_open(core))?
            .map_err(|error| internal("read the note types", error))?;
        let note_types = found
            .into_iter()
            .map(|t| NoteTypeSummary {
                id: t.id.to_string(),
                name: t.name,
                kind: match t.kind {
                    Kind::Standard => NoteTypeKind::Standard,
                    Kind::Cloze => NoteTypeKind::Cloze,
                },
                fields: t
                    .fields
                    .into_iter()
                    .map(|f| FieldSummary {
                        id: f.id.to_string(),
                        name: f.name,
                    })
                    .collect(),
            })
            .collect();
        Ok(NoteTypeList { note_types })
    }
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct TagList {
    /// Every tag in use, with `::` between the parts of a nested one, for suggestions.
    pub tags: Vec<String>,
}

pub struct GetTags;

impl Method for GetTags {
    const NAME: &'static str = "getTags";
    type Input = ();
    type Output = TagList;

    fn call(core: &Core, (): (), _: &OpContext) -> Result<TagList, ApiError> {
        let tags = core
            .with_collection(|c| c.tags())
            .ok_or_else(|| not_open(core))?
            .map_err(|error| internal("read the tags", error))?;
        Ok(TagList {
            tags: tags.into_iter().map(|t| t.name).collect(),
        })
    }
}

// ---- Adding a note --------------------------------------------------------------------------------

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct FindDuplicatesInput {
    pub note_type_id: String,
    /// The text of the first field.
    pub value: String,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct FindDuplicatesOutput {
    /// Notes of this note type with the same first field. Empty when there are none.
    pub note_ids: Vec<String>,
}

pub struct FindDuplicates;

impl Method for FindDuplicates {
    const NAME: &'static str = "findDuplicates";
    type Input = FindDuplicatesInput;
    type Output = FindDuplicatesOutput;

    fn call(core: &Core, input: Self::Input, _: &OpContext) -> Result<Self::Output, ApiError> {
        let note_type = parse_id(&input.note_type_id)?;
        let found = core
            .with_collection(|c| c.find_duplicates(note_type, &input.value))
            .ok_or_else(|| not_open(core))?
            .map_err(|error| note_error("look for duplicates", error))?;
        Ok(FindDuplicatesOutput {
            note_ids: found.into_iter().map(|id| id.to_string()).collect(),
        })
    }
}

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct FieldValue {
    pub field_id: String,
    /// The field's HTML. A field left out is empty.
    pub value: String,
}

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AddNoteInput {
    pub deck_id: String,
    pub note_type_id: String,
    pub fields: Vec<FieldValue>,
    /// A tag with a space or an empty part is refused, and nothing is added.
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AddNoteOutput {
    pub note_id: String,
    pub card_count: u32,
    /// Other notes of this note type with the same first field. A warning, never a block.
    pub duplicates: Vec<String>,
}

/// Adds a note with its cards and tags in one write.
pub struct AddNote;

impl Method for AddNote {
    const NAME: &'static str = "addNote";
    type Input = AddNoteInput;
    type Output = AddNoteOutput;

    fn call(core: &Core, input: Self::Input, _: &OpContext) -> Result<Self::Output, ApiError> {
        let deck = parse_id(&input.deck_id)?;
        let note_type = parse_id(&input.note_type_id)?;
        let values = input
            .fields
            .iter()
            .map(|f| Ok((parse_id(&f.field_id)?, f.value.as_str())))
            .collect::<Result<Vec<_>, ApiError>>()?;
        let tags: Vec<&str> = input.tags.iter().map(String::as_str).collect();
        let added = core
            .with_collection(|c| {
                c.add_note_with(&NewNote {
                    deck,
                    note_type,
                    values: &values,
                    tags: &tags,
                })
            })
            .ok_or_else(|| not_open(core))?
            .map_err(|error| note_error("add a note", error))?;
        Ok(AddNoteOutput {
            note_id: added.id.to_string(),
            card_count: u32::try_from(added.cards.len()).unwrap_or(u32::MAX),
            duplicates: added.duplicates.iter().map(ToString::to_string).collect(),
        })
    }
}

// ---- Adding media ---------------------------------------------------------------------------------

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AddMediaInput {
    /// The file's own name. Only its stem and extension are used.
    pub name: String,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AddMediaOutput {
    /// The name to use in a field (`<img src="...">` or `[sound:...]`).
    pub name: String,
    /// False when the collection already had these exact bytes.
    pub new: bool,
}

/// Stores a file, sent as the request attachment. The same bytes are stored once.
pub struct AddMedia;

impl Method for AddMedia {
    const NAME: &'static str = "addMedia";
    const ATTACHMENT_IN: bool = true;
    type Input = AddMediaInput;
    type Output = AddMediaOutput;

    fn call(core: &Core, input: Self::Input, ctx: &OpContext) -> Result<Self::Output, ApiError> {
        let bytes = ctx.attachment().filter(|b| !b.is_empty()).ok_or_else(|| {
            ApiError::invalid_input("The file is empty. Pick a different file and try again.")
        })?;
        let added = core
            .with_collection(|c| c.add_media(&input.name, bytes))
            .ok_or_else(|| not_open(core))?
            .map_err(|error| match error {
                MediaError::Collection(inner) => match ApiError::from(inner) {
                    api if api == ApiError::internal() => internal("add a media file", &api),
                    api => api,
                },
                other => ApiError::invalid_input(other.to_string()),
            })?;
        Ok(AddMediaOutput {
            name: added.name,
            new: added.new,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use fc_core::clock::ManualClock;
    use fc_core::notetype::builtin;
    use serde_json::{Value, json};

    use crate::{Host, Id, dispatch};

    use super::*;

    fn core() -> Core {
        let core = Core::new();
        let host = Host {
            clock: Arc::new(ManualClock::new(1_791_363_600_000)),
            installation_id: Id::from_bytes([1; 16]),
        };
        core.open_collection(":memory:", host).unwrap();
        core
    }

    fn call(core: &Core, method: &str, input: Value) -> Result<Value, ApiError> {
        dispatch(core, method, input, &OpContext::uncancellable()).map(|r| r.output)
    }

    fn with_bytes(
        core: &Core,
        method: &str,
        input: Value,
        bytes: &[u8],
    ) -> Result<Value, ApiError> {
        let ctx = OpContext::uncancellable().with_attachment(bytes);
        dispatch(core, method, input, &ctx).map(|r| r.output)
    }

    /// The Default deck's ID, the Basic note type's field IDs and the Cloze field ID.
    struct Ids {
        deck: String,
        basic: String,
        front: String,
        back: String,
        cloze: String,
        text: String,
    }

    fn ids(core: &Core) -> Ids {
        let decks = call(core, "getDeckList", Value::Null).unwrap();
        let types = call(core, "getNoteTypes", Value::Null).unwrap();
        let find = |name: &str| {
            types["noteTypes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["name"] == name)
                .unwrap()
                .clone()
        };
        let (basic, cloze) = (find("Basic"), find("Cloze"));
        let text = |v: &Value, at: usize| v["fields"][at]["id"].as_str().unwrap().to_owned();
        Ids {
            deck: decks["decks"][0]["id"].as_str().unwrap().to_owned(),
            basic: basic["id"].as_str().unwrap().to_owned(),
            front: text(&basic, 0),
            back: text(&basic, 1),
            cloze: cloze["id"].as_str().unwrap().to_owned(),
            text: text(&cloze, 0),
        }
    }

    fn add_input(ids: &Ids, front: &str, tags: &[&str]) -> Value {
        json!({
            "deckId": ids.deck,
            "noteTypeId": ids.basic,
            "fields": [
                { "fieldId": ids.front, "value": front },
                { "fieldId": ids.back, "value": "back" },
            ],
            "tags": tags,
        })
    }

    #[test]
    fn note_types_come_with_their_kind_and_fields_in_order() {
        let core = core();
        let list = call(&core, "getNoteTypes", Value::Null).unwrap();
        let types = list["noteTypes"].as_array().unwrap();
        let by_name = |n: &str| types.iter().find(|t| t["name"] == n).unwrap();
        assert_eq!(by_name("Basic")["kind"], "standard");
        assert_eq!(by_name("Cloze")["kind"], "cloze");
        let names: Vec<_> = by_name("Basic")["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["Front", "Back"]);
        assert_eq!(by_name("Basic")["id"], json!(builtin::basic().to_string()));
    }

    #[test]
    fn a_new_collection_has_no_tags() {
        let out = call(&core(), "getTags", Value::Null).unwrap();
        assert_eq!(out, json!({ "tags": [] }));
    }

    #[test]
    fn adding_a_note_makes_its_cards_and_its_tags_show_in_get_tags() {
        let core = core();
        let ids = ids(&core);
        let out = call(
            &core,
            "addNote",
            add_input(&ids, "kot", &["animals", "pl::basic"]),
        )
        .unwrap();
        assert_eq!(out["cardCount"], 1);
        assert_eq!(out["duplicates"], json!([]));
        assert!(out["noteId"].is_string());
        let tags = call(&core, "getTags", Value::Null).unwrap();
        assert_eq!(tags["tags"], json!(["animals", "pl", "pl::basic"]));
        let decks = call(&core, "getDeckList", Value::Null).unwrap();
        assert_eq!(decks["totalCards"], 1);
    }

    #[test]
    fn a_cloze_note_makes_one_card_per_number() {
        let core = core();
        let ids = ids(&core);
        let out = call(
            &core,
            "addNote",
            json!({
                "deckId": ids.deck,
                "noteTypeId": ids.cloze,
                "fields": [{ "fieldId": ids.text, "value": "{{c1::Warszawa}} is {{c2::big}}" }],
                "tags": [],
            }),
        )
        .unwrap();
        assert_eq!(out["cardCount"], 2);
    }

    #[test]
    fn duplicates_are_reported_by_find_duplicates_and_by_add_note() {
        let core = core();
        let ids = ids(&core);
        let first = call(&core, "addNote", add_input(&ids, "kot", &[])).unwrap();
        let found = call(
            &core,
            "findDuplicates",
            json!({ "noteTypeId": ids.basic, "value": "  KOT " }),
        )
        .unwrap();
        assert_eq!(found["noteIds"], json!([first["noteId"]]));
        let none = call(
            &core,
            "findDuplicates",
            json!({ "noteTypeId": ids.basic, "value": "pies" }),
        )
        .unwrap();
        assert_eq!(none["noteIds"], json!([]));
        let second = call(&core, "addNote", add_input(&ids, "kot", &[])).unwrap();
        assert_eq!(second["duplicates"], json!([first["noteId"]]));
    }

    #[test]
    fn a_note_that_makes_no_cards_says_what_to_fill_in() {
        let core = core();
        let ids = ids(&core);
        let err = call(&core, "addNote", add_input(&ids, "", &[])).unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
        assert_eq!(
            err.message,
            "This note would make no cards. Fill in the front."
        );
        let err = call(
            &core,
            "addNote",
            json!({
                "deckId": ids.deck,
                "noteTypeId": ids.cloze,
                "fields": [{ "fieldId": ids.text, "value": "no cloze here" }],
                "tags": [],
            }),
        )
        .unwrap_err();
        assert_eq!(
            err.message,
            "This note would make no cards. Add a cloze with the Cloze button."
        );
    }

    #[test]
    fn a_bad_tag_refuses_the_note_with_a_plain_message() {
        let core = core();
        let ids = ids(&core);
        let err = call(&core, "addNote", add_input(&ids, "kot", &["two words"])).unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
        assert!(err.message.contains("two words"), "{}", err.message);
        let decks = call(&core, "getDeckList", Value::Null).unwrap();
        assert_eq!(decks["totalCards"], 0, "nothing was added");
    }

    #[test]
    fn a_missing_deck_or_note_type_is_not_found() {
        let core = core();
        let ids = ids(&core);
        let gone = Id::from_bytes([9; 16]).to_string();
        let mut input = add_input(&ids, "kot", &[]);
        input["deckId"] = json!(gone);
        let err = call(&core, "addNote", input).unwrap_err();
        assert_eq!(err.kind, ErrorKind::NotFound);
        assert!(err.message.contains("deleted"), "{}", err.message);
        let mut input = add_input(&ids, "kot", &[]);
        input["noteTypeId"] = json!(gone);
        assert_eq!(
            call(&core, "addNote", input).unwrap_err().kind,
            ErrorKind::NotFound
        );
    }

    #[test]
    fn a_field_of_another_note_type_is_invalid_input() {
        let core = core();
        let ids = ids(&core);
        let mut input = add_input(&ids, "kot", &[]);
        input["fields"][0]["fieldId"] = json!(ids.text);
        assert_eq!(
            call(&core, "addNote", input).unwrap_err().kind,
            ErrorKind::InvalidInput
        );
    }

    #[test]
    fn an_id_that_is_not_an_id_is_invalid_input() {
        let core = core();
        let err = call(
            &core,
            "findDuplicates",
            json!({ "noteTypeId": "nope", "value": "x" }),
        )
        .unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
    }

    #[test]
    fn media_is_stored_once_and_comes_back_through_get_media() {
        let core = core();
        let first = with_bytes(
            &core,
            "addMedia",
            json!({ "name": "cat.png" }),
            b"png bytes",
        )
        .unwrap();
        assert_eq!(first["new"], true);
        let name = first["name"].as_str().unwrap();
        assert!(name.starts_with("cat-") && name.ends_with(".png"), "{name}");
        let again = with_bytes(
            &core,
            "addMedia",
            json!({ "name": "cat.png" }),
            b"png bytes",
        )
        .unwrap();
        assert_eq!(again["name"], first["name"]);
        assert_eq!(again["new"], false);
        // The bytes are stored once even under another file name; only the stem differs.
        let other = with_bytes(
            &core,
            "addMedia",
            json!({ "name": "kitten.png" }),
            b"png bytes",
        )
        .unwrap();
        assert_eq!(other["new"], false);
        let reply = dispatch(
            &core,
            "getMedia",
            json!({ "name": name }),
            &OpContext::uncancellable(),
        )
        .unwrap();
        assert_eq!(reply.attachment.as_deref(), Some(&b"png bytes"[..]));
    }

    #[test]
    fn media_without_bytes_is_invalid_input() {
        let core = core();
        let none = call(&core, "addMedia", json!({ "name": "a.png" })).unwrap_err();
        assert_eq!(none.kind, ErrorKind::InvalidInput);
        let empty = with_bytes(&core, "addMedia", json!({ "name": "a.png" }), b"").unwrap_err();
        assert_eq!(empty.kind, ErrorKind::InvalidInput);
    }

    #[test]
    fn without_a_collection_every_method_says_so() {
        let core = Core::new();
        let id = Id::from_bytes([1; 16]).to_string();
        let calls = [
            ("getNoteTypes", Value::Null),
            ("getTags", Value::Null),
            ("findDuplicates", json!({ "noteTypeId": id, "value": "x" })),
            (
                "addNote",
                json!({ "deckId": id, "noteTypeId": id, "fields": [], "tags": [] }),
            ),
        ];
        for (method, input) in calls {
            let err = call(&core, method, input).unwrap_err();
            assert_eq!(err.kind, ErrorKind::NotFound, "{method}");
        }
        let err = with_bytes(&core, "addMedia", json!({ "name": "a.png" }), b"x").unwrap_err();
        assert_eq!(err.kind, ErrorKind::NotFound);
    }
}
