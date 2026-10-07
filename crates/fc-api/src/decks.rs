//! The deck list for the home screen (step 2.2).

use std::collections::HashMap;

use fc_core::Core;
use serde::Serialize;
use ts_rs::TS;

use crate::collection::not_open;
use crate::{ApiError, Method, OpContext};

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
