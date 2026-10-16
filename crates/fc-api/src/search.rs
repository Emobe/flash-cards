//! Search and saved searches for the Browse screen (step 3.1a, ADR 0013 decision 8). Thin
//! wrappers over `fc_core::search`.

use fc_core::Core;
use fc_core::collection::Collection;
use fc_core::scheduling::CardState;
use fc_core::search::{
    self, Mode, SavedSearch, SavedSearchChange, SearchError, SearchOptions, SearchRow, Sort,
    SortKey,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::collection::not_open;
use crate::study::{StudyCardState, internal, parse_id};
use crate::{ApiError, ErrorKind, Method, OpContext};

fn search_error(what: &str, error: SearchError) -> ApiError {
    match error {
        SearchError::NotFound => ApiError::new(ErrorKind::NotFound, error.to_string()),
        SearchError::Syntax { .. }
        | SearchError::UnknownField { .. }
        | SearchError::EmptyName
        | SearchError::NameTaken(_)
        | SearchError::UnknownSort(_) => ApiError::invalid_input(error.to_string()),
        SearchError::Collection(inner) => match ApiError::from(inner) {
            api if api == ApiError::internal() => internal(what, &api),
            api => api,
        },
    }
}

/// Runs `f` on the open collection.
fn with<T>(
    core: &Core,
    what: &str,
    f: impl FnOnce(&Collection) -> Result<T, SearchError>,
) -> Result<T, ApiError> {
    core.with_collection(f)
        .ok_or_else(|| not_open(core))?
        .map_err(|error| search_error(what, error))
}

/// What to sort by: the core's ten sorts, by the names the CLI uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum SearchSortKey {
    /// When the note was added.
    Created,
    Due,
    /// The note's sort field, alphabetically.
    Field,
    /// The deck's place in the deck list.
    Deck,
    /// The note type's name.
    Note,
    Difficulty,
    Stability,
    Lapses,
    Reviews,
    /// A shuffle that stays the same for the same seed.
    Random,
}

impl SearchSortKey {
    fn core(self) -> SortKey {
        match self {
            Self::Created => SortKey::Created,
            Self::Due => SortKey::Due,
            Self::Field => SortKey::SortField,
            Self::Deck => SortKey::Deck,
            Self::Note => SortKey::NoteType,
            Self::Difficulty => SortKey::Difficulty,
            Self::Stability => SortKey::Stability,
            Self::Lapses => SortKey::Lapses,
            Self::Reviews => SortKey::Reviews,
            Self::Random => SortKey::Random,
        }
    }

    fn from_core(key: SortKey) -> Self {
        match key {
            SortKey::Created => Self::Created,
            SortKey::Due => Self::Due,
            SortKey::SortField => Self::Field,
            SortKey::Deck => Self::Deck,
            SortKey::NoteType => Self::Note,
            SortKey::Difficulty => Self::Difficulty,
            SortKey::Stability => Self::Stability,
            SortKey::Lapses => Self::Lapses,
            SortKey::Reviews => Self::Reviews,
            SortKey::Random => Self::Random,
        }
    }
}

/// A sort. Cards with nothing to sort by (a new card has no due date) come last either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SearchSort {
    pub key: SearchSortKey,
    pub descending: bool,
}

impl SearchSort {
    fn core(self) -> Sort {
        Sort {
            key: self.key.core(),
            descending: self.descending,
        }
    }

    fn from_core(sort: Sort) -> Self {
        Self {
            key: SearchSortKey::from_core(sort.key),
            descending: sort.descending,
        }
    }
}

/// Whether results are cards, or one row per note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum SearchMode {
    Notes,
    Cards,
}

impl SearchMode {
    fn core(self) -> Mode {
        match self {
            Self::Notes => Mode::Notes,
            Self::Cards => Mode::Cards,
        }
    }

