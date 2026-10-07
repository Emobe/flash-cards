//! The review screen's methods (step 2.3a): a study session, the next card, its HTML, answering,
//! undo and media bytes. Thin wrappers over `fc_core::study`, `render_card` and `media_bytes`.

use std::str::FromStr;

use fc_core::Core;
use fc_core::id::Id;
use fc_core::media::MediaError;
use fc_core::scheduling::{CardState, Due, Rating};
use fc_core::study::{Counts, Next, StudyError};
use fc_core::template::RenderError;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::collection::not_open;
use crate::{ApiError, ErrorKind, EventRating, Method, OpContext, SessionSummary};

fn parse_id(text: &str) -> Result<Id, ApiError> {
    Id::from_str(text).map_err(|_| ApiError::invalid_input("That is not a valid ID."))
}

/// Details of an unexpected failure go to the log, never to the user.
fn internal(what: &str, error: impl std::fmt::Debug) -> ApiError {
    eprintln!("Could not {what}: {error:?}");
    ApiError::internal()
}

fn study_error(what: &str, error: StudyError) -> ApiError {
    match error {
        StudyError::NotFound => ApiError::new(ErrorKind::NotFound, error.to_string()),
        StudyError::StartHour
        | StudyError::LaterAnswer
        | StudyError::Scheduling(_)
        | StudyError::NotEnoughHistory { .. } => ApiError::invalid_input(error.to_string()),
        StudyError::Collection(inner) => match ApiError::from(inner) {
            api if api == ApiError::internal() => internal(what, &api),
            api => api,
        },
    }
}

/// Runs `f` on the open collection.
fn with<T>(
    core: &Core,
    what: &str,
    f: impl FnOnce(&fc_core::collection::Collection) -> Result<T, StudyError>,
) -> Result<T, ApiError> {
    core.with_collection(f)
        .ok_or_else(|| not_open(core))?
        .map_err(|error| study_error(what, error))
}

// ---- Sessions -------------------------------------------------------------------------------------

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct StartSessionInput {
    /// A UUID string. The deck and the decks inside it are studied.
    pub deck_id: String,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct StartSessionOutput {
    pub session_id: String,
}

/// Starts a study session on a deck. One that was open ends as replaced (ADR 0009).
pub struct StartStudySession;

impl Method for StartStudySession {
    const NAME: &'static str = "startStudySession";
    type Input = StartSessionInput;
    type Output = StartSessionOutput;

    fn call(core: &Core, input: Self::Input, _: &OpContext) -> Result<Self::Output, ApiError> {
        let deck = parse_id(&input.deck_id)?;
        let session = with(core, "start a study session", |c| {
            c.start_study_session(Some(deck))
        })?;
        Ok(StartSessionOutput {
            session_id: session.to_string(),
        })
    }
}

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct EndSessionInput {
    pub session_id: String,
}

/// Ends a study session and returns what it did, or null if it was not open.
pub struct EndStudySession;

impl Method for EndStudySession {
    const NAME: &'static str = "endStudySession";
    type Input = EndSessionInput;
    type Output = Option<SessionSummary>;

    fn call(core: &Core, input: Self::Input, _: &OpContext) -> Result<Self::Output, ApiError> {
        let session = parse_id(&input.session_id)?;
        let summary = with(core, "end a study session", |c| {
            c.end_study_session(session)
        })?;
        Ok(summary.map(|s| SessionSummary {
            answered: s.answered,
            again: s.again,
            studied_ms: s.studied_ms,
            elapsed_ms: s.elapsed_ms,
        }))
    }
}

// ---- The next card --------------------------------------------------------------------------------

/// What is left to study today in the deck being studied.
#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct StudyCounts {
    pub new_count: u32,
    pub learning_count: u32,
    pub review_count: u32,
}

impl From<Counts> for StudyCounts {
    fn from(counts: Counts) -> Self {
        Self {
            new_count: counts.new,
            learning_count: counts.learning,
            review_count: counts.review,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum PreviewUnit {
    Minutes,
    Days,
}

/// When a card would come back after an answer. The UI words it ("10m", "3d").
#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct IntervalPreview {
    pub unit: PreviewUnit,
    pub amount: u32,
}

#[derive(Debug, Clone, Copy, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum StudyCardState {
    New,
    Learning,
    Review,
    Relearning,
}

#[derive(Debug, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum StudyNext {
    /// Show this card. `previews` are for Again, Hard, Good and Easy, in that order.
    Card {
        card_id: String,
        deck_id: String,
        state: StudyCardState,
        previews: Vec<IntervalPreview>,
        counts: StudyCounts,
    },
    /// Nothing is due now, but a learning card is due in `wait_seconds`.
    Waiting {
        wait_seconds: u32,
        counts: StudyCounts,
    },
    /// Nothing more to study today.
    Done { counts: StudyCounts },
}

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct NextCardInput {
    pub deck_id: String,
}

