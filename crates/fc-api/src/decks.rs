//! The deck list for the home screen (step 2.2) and changing decks (step 2.5a).

use std::collections::HashMap;

use fc_core::Core;
use fc_core::deck::DeckError;
use fc_core::id::Id;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::collection::not_open;
use crate::study::{internal, parse_id};
use crate::{ApiError, ErrorKind, Method, OpContext};

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct DeckSummary {
    /// A UUID string.
    pub id: String,
    /// The name to show, for this deck alone (not the whole path).
    pub name: String,
    /// The names from the top level down, joined by `::`.
    pub path: String,
    /// The deck it is inside, or null at the top level.
    pub parent_id: Option<String>,
    /// 0 at the top level.
    pub depth: u32,
    /// The Default deck, which cannot be deleted.
    pub is_default: bool,
    /// What is left to study today in this deck and the decks inside it.
    pub new_count: u32,
    pub learning_count: u32,
    pub review_count: u32,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct DeckList {
    /// Every deck that is not deleted, each followed by the decks inside it.
    pub decks: Vec<DeckSummary>,
    /// Cards in the whole collection, so the screen can tell a new user from a finished one.
    pub total_cards: u32,
}

/// Details go to the log, never to the user.
fn log(error: impl std::fmt::Debug) -> ApiError {
    eprintln!("Could not read the deck list: {error:?}");
    ApiError::internal()
}

/// The decks in tree order with their counts for today, read together so they agree.
pub struct GetDeckList;

impl Method for GetDeckList {
    const NAME: &'static str = "getDeckList";
    type Input = ();
    type Output = DeckList;

    fn call(core: &Core, (): (), _: &OpContext) -> Result<DeckList, ApiError> {
        let (decks, counts) = core
            .with_collection(|c| {
                Ok::<_, ApiError>((c.decks().map_err(log)?, c.deck_counts().map_err(log)?))
            })
            .ok_or_else(|| not_open(core))??;
        let counts: HashMap<_, _> = counts.into_iter().map(|d| (d.deck, d.counts)).collect();
        let total = decks.iter().map(|d| d.cards).sum::<usize>();
        let decks = decks
            .into_iter()
            .map(|deck| {
                let counts = counts.get(&deck.id).copied().unwrap_or_default();
                DeckSummary {
                    id: deck.id.to_string(),
                    name: deck.display_name,
                    path: deck.path,
                    parent_id: deck.parent.map(|p| p.to_string()),
                    depth: u32::try_from(deck.depth).unwrap_or(u32::MAX),
                    is_default: deck.id == fc_core::deck::default_deck(),
                    new_count: counts.new,
                    learning_count: counts.learning,
                    review_count: counts.review,
                }
            })
            .collect();
        Ok(DeckList {
            decks,
            total_cards: u32::try_from(total).unwrap_or(u32::MAX),
        })
    }
}

fn deck_error(what: &str, error: DeckError) -> ApiError {
    match error {
        DeckError::NotFound => ApiError::new(
            ErrorKind::NotFound,
            "That deck was deleted. Go back to the deck list and try again.",
        ),
        DeckError::EmptyName
        | DeckError::NameHasSeparator
        | DeckError::NameTaken(_)
        | DeckError::MoveIntoItself
        | DeckError::Default => ApiError::invalid_input(error.to_string()),
        other => internal(what, other),
    }
}

fn optional_id(text: Option<&str>) -> Result<Option<Id>, ApiError> {
    text.map(parse_id).transpose()
}

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct CreateDeckInput {
    pub name: String,
    /// The deck to put it inside, or null for the top level.
    pub parent_id: Option<String>,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct CreatedDeck {
    /// A UUID string.
    pub id: String,
}

/// Makes a deck at the top level or inside another.
pub struct CreateDeck;

impl Method for CreateDeck {
    const NAME: &'static str = "createDeck";
    type Input = CreateDeckInput;
    type Output = CreatedDeck;

    fn call(core: &Core, input: CreateDeckInput, _: &OpContext) -> Result<CreatedDeck, ApiError> {
        let parent = optional_id(input.parent_id.as_deref())?;
        let id = core
            .with_collection(|c| c.create_deck(&input.name, parent))
            .ok_or_else(|| not_open(core))?
            .map_err(|error| deck_error("create the deck", error))?;
        Ok(CreatedDeck { id: id.to_string() })
    }
}

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RenameDeckInput {
    pub deck_id: String,
    pub name: String,
}

pub struct RenameDeck;

impl Method for RenameDeck {
    const NAME: &'static str = "renameDeck";
    type Input = RenameDeckInput;
    type Output = ();

    fn call(core: &Core, input: RenameDeckInput, _: &OpContext) -> Result<(), ApiError> {
        let id = parse_id(&input.deck_id)?;
        core.with_collection(|c| c.rename_deck(id, &input.name))
            .ok_or_else(|| not_open(core))?
            .map_err(|error| deck_error("rename the deck", error))
    }
}

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct MoveDeckInput {
    pub deck_id: String,
    /// The deck to put it inside, or null for the top level.
    pub parent_id: Option<String>,
}

/// Moves a deck, with the decks inside it.
pub struct MoveDeck;

impl Method for MoveDeck {
    const NAME: &'static str = "moveDeck";
    type Input = MoveDeckInput;
    type Output = ();

    fn call(core: &Core, input: MoveDeckInput, _: &OpContext) -> Result<(), ApiError> {
        let id = parse_id(&input.deck_id)?;
        let parent = optional_id(input.parent_id.as_deref())?;
        core.with_collection(|c| c.move_deck(id, parent))
            .ok_or_else(|| not_open(core))?
            .map_err(|error| deck_error("move the deck", error))
    }
}

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct DeckIdInput {
    pub deck_id: String,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct DeletedDeck {
    /// Decks that went with it, not counting the deck itself.
    pub decks: u32,
    /// Cards that went with it and the decks inside it.
    pub cards: u32,
}

fn counts(c: &fc_core::collection::Collection) -> Result<(u32, u32), DeckError> {
    let decks = c.decks()?;
    let cards = decks.iter().map(|d| d.cards).sum::<usize>();
    Ok((
        u32::try_from(decks.len()).unwrap_or(u32::MAX),
        u32::try_from(cards).unwrap_or(u32::MAX),
    ))
}

/// Deletes a deck, the decks inside it and their cards. `restoreDeck` brings them back.
pub struct DeleteDeck;

impl Method for DeleteDeck {
    const NAME: &'static str = "deleteDeck";
    type Input = DeckIdInput;
    type Output = DeletedDeck;

    fn call(core: &Core, input: DeckIdInput, _: &OpContext) -> Result<DeletedDeck, ApiError> {
        let id = parse_id(&input.deck_id)?;
        let (decks, cards) = core
            .with_collection(|c| {
                let before = counts(c)?;
                c.delete_deck(id)?;
                let after = counts(c)?;
                Ok::<_, DeckError>((
                    before.0.saturating_sub(after.0).saturating_sub(1),
                    before.1.saturating_sub(after.1),
                ))
            })
            .ok_or_else(|| not_open(core))?
            .map_err(|error| deck_error("delete the deck", error))?;
        Ok(DeletedDeck { decks, cards })
    }
}

/// Brings a deleted deck back with what was deleted along with it.
pub struct RestoreDeck;

impl Method for RestoreDeck {
    const NAME: &'static str = "restoreDeck";
    type Input = DeckIdInput;
    type Output = ();

    fn call(core: &Core, input: DeckIdInput, _: &OpContext) -> Result<(), ApiError> {
        let id = parse_id(&input.deck_id)?;
        core.with_collection(|c| c.restore_deck(id))
            .ok_or_else(|| not_open(core))?
            .map_err(|error| deck_error("restore the deck", error))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use fc_core::clock::ManualClock;
    use fc_core::notetype::builtin;
    use serde_json::{Value, json};

    use crate::{Host, dispatch};

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

    fn deck_list(core: &Core) -> Value {
        dispatch(
            core,
            "getDeckList",
            Value::Null,
            &OpContext::uncancellable(),
        )
        .unwrap()
        .output
    }

    fn add_cards(core: &Core, deck: Id, words: &[&str]) {
        core.with_collection(|c| {
            let fields = c.note_type(builtin::basic()).unwrap().unwrap().fields;
            for word in words {
                c.add_note_to_deck(
                    deck,
                    builtin::basic(),
                    &[(fields[0].id, word), (fields[1].id, "x")],
                )
                .unwrap();
            }
        })
        .unwrap();
    }

    #[test]
    fn a_new_collection_has_the_default_deck_and_no_cards() {
        let list = deck_list(&core());
        assert_eq!(list["totalCards"], 0);
        let decks = list["decks"].as_array().unwrap();
        assert_eq!(decks.len(), 1);
        assert_eq!(decks[0]["parentId"], Value::Null);
        assert_eq!(decks[0]["depth"], 0);
        assert_eq!(decks[0]["newCount"], 0);
    }

    #[test]
    fn counts_roll_up_into_the_parent_and_the_tree_is_in_order() {
        let core = core();
        let (polish, verbs) = core
            .with_collection(|c| {
                let polish = c.create_deck("Polish", None).unwrap();
                let verbs = c.create_deck("Verbs", Some(polish)).unwrap();
                (polish, verbs)
            })
            .unwrap();
        add_cards(&core, polish, &["dom", "kot"]);
        add_cards(&core, verbs, &["byc", "miec", "robic"]);

        let list = deck_list(&core);
        assert_eq!(list["totalCards"], 5);
        let decks = list["decks"].as_array().unwrap();
        let find = |name: &str| decks.iter().position(|d| d["name"] == name).unwrap();
        let (p, v) = (find("Polish"), find("Verbs"));
        assert_eq!(v, p + 1, "a deck is followed by the decks inside it");
        assert_eq!(decks[p]["newCount"], 5);
        assert_eq!(decks[p]["learningCount"], 0);
        assert_eq!(decks[p]["reviewCount"], 0);
        assert_eq!(decks[p]["path"], "Polish");
        assert_eq!(decks[v]["newCount"], 3);
        assert_eq!(decks[v]["depth"], 1);
        assert_eq!(decks[v]["path"], "Polish::Verbs");
        assert_eq!(decks[v]["parentId"], json!(polish.to_string()));
        assert_eq!(decks[v]["id"], json!(verbs.to_string()));
    }

    #[test]
    fn a_deleted_deck_is_left_out_with_its_cards() {
        let core = core();
        let polish = core
            .with_collection(|c| c.create_deck("Polish", None).unwrap())
            .unwrap();
        add_cards(&core, polish, &["dom"]);
        core.with_collection(|c| c.delete_deck(polish).unwrap())
            .unwrap();
        let list = deck_list(&core);
        assert_eq!(list["totalCards"], 0);
        assert!(
            list["decks"]
                .as_array()
                .unwrap()
                .iter()
                .all(|d| d["name"] != "Polish")
        );
    }

    fn call(core: &Core, method: &str, input: Value) -> Result<Value, ApiError> {
        dispatch(core, method, input, &OpContext::uncancellable()).map(|o| o.output)
    }

    fn make(core: &Core, name: &str, parent: Option<&str>) -> String {
        call(
            core,
            "createDeck",
            json!({ "name": name, "parentId": parent }),
        )
        .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    fn paths(core: &Core) -> Vec<String> {
        deck_list(core)["decks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["path"].as_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn a_deck_is_created_renamed_and_moved() {
        let core = core();
        let polish = make(&core, "  Polish ", None);
        let verbs = make(&core, "Verbs", Some(&polish));
        assert_eq!(paths(&core), ["Default", "Polish", "Polish::Verbs"]);

        call(
            &core,
            "renameDeck",
            json!({ "deckId": verbs, "name": "Grammar" }),
        )
        .unwrap();
        assert_eq!(paths(&core), ["Default", "Polish", "Polish::Grammar"]);

        call(
            &core,
            "moveDeck",
            json!({ "deckId": verbs, "parentId": null }),
        )
        .unwrap();
        assert_eq!(paths(&core), ["Default", "Grammar", "Polish"]);
    }

    #[test]
    fn bad_names_and_moves_say_what_to_do() {
        let core = core();
        let polish = make(&core, "Polish", None);
        let verbs = make(&core, "Verbs", Some(&polish));
        for (method, input) in [
            ("createDeck", json!({ "name": "  ", "parentId": null })),
            ("createDeck", json!({ "name": "a::b", "parentId": null })),
            ("createDeck", json!({ "name": "polish", "parentId": null })),
            ("renameDeck", json!({ "deckId": verbs, "name": "" })),
            ("moveDeck", json!({ "deckId": polish, "parentId": verbs })),
            ("moveDeck", json!({ "deckId": polish, "parentId": polish })),
        ] {
            let err = call(&core, method, input).unwrap_err();
            assert_eq!(err.kind, ErrorKind::InvalidInput, "{method}");
            assert!(!err.message.is_empty());
        }
        assert_eq!(paths(&core), ["Default", "Polish", "Polish::Verbs"]);
    }

    #[test]
    fn a_bad_or_missing_deck_is_refused() {
        let core = core();
        let gone = Id::from_bytes([9; 16]).to_string();
        for (method, input) in [
            ("renameDeck", json!({ "deckId": gone, "name": "x" })),
            ("moveDeck", json!({ "deckId": gone, "parentId": null })),
            ("deleteDeck", json!({ "deckId": gone })),
            ("restoreDeck", json!({ "deckId": gone })),
            ("createDeck", json!({ "name": "x", "parentId": gone })),
        ] {
            assert_eq!(
                call(&core, method, input).unwrap_err().kind,
                ErrorKind::NotFound,
                "{method}"
            );
        }
        let err = call(&core, "deleteDeck", json!({ "deckId": "nope" })).unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
    }

    #[test]
    fn delete_counts_what_went_and_restore_brings_it_back() {
        let core = core();
        let polish = make(&core, "Polish", None);
        let verbs = make(&core, "Verbs", Some(&polish));
        let nouns = make(&core, "Nouns", Some(&polish));
        let other = make(&core, "French", None);
        let id = |text: &str| text.parse::<Id>().unwrap();
        add_cards(&core, id(&polish), &["dom"]);
        add_cards(&core, id(&verbs), &["byc", "isc"]);
        add_cards(&core, id(&nouns), &["kot"]);
        add_cards(&core, id(&other), &["chat"]);

        let gone = call(&core, "deleteDeck", json!({ "deckId": polish })).unwrap();
        assert_eq!(gone, json!({ "decks": 2, "cards": 4 }));
        assert_eq!(paths(&core), ["Default", "French"]);
        assert_eq!(deck_list(&core)["totalCards"], 1);

        // Deleting again does nothing and counts nothing.
        let again = call(&core, "deleteDeck", json!({ "deckId": polish })).unwrap();
        assert_eq!(again, json!({ "decks": 0, "cards": 0 }));

        call(&core, "restoreDeck", json!({ "deckId": polish })).unwrap();
        assert_eq!(
            paths(&core),
            [
                "Default",
                "French",
                "Polish",
                "Polish::Nouns",
                "Polish::Verbs"
            ]
        );
        assert_eq!(deck_list(&core)["totalCards"], 5);
    }

    #[test]
    fn the_default_deck_cannot_be_deleted_and_a_taken_name_blocks_restore() {
        let core = core();
        let default = fc_core::deck::default_deck().to_string();
        let err = call(&core, "deleteDeck", json!({ "deckId": default })).unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
        assert!(err.message.contains("Default"));

        let polish = make(&core, "Polish", None);
        call(&core, "deleteDeck", json!({ "deckId": polish })).unwrap();
        make(&core, "Polish", None);
        let err = call(&core, "restoreDeck", json!({ "deckId": polish })).unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
        assert!(err.message.contains("Polish"));
    }

    #[test]
    fn without_a_collection_it_says_so() {
        let err = dispatch(
            &Core::new(),
            "getDeckList",
            Value::Null,
            &OpContext::uncancellable(),
        )
        .unwrap_err();
        assert_eq!(err.kind, crate::ErrorKind::NotFound);
    }
}