    fn from_core(mode: Mode) -> Self {
        match mode {
            Mode::Notes => Self::Notes,
            Mode::Cards => Self::Cards,
        }
    }
}

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SearchInput {
    /// The query text (the grammar is in `fc_core::search`).
    pub query: String,
    pub sort: SearchSort,
    pub mode: SearchMode,
    pub offset: u32,
    /// At most 1,000. 0 only counts.
    pub limit: u32,
    /// For the shuffle sort: the same seed gives the same order, so pages agree.
    pub seed: u32,
}

/// When a card is due.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SearchDue {
    /// A review card: study days from today (0 today, negative overdue).
    Day { days: i32 },
    /// A learning or relearning card: minutes from now, rounded up (negative when overdue).
    Time { minutes: i32 },
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SearchResultRow {
    /// UUID strings.
    pub card_id: String,
    pub note_id: String,
    pub note_type_id: String,
    pub deck_id: String,
    /// The note's sort field as plain text (never HTML).
    pub title: String,
    /// The next field that has text, as plain text, or empty.
    pub detail: String,
    pub state: StudyCardState,
    /// Null for a new card.
    pub due: Option<SearchDue>,
    pub suspended: bool,
    /// Buried until tomorrow.
    pub buried: bool,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SearchResults {
    /// How many cards or notes the whole search found.
    pub total: u32,
    pub rows: Vec<SearchResultRow>,
}

fn saturate(n: i64) -> i32 {
    i32::try_from(n).unwrap_or(if n < 0 { i32::MIN } else { i32::MAX })
}

fn row(row: SearchRow, today: i64, now_ms: i64) -> SearchResultRow {
    let due = match row.state {
        CardState::New => None,
        CardState::Review => row.due_day.map(|day| SearchDue::Day {
            days: saturate(day - today),
        }),
        CardState::Learning | CardState::Relearning => row.due_ms.map(|ms| SearchDue::Time {
            // Rounded up, so "in 1 minute" is never shown for a card due now.
            minutes: saturate((ms - now_ms + 59_999).div_euclid(60_000)),
        }),
    };
    SearchResultRow {
        card_id: row.card.to_string(),
        note_id: row.note.to_string(),
        note_type_id: row.note_type.to_string(),
        deck_id: row.deck.to_string(),
        title: row.title,
        detail: row.detail,
        state: match row.state {
            CardState::New => StudyCardState::New,
            CardState::Learning => StudyCardState::Learning,
            CardState::Review => StudyCardState::Review,
            CardState::Relearning => StudyCardState::Relearning,
        },
        due,
        suspended: row.suspended,
        buried: row.buried,
    }
}

/// One page of a search.
pub struct Search;

impl Method for Search {
    const NAME: &'static str = "search";
    type Input = SearchInput;
    type Output = SearchResults;

    fn call(core: &Core, input: SearchInput, _: &OpContext) -> Result<SearchResults, ApiError> {
        let options = SearchOptions {
            sort: input.sort.core(),
            mode: input.mode.core(),
            offset: input.offset,
            limit: input.limit,
            seed: u64::from(input.seed),
        };
        let (page, today, now) = with(core, "search", |c| {
            let page = c.search(&input.query, &options)?;
            let today = c
                .study_today()
                .map_err(|e| SearchError::Collection(storage(e)))?;
            Ok((page, today, c.now_ms()))
        })?;
        Ok(SearchResults {
            total: u32::try_from(page.total).unwrap_or(u32::MAX),
            rows: page.rows.into_iter().map(|r| row(r, today, now)).collect(),
        })
    }
}

fn storage(error: impl std::fmt::Display) -> fc_core::collection::CollectionError {
    fc_core::collection::CollectionError::Storage(error.to_string())
}

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct CheckQueryInput {
    pub query: String,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct QueryProblem {
    /// What is wrong and what to do, without the position.
    pub message: String,
    /// The character where the problem is, counted from 0.
    pub position: u32,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct QueryCheck {
    /// Null when the query reads.
    pub problem: Option<QueryProblem>,
}

/// Whether a query can be read, without running it, so a screen can check as people type. An
/// unknown field name is only found when the search runs.
pub struct CheckQuery;

impl Method for CheckQuery {
    const NAME: &'static str = "checkQuery";
    type Input = CheckQueryInput;
    type Output = QueryCheck;

    fn call(_: &Core, input: CheckQueryInput, _: &OpContext) -> Result<QueryCheck, ApiError> {
        Ok(QueryCheck {
            problem: match search::check_query(&input.query) {
                Ok(()) => None,
                Err(SearchError::Syntax { position, problem }) => Some(QueryProblem {
                    message: problem,
                    position: u32::try_from(position).unwrap_or(u32::MAX),
                }),
                Err(other) => return Err(search_error("check a query", other)),
            },
        })
    }
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SavedSearchSummary {
    /// A UUID string.
    pub id: String,
    pub name: String,
    pub query: String,
    pub sort: SearchSort,
    pub mode: SearchMode,
}

fn saved(search: SavedSearch) -> SavedSearchSummary {
    SavedSearchSummary {
        id: search.id.to_string(),
        name: search.name,
        query: search.query,
        sort: SearchSort::from_core(search.sort),
        mode: SearchMode::from_core(search.mode),
    }
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SavedSearchList {
    /// By name, ignoring case.
    pub searches: Vec<SavedSearchSummary>,
}

pub struct GetSavedSearches;

impl Method for GetSavedSearches {
    const NAME: &'static str = "getSavedSearches";
    type Input = ();
    type Output = SavedSearchList;

    fn call(core: &Core, (): (), _: &OpContext) -> Result<SavedSearchList, ApiError> {
        let searches = with(core, "read the saved searches", Collection::saved_searches)?;
        Ok(SavedSearchList {
            searches: searches.into_iter().map(saved).collect(),
        })
    }
}

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct NewSavedSearchInput {
    pub name: String,
    pub query: String,
    pub sort: SearchSort,
    pub mode: SearchMode,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct NewSavedSearch {
    /// A UUID string.
    pub id: String,
}

/// Saves a search under a name no other saved search has. The query must read.
pub struct SaveSearch;

impl Method for SaveSearch {
    const NAME: &'static str = "saveSearch";
    type Input = NewSavedSearchInput;
    type Output = NewSavedSearch;

    fn call(
        core: &Core,
        input: NewSavedSearchInput,
        _: &OpContext,
    ) -> Result<NewSavedSearch, ApiError> {
        let id = with(core, "save the search", |c| {
            c.save_search(
                &input.name,
                &input.query,
                input.sort.core(),
                input.mode.core(),
            )
        })?;
        Ok(NewSavedSearch { id: id.to_string() })
    }
}

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct UpdateSavedSearchInput {
    pub id: String,
    /// Each that is null stays as it is.
    pub name: Option<String>,
    pub query: Option<String>,
    pub sort: Option<SearchSort>,
    pub mode: Option<SearchMode>,
}

pub struct UpdateSavedSearch;

impl Method for UpdateSavedSearch {
    const NAME: &'static str = "updateSavedSearch";
    type Input = UpdateSavedSearchInput;
    type Output = ();

    fn call(core: &Core, input: UpdateSavedSearchInput, _: &OpContext) -> Result<(), ApiError> {
        let id = parse_id(&input.id)?;
        let change = SavedSearchChange {
            name: input.name,
            query: input.query,
            sort: input.sort.map(SearchSort::core),
            mode: input.mode.map(SearchMode::core),
        };
        with(core, "change the saved search", |c| {
            c.update_saved_search(id, &change)
        })
    }
}

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SavedSearchIdInput {
    pub id: String,
}

pub struct DeleteSavedSearch;

impl Method for DeleteSavedSearch {
    const NAME: &'static str = "deleteSavedSearch";
    type Input = SavedSearchIdInput;
    type Output = ();

    fn call(core: &Core, input: SavedSearchIdInput, _: &OpContext) -> Result<(), ApiError> {
        let id = parse_id(&input.id)?;
        with(core, "delete the saved search", |c| {
            c.delete_saved_search(id)
        })
    }
}