/// The card to study next in a deck, with the next interval for each answer.
pub struct NextCard;

impl Method for NextCard {
    const NAME: &'static str = "nextCard";
    type Input = NextCardInput;
    type Output = StudyNext;

    fn call(core: &Core, input: Self::Input, _: &OpContext) -> Result<Self::Output, ApiError> {
        let deck = parse_id(&input.deck_id)?;
        let (next, now) = with(core, "find the next card", |c| {
            Ok((c.next_card(deck)?, c.now_ms()))
        })?;
        Ok(match next {
            Next::Card {
                card,
                deck,
                state,
                previews,
                counts,
            } => StudyNext::Card {
                card_id: card.to_string(),
                deck_id: deck.to_string(),
                state: match state {
                    CardState::New => StudyCardState::New,
                    CardState::Learning => StudyCardState::Learning,
                    CardState::Review => StudyCardState::Review,
                    CardState::Relearning => StudyCardState::Relearning,
                },
                previews: previews
                    .iter()
                    .map(|due| match *due {
                        Due::Minutes(amount) => IntervalPreview {
                            unit: PreviewUnit::Minutes,
                            amount,
                        },
                        Due::Days(amount) => IntervalPreview {
                            unit: PreviewUnit::Days,
                            amount,
                        },
                    })
                    .collect(),
                counts: counts.into(),
            },
            Next::Waiting { until_ms, counts } => StudyNext::Waiting {
                // Rounded up, so asking again after this long finds the card.
                wait_seconds: u32::try_from(((until_ms - now).max(0) + 999) / 1000)
                    .unwrap_or(u32::MAX),
                counts: counts.into(),
            },
            Next::Done { counts } => StudyNext::Done {
                counts: counts.into(),
            },
        })
    }
}

// ---- Showing a card -------------------------------------------------------------------------------

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RenderCardInput {
    pub card_id: String,
}

/// Both sides of a card as complete HTML documents for the card frame. Untrusted content.
#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RenderedCardOutput {
    pub front: String,
    pub back: String,
    /// Names of the media files either side uses, for `getMedia`.
    pub media: Vec<String>,
}

pub struct RenderCard;

impl Method for RenderCard {
    const NAME: &'static str = "renderCard";
    type Input = RenderCardInput;
    type Output = RenderedCardOutput;

    fn call(core: &Core, input: Self::Input, _: &OpContext) -> Result<Self::Output, ApiError> {
        let card = parse_id(&input.card_id)?;
        let rendered = core
            .with_collection(|c| c.render_card(card))
            .ok_or_else(|| not_open(core))?
            .map_err(|error| match error {
                RenderError::NotFound => ApiError::new(ErrorKind::NotFound, error.to_string()),
                // The message says which template and where, so the person can fix it.
                RenderError::Template(_) => ApiError::invalid_input(error.to_string()),
                RenderError::Collection(inner) => match ApiError::from(inner) {
                    api if api == ApiError::internal() => internal("render a card", &api),
                    api => api,
                },
            })?;
        Ok(RenderedCardOutput {
            front: rendered.front,
            back: rendered.back,
            media: rendered.media,
        })
    }
}

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct GetMediaInput {
    pub name: String,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct GetMediaOutput {
    /// For the `Blob` that wraps the attachment.
    pub content_type: String,
}

/// The type to tell the browser from the file name's extension. The core stores no type.
fn content_type(name: &str) -> &'static str {
    let extension = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" | "oga" | "opus" => "audio/ogg",
        "m4a" | "aac" => "audio/mp4",
        "flac" => "audio/flac",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
}

/// The bytes of a media file as the reply attachment (ADR 0005: for the card frame, as a `Blob`).
pub struct GetMedia;

impl Method for GetMedia {
    const NAME: &'static str = "getMedia";
    const ATTACHMENT_OUT: bool = true;
    type Input = GetMediaInput;
    type Output = GetMediaOutput;

