//! One card's information (step 3.3): where it is, how it is scheduled and what happened to it.
//!
//! Read only. Everything from the note is plain text: card content is never given to the page as
//! HTML (CLAUDE.md).

use fc_core::Core;
use fc_core::scheduling::{CardState, Due};
use fc_core::stats::{HistoryKind, ReviewEntry};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::edit::{deck_error, note_error, study_error, tag_error, with};
use crate::search::SearchDue;
use crate::study::{StudyCardState, internal, parse_id};
use crate::{ApiError, Method, OpContext};

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct CardInfoInput {
    /// A UUID string.
    pub card_id: String,
}

/// What the card remembers, from FSRS.
#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct CardMemory {
    /// Days until the chance of remembering falls to 90 %.
    pub stability: f32,
    /// 1 to 10.
    pub difficulty: f32,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct CardInfoCard {
    pub state: StudyCardState,
    /// The learning or relearning step, 0 for the first.
    pub step: u32,
    /// Null for a new card.
    pub due: Option<SearchDue>,
    pub memory: Option<CardMemory>,
    /// Study days since the last answer. Null for a card never answered.
    pub days_since_last_answer: Option<u32>,
    /// Answers in the history below.
    pub answers: u32,
    /// Answers that sent a review card to relearning.
    pub lapses: u32,
    pub suspended: bool,
    /// Buried until tomorrow.
    pub buried: bool,
    /// A UUID string.
    pub deck_id: String,
    /// The deck's full name, with `::` between levels.
    pub deck_name: String,
    /// The template name, with the cloze number for a cloze card.
    pub template_name: String,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct CardInfoField {
    pub name: String,
    /// Plain text, never HTML.
    pub text: String,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct CardInfoNote {
    /// UUID strings.
    pub note_id: String,
    pub note_type_id: String,
    pub note_type_name: String,
    pub fields: Vec<CardInfoField>,
    pub tags: Vec<String>,
    /// Live cards the note has.
    pub card_count: u32,
}

/// What an entry of the history is.
#[derive(Debug, Clone, Copy, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum CardInfoEntryKind {
    Answer,
    /// The due date was set by hand.
    SetDue,
    /// The card was reset to new.
    Reset,
    /// The card took the schedule of the card it replaced when its note changed type.
    Transfer,
}

/// An answer's rating.
#[derive(Debug, Clone, Copy, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum CardInfoRating {
    Again,
    Hard,
    Good,
    Easy,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct CardInfoEntry {
    /// A UUID string: the event this entry is.
    pub id: String,
    pub kind: CardInfoEntryKind,
    /// Unix milliseconds.
    #[ts(type = "number")]
    pub time_ms: i64,
    /// The device's UTC offset in minutes when it happened, to show the local time.
    pub utc_offset: i32,
    /// Null unless the entry is an answer.
    pub rating: Option<CardInfoRating>,
    pub state_before: Option<StudyCardState>,
    pub state: Option<StudyCardState>,
    /// Study days since the previous answer. Null for the first answer and for other entries.
    pub days_since_previous: Option<u32>,
    /// When the card was due after this entry, counted from it.
    pub due: Option<CardInfoDue>,
    /// How long the answer took.
    pub duration_ms: u32,
    pub memory: Option<CardMemory>,
}

/// A time after an entry: study days or minutes.
#[derive(Debug, Clone, Copy, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum CardInfoDue {
    Days { days: u32 },
    Minutes { minutes: u32 },
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct CardInfoOutput {
    pub card: CardInfoCard,
    pub note: CardInfoNote,
    /// Oldest first.
    pub history: Vec<CardInfoEntry>,
}

fn state(state: CardState) -> StudyCardState {
    match state {
        CardState::New => StudyCardState::New,
        CardState::Learning => StudyCardState::Learning,
        CardState::Review => StudyCardState::Review,
        CardState::Relearning => StudyCardState::Relearning,
    }
}

fn entry(e: &ReviewEntry) -> CardInfoEntry {
    use fc_core::scheduling::Rating;
    CardInfoEntry {
        id: e.event.to_string(),
        kind: match e.kind {
            HistoryKind::Answer => CardInfoEntryKind::Answer,
            HistoryKind::SetDue => CardInfoEntryKind::SetDue,
            HistoryKind::Reset => CardInfoEntryKind::Reset,
            HistoryKind::Transfer => CardInfoEntryKind::Transfer,
        },
        time_ms: e.time_ms,
        utc_offset: e.utc_offset,
        rating: e.rating.map(|r| match r {
            Rating::Again => CardInfoRating::Again,
            Rating::Hard => CardInfoRating::Hard,
            Rating::Good => CardInfoRating::Good,
            Rating::Easy => CardInfoRating::Easy,
        }),
        state_before: e.state_before.map(state),
        state: e.state.map(state),
        days_since_previous: e.days_since_previous,
        due: e.due.map(|d| match d {
            Due::Days(days) => CardInfoDue::Days { days },
            Due::Minutes(minutes) => CardInfoDue::Minutes { minutes },
        }),
        duration_ms: e.duration_ms,
        memory: e.memory.map(|m| CardMemory {
            stability: m.stability,
            difficulty: m.difficulty,
        }),
    }
}

fn saturate(n: i64) -> i32 {
    i32::try_from(n).unwrap_or(if n < 0 { i32::MIN } else { i32::MAX })
}

/// One live card: its schedule, its note and its history.
pub struct GetCardInfo;

impl Method for GetCardInfo {
    const NAME: &'static str = "getCardInfo";
    type Input = CardInfoInput;
    type Output = CardInfoOutput;

    fn call(core: &Core, input: CardInfoInput, _: &OpContext) -> Result<CardInfoOutput, ApiError> {
        let card = parse_id(&input.card_id)?;
        let what = "read a card's information";
        with(core, |c| {
            let detail = c.card_detail(card).map_err(|e| study_error(what, e))?;
            let schedule = c.card_schedule(card).map_err(|e| study_error(what, e))?;
            let history = c.card_history(card).map_err(|e| study_error(what, e))?;
            let today = c.study_today().map_err(|e| study_error(what, e))?;
            let now = c.now_ms();
            let note = c
                .note(detail.note)
                .map_err(|e| note_error(what, e))?
                .ok_or_else(|| note_error(what, fc_core::note::NoteError::NotFound))?;
            let tags = c.note_tags(detail.note).map_err(|e| tag_error(what, e))?;
            let card_count = c
                .cards_of_note(detail.note)
                .map_err(|e| note_error(what, e))?
                .len();
            let note_type = c
                .note_type(detail.note_type)
                .map_err(|e| internal(what, e))?;
            let deck = c.deck(detail.deck).map_err(|e| deck_error(what, e))?;

            let template_name = note_type
                .as_ref()
                .and_then(|t| t.templates.iter().find(|t| t.id == detail.template))
                .map_or_else(String::new, |t| t.name.clone());
            let due = match schedule.state {
                CardState::New => None,
                CardState::Review => schedule.due_day.map(|day| SearchDue::Day {
                    days: saturate(day - today),
                }),
                CardState::Learning | CardState::Relearning => {
                    schedule.due_ms.map(|ms| SearchDue::Time {
                        minutes: saturate((ms - now + 59_999).div_euclid(60_000)),
                    })
                }
            };
            // Counted from the history, which follows a card through a note type change; the
            // cached counts start again at 0 for the new card (ADR 0014, Consequences).
            let answers = history
                .iter()
                .filter(|e| e.kind == HistoryKind::Answer)
                .count();
            let lapses = history
                .iter()
                .filter(|e| {
                    e.kind == HistoryKind::Answer
                        && e.state_before == Some(CardState::Review)
                        && e.state == Some(CardState::Relearning)
                })
                .count();
            Ok(CardInfoOutput {
                card: CardInfoCard {
                    state: state(schedule.state),
                    step: schedule.step,
                    due,
                    memory: schedule.memory.map(|m| CardMemory {
                        stability: m.stability,
                        difficulty: m.difficulty,
                    }),
                    days_since_last_answer: schedule
                        .last_day
                        .map(|day| fc_core::scheduling::elapsed_days(today, day)),
                    answers: u32::try_from(answers).unwrap_or(u32::MAX),
                    lapses: u32::try_from(lapses).unwrap_or(u32::MAX),
                    suspended: detail.suspended,
                    buried: detail.buried,
                    deck_id: detail.deck.to_string(),
                    deck_name: deck.map_or_else(String::new, |d| d.path),
                    template_name: if detail.ordinal > 0 {
                        format!("{template_name} (cloze {})", detail.ordinal)
                    } else {
                        template_name
                    },
                },
                note: CardInfoNote {
                    note_id: detail.note.to_string(),
                    note_type_id: detail.note_type.to_string(),
                    note_type_name: note_type.map_or_else(String::new, |t| t.name),
                    fields: note
                        .fields
                        .iter()
                        .map(|f| CardInfoField {
                            name: f.name.clone(),
                            text: fc_core::readable_text(&f.value),
                        })
                        .collect(),
                    tags,
                    card_count: u32::try_from(card_count).unwrap_or(u32::MAX),
                },
                history: history.iter().map(entry).collect(),
            })
        })
    }
}
