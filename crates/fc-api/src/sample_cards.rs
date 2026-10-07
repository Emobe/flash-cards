//! Debug-only: a few Polish cards to study (step 2.3b). Nothing can add cards until 2.4, so this is
//! how the review screen is tried on a phone, on the desktop and on the web. Registered only when
//! `debug_assertions` is on, like the other debug methods.

use fc_core::Core;
use fc_core::collection::Collection;
use fc_core::id::Id;
use fc_core::notetype::builtin;
use serde::Serialize;
use ts_rs::TS;

use crate::collection::not_open;
use crate::{ApiError, Method, OpContext};

const SAMPLE_PNG: &[u8] = include_bytes!("sample_media/sample.png");
const SAMPLE_WAV: &[u8] = include_bytes!("sample_media/sample.wav");

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SampleCardsOutput {
    /// The notes added (a note can make more than one card).
    pub notes: u32,
}

/// Adds the sample deck `Sample` (words, a reversed pair, a cloze) and `Sample::Sound` (cards with
/// the sample sound and image). Calling it again adds the same notes again.
pub struct DebugAddSampleCards;

impl Method for DebugAddSampleCards {
    const NAME: &'static str = "debugAddSampleCards";
    type Input = ();
    type Output = SampleCardsOutput;

    fn call(core: &Core, (): (), _: &OpContext) -> Result<SampleCardsOutput, ApiError> {
        core.with_collection(add_samples)
            .ok_or_else(|| not_open(core))?
            .map_err(|message| {
                eprintln!("Could not add the sample cards: {message}");
                ApiError::internal()
            })
    }
}

fn deck_named(c: &Collection, name: &str, parent: Option<Id>) -> Result<Id, String> {
    let existing = c
        .decks()
        .map_err(|e| format!("{e:?}"))?
        .into_iter()
        .find(|d| d.display_name == name && d.parent == parent);
    match existing {
        Some(deck) => Ok(deck.id),
        None => c.create_deck(name, parent).map_err(|e| format!("{e:?}")),
    }
}

fn add_samples(c: &Collection) -> Result<SampleCardsOutput, String> {
    let debug = |e: &dyn std::fmt::Debug| format!("{e:?}");
    let sample = deck_named(c, "Sample", None)?;
    let sound = deck_named(c, "Sound", Some(sample))?;
    let basic = c
        .note_type(builtin::basic())
        .map_err(|e| debug(&e))?
        .ok_or("no Basic")?
        .fields;
    let reversed = c
        .note_type(builtin::basic_and_reversed())
        .map_err(|e| debug(&e))?
        .ok_or("no Basic and reversed")?
        .fields;
    let cloze = c
        .note_type(builtin::cloze())
        .map_err(|e| debug(&e))?
        .ok_or("no Cloze")?
        .fields;
    let mut notes = 0;

    for (front, back) in [
        ("dom", "house"),
        ("kot", "cat"),
        ("pies", "dog"),
        ("woda", "water"),
        ("chleb", "bread"),
        ("dziękuję", "thank you"),
        ("proszę", "please / here you are"),
        ("dzień dobry", "good morning, hello"),
    ] {
        c.add_note_to_deck(
            sample,
            builtin::basic(),
            &[(basic[0].id, front), (basic[1].id, back)],
        )
        .map_err(|e| debug(&e))?;
        notes += 1;
    }
    c.add_note_to_deck(
        sample,
        builtin::basic_and_reversed(),
        &[(reversed[0].id, "mleko"), (reversed[1].id, "milk")],
    )
    .map_err(|e| debug(&e))?;
    c.add_note_to_deck(
        sample,
        builtin::cloze(),
        &[
            (cloze[0].id, "{{c1::Warszawa}} jest stolicą {{c2::Polski}}."),
            (cloze[1].id, "Warsaw is the capital of Poland."),
        ],
    )
    .map_err(|e| debug(&e))?;
    notes += 2;

    let wav = c
        .add_media("sample.wav", SAMPLE_WAV)
        .map_err(|e| debug(&e))?
        .name;
    let png = c
        .add_media("sample.png", SAMPLE_PNG)
        .map_err(|e| debug(&e))?
        .name;
    for (front, back) in [
        (format!("słuchaj [sound:{wav}]"), "listen".to_owned()),
        (
            "co to jest?".to_owned(),
            format!("a picture <img src=\"{png}\"> [sound:{wav}]"),
        ),
        (
            format!("[sound:{wav}] [sound:{wav}]"),
            "two sounds in a row".to_owned(),
        ),
    ] {
        c.add_note_to_deck(
            sound,
            builtin::basic(),
            &[(basic[0].id, &front), (basic[1].id, &back)],
        )
        .map_err(|e| debug(&e))?;
        notes += 1;
    }
    Ok(SampleCardsOutput { notes })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use fc_core::clock::ManualClock;
    use serde_json::{Value, json};

    use crate::{Host, dispatch};

    use super::*;

    #[test]
    fn it_adds_decks_with_cards_and_media_that_render() {
        let core = Core::new();
        let host = Host {
            clock: Arc::new(ManualClock::new(1_791_363_600_000)),
            installation_id: Id::from_bytes([1; 16]),
        };
        core.open_collection(":memory:", host).unwrap();
        let ctx = OpContext::uncancellable();
        let out = dispatch(&core, "debugAddSampleCards", Value::Null, &ctx).unwrap();
        assert_eq!(out.output, json!({ "notes": 13 }));

        let list = dispatch(&core, "getDeckList", Value::Null, &ctx)
            .unwrap()
            .output;
        assert_eq!(
            list["totalCards"], 15,
            "11 notes with one card, a reversed pair, two clozes"
        );
        let decks = list["decks"].as_array().unwrap();
        let sound = decks.iter().find(|d| d["path"] == "Sample::Sound").unwrap();
        assert_eq!(sound["newCount"], 3);

        // Every card renders and every media file it names can be read back.
        let next = |deck: &str| {
            dispatch(&core, "nextCard", json!({ "deckId": deck }), &ctx)
                .unwrap()
                .output
        };
        let card = next(sound["id"].as_str().unwrap());
        let html = dispatch(
            &core,
            "renderCard",
            json!({ "cardId": card["cardId"] }),
            &ctx,
        )
        .unwrap()
        .output;
        let media = html["media"].as_array().unwrap();
        assert!(!media.is_empty());
        for name in media {
            let reply = dispatch(&core, "getMedia", json!({ "name": name }), &ctx).unwrap();
            assert!(reply.attachment.is_some_and(|b| !b.is_empty()));
        }

        // Asking twice adds the notes again, into the same decks.
        dispatch(&core, "debugAddSampleCards", Value::Null, &ctx).unwrap();
        let list = dispatch(&core, "getDeckList", Value::Null, &ctx)
            .unwrap()
            .output;
        assert_eq!(list["totalCards"], 30);
        assert_eq!(
            list["decks"].as_array().unwrap().len(),
            3,
            "Default, Sample, Sound"
        );
    }
}