    fn call(core: &Core, input: Self::Input, ctx: &OpContext) -> Result<Self::Output, ApiError> {
        let bytes = core
            .with_collection(|c| c.media_bytes(&input.name))
            .ok_or_else(|| not_open(core))?
            .map_err(|error| match error {
                MediaError::NotFound(_) | MediaError::NoBytes(_) => {
                    ApiError::new(ErrorKind::NotFound, error.to_string())
                }
                MediaError::Empty | MediaError::StillUsed(_) => {
                    ApiError::invalid_input(error.to_string())
                }
                MediaError::Collection(inner) => match ApiError::from(inner) {
                    api if api == ApiError::internal() => internal("read a media file", &api),
                    api => api,
                },
            })?;
        ctx.set_reply_attachment(bytes);
        Ok(GetMediaOutput {
            content_type: content_type(&input.name).to_owned(),
        })
    }
}

// ---- Answering ------------------------------------------------------------------------------------

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AnswerCardInput {
    pub card_id: String,
    pub rating: EventRating,
    /// How long the person took, in milliseconds. The core keeps up to an hour.
    pub duration_ms: u32,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AnswerCardOutput {
    /// The review that records it.
    pub event_id: String,
}

pub struct AnswerCard;

impl Method for AnswerCard {
    const NAME: &'static str = "answerCard";
    type Input = AnswerCardInput;
    type Output = AnswerCardOutput;

    fn call(core: &Core, input: Self::Input, _: &OpContext) -> Result<Self::Output, ApiError> {
        let card = parse_id(&input.card_id)?;
        let rating = match input.rating {
            EventRating::Again => Rating::Again,
            EventRating::Hard => Rating::Hard,
            EventRating::Good => Rating::Good,
            EventRating::Easy => Rating::Easy,
        };
        let answered = with(core, "record an answer", |c| {
            c.answer(card, rating, input.duration_ms)
        })?;
        Ok(AnswerCardOutput {
            event_id: answered.event.to_string(),
        })
    }
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct UndoneAnswer {
    pub card_id: String,
}

/// Takes back the newest answer made on this device. Null when there is none.
pub struct UndoAnswer;

impl Method for UndoAnswer {
    const NAME: &'static str = "undoAnswer";
    type Input = ();
    type Output = Option<UndoneAnswer>;

    fn call(core: &Core, (): (), _: &OpContext) -> Result<Self::Output, ApiError> {
        let undone = with(core, "undo an answer", |c| c.undo_answer())?;
        Ok(undone.map(|u| UndoneAnswer {
            card_id: u.card.to_string(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use fc_core::clock::ManualClock;
    use fc_core::deck::{PresetChange, default_deck, default_preset};
    use fc_core::notetype::builtin;
    use serde_json::{Value, json};

    use crate::{Host, dispatch};

    use super::*;

    fn core() -> (Core, Arc<ManualClock>) {
        let clock = Arc::new(ManualClock::new(1_791_363_600_000));
        let core = Core::new();
        let host = Host {
            clock: clock.clone(),
            installation_id: Id::from_bytes([1; 16]),
        };
        core.open_collection(":memory:", host).unwrap();
        (core, clock)
    }

    fn call(core: &Core, method: &str, input: Value) -> Result<Value, ApiError> {
        dispatch(core, method, input, &OpContext::uncancellable()).map(|r| r.output)
    }

    fn add_cards(core: &Core, fronts: &[&str]) {
        core.with_collection(|c| {
            let fields = c.note_type(builtin::basic()).unwrap().unwrap().fields;
            for front in fronts {
                c.add_note_to_deck(
                    default_deck(),
                    builtin::basic(),
                    &[(fields[0].id, front), (fields[1].id, "back")],
                )
                .unwrap();
            }
        })
        .unwrap();
    }

    fn deck() -> String {
        default_deck().to_string()
    }

    fn next(core: &Core) -> Value {
        call(core, "nextCard", json!({ "deckId": deck() })).unwrap()
    }

    #[test]
    fn a_session_shows_answers_undoes_and_ends() {
        let (core, _) = core();
        add_cards(&core, &["dom", "kot"]);
        let session = call(&core, "startStudySession", json!({ "deckId": deck() })).unwrap();
        let session = session["sessionId"].as_str().unwrap().to_owned();

        let first = next(&core);
        assert_eq!(first["kind"], "card");
        assert_eq!(first["state"], "new");
        assert_eq!(first["counts"]["newCount"], 2);
        let previews = first["previews"].as_array().unwrap();
        assert_eq!(previews.len(), 4, "Again, Hard, Good, Easy");
        assert_eq!(previews[0], json!({ "unit": "minutes", "amount": 1 }));
        let card = first["cardId"].as_str().unwrap().to_owned();

        let html = call(&core, "renderCard", json!({ "cardId": card })).unwrap();
        assert!(
            html["front"].as_str().unwrap().contains("dom")
                || html["front"].as_str().unwrap().contains("kot")
        );
        assert!(html["back"].as_str().unwrap().contains("back"));

        let answered = call(
            &core,
            "answerCard",
            json!({ "cardId": card, "rating": "good", "durationMs": 4000 }),
        )
        .unwrap();
        assert!(answered["eventId"].as_str().unwrap().parse::<Id>().is_ok());

        // Undo gives the card back, and the queue offers it again first.
        let undone = call(&core, "undoAnswer", Value::Null).unwrap();
        assert_eq!(undone["cardId"], json!(card));
        let again = next(&core);
        assert_eq!(again["cardId"], json!(card));
        assert_eq!(again["counts"], first["counts"]);
        assert_eq!(call(&core, "undoAnswer", Value::Null).unwrap(), Value::Null);

        call(
            &core,
            "answerCard",
            json!({ "cardId": card, "rating": "again", "durationMs": 2000 }),
        )
        .unwrap();
        let summary = call(&core, "endStudySession", json!({ "sessionId": session })).unwrap();
        assert_eq!(summary["answered"], 1, "the undone answer is left out");
        assert_eq!(summary["again"], 1);
        assert_eq!(summary["studiedMs"], 2000);
        // Ending it twice says nothing the second time.
        let twice = call(&core, "endStudySession", json!({ "sessionId": session })).unwrap();
        assert_eq!(twice, Value::Null);
    }

    #[test]
    fn an_empty_deck_is_done() {
        let (core, _) = core();
        let done = next(&core);
        assert_eq!(done["kind"], "done");
        assert_eq!(done["counts"]["newCount"], 0);
    }

    #[test]
    fn a_learning_card_due_later_today_is_waiting_with_the_seconds_to_go() {
        let (core, _) = core();
        add_cards(&core, &["dom"]);
        core.with_collection(|c| {
            c.set_preset_options(
                default_preset(),
                &PresetChange {
                    learning_steps: Some(vec![60]),
                    ..PresetChange::default()
                },
            )
            .unwrap();
        })
        .unwrap();
        let card = next(&core)["cardId"].as_str().unwrap().to_owned();
        call(
            &core,
            "answerCard",
            json!({ "cardId": card, "rating": "again", "durationMs": 1000 }),
        )
        .unwrap();
        let waiting = next(&core);
        assert_eq!(waiting["kind"], "waiting");
        assert_eq!(waiting["waitSeconds"], 3600);
    }

    #[test]
    fn media_comes_back_as_an_attachment_with_a_type_from_the_extension() {
        let (core, _) = core();
        let name = core
            .with_collection(|c| c.add_media("pies.mp3", b"ID3-bytes").unwrap().name)
            .unwrap();
        let reply = dispatch(
            &core,
            "getMedia",
            json!({ "name": name }),
            &OpContext::uncancellable(),
        )
        .unwrap();
        assert_eq!(reply.output, json!({ "contentType": "audio/mpeg" }));
        assert_eq!(reply.attachment.as_deref(), Some(&b"ID3-bytes"[..]));

        let missing = call(
            &core,
            "getMedia",
            json!({ "name": "nope-0123456789abcdef.png" }),
        );
        assert_eq!(missing.unwrap_err().kind, ErrorKind::NotFound);
    }

    #[test]
    fn content_types_follow_the_extension_in_any_case() {
        assert_eq!(content_type("a.PNG"), "image/png");
        assert_eq!(content_type("a.m4a"), "audio/mp4");
        assert_eq!(content_type("a.unknown"), "application/octet-stream");
        assert_eq!(content_type("noextension"), "application/octet-stream");
    }

    #[test]
    fn a_missing_card_is_not_found() {
        let (core, _) = core();
        let missing = call(
            &core,
            "renderCard",
            json!({ "cardId": Id::from_bytes([9; 16]).to_string() }),
        );
        assert_eq!(missing.unwrap_err().kind, ErrorKind::NotFound);
    }

    #[test]
    fn bad_ids_and_no_collection_are_readable_errors() {
        let (core, _) = core();
        let err = call(&core, "nextCard", json!({ "deckId": "not-an-id" })).unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
        let err = call(
            &core,
            "startStudySession",
            json!({ "deckId": Id::from_bytes([9; 16]).to_string() }),
        )
        .unwrap_err();
        assert_eq!(err.kind, ErrorKind::NotFound);

        let closed = Core::new();
        for (method, input) in [
            ("nextCard", json!({ "deckId": deck() })),
            ("undoAnswer", Value::Null),
            ("getMedia", json!({ "name": "a.png" })),
        ] {
            let err = call(&closed, method, input).unwrap_err();
            assert_eq!(err.kind, ErrorKind::NotFound, "{method}");
        }
    }
}
