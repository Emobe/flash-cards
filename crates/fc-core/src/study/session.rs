//! Study sessions (ADR 0009, part 5). A session is held in memory by the open collection: at most
//! one at a time, with only its ID, deck and start time. Nothing is stored, so a session lost to an
//! app kill is simply never ended. Its summary is counted from the review log when it ends.

use super::StudyError;
use crate::collection::Collection;
use crate::events::Event;
use crate::id::Id;
use crate::scheduling::Rating;
use crate::sync::state;

/// The open session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OpenSession {
    pub id: Id,
    pub deck: Option<Id>,
    pub started_ms: i64,
}

/// Why a session ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    /// `end_study_session` was called.
    Ended,
    /// Another session started.
    Replaced,
    /// The collection was closed.
    Closed,
}

/// What a session did: the answers made on this device since it started that are not undone,
/// whatever their deck.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionSummary {
    pub answered: u32,
    pub again: u32,
    /// The answers' stored durations added up.
    pub studied_ms: u32,
    /// Wall time from the start to the end.
    pub elapsed_ms: u32,
}

impl Collection {
    /// Starts a study session on `deck`, or on the whole collection with `None`, and returns its
    /// ID. A session that was open ends with `Replaced`. Refused with `NotFound` if the deck is
    /// missing or deleted.
    pub fn start_study_session(&self, deck: Option<Id>) -> Result<Id, StudyError> {
        if let Some(deck) = deck
            && self.deck(deck)?.is_none_or(|found| found.deleted)
        {
            return Err(StudyError::NotFound);
        }
        self.end_session(EndReason::Replaced)?;
        let id = state::new_id(&self.host)?;
        *self.session.borrow_mut() = Some(OpenSession {
            id,
            deck,
            started_ms: self.host.clock.now().unix_ms,
        });
        self.emit_now(Event::StudySessionStarted { session: id, deck });
        Ok(id)
    }

    /// Ends the session `id` and returns its summary, or `None` (and emits nothing) if that session
    /// is not open.
    pub fn end_study_session(&self, id: Id) -> Result<Option<SessionSummary>, StudyError> {
        if self.open_session_id() != Some(id) {
            return Ok(None);
        }
        self.end_session(EndReason::Ended)
    }

    /// The open session's ID, if any.
    pub(crate) fn open_session_id(&self) -> Option<Id> {
        self.session.borrow().map(|s| s.id)
    }

    /// Ends the open session, if any, for a reason that cannot report a failure (closing). If the
    /// summary cannot be read, the session ends without an event.
    pub(crate) fn end_open_session(&self, reason: EndReason) {
        if self.end_session(reason).is_err() {
            self.session.borrow_mut().take();
        }
    }

    fn end_session(&self, reason: EndReason) -> Result<Option<SessionSummary>, StudyError> {
        let Some(open) = *self.session.borrow() else {
            return Ok(None);
        };
        let summary = self.summary(&open)?;
        self.session.borrow_mut().take();
        self.emit_now(Event::StudySessionEnded {
            session: open.id,
            deck: open.deck,
            reason,
            summary,
        });
        Ok(Some(summary))
    }

    fn summary(&self, open: &OpenSession) -> Result<SessionSummary, StudyError> {
        let device = state::device_id(&self.conn)?;
        let (answered, again, studied): (i64, i64, i64) = self
            .conn
            .query_row(
                "SELECT COUNT(*),
                        COALESCE(SUM(e.rating = ?3), 0),
                        COALESCE(SUM(e.duration_ms), 0)
                 FROM card_event e
                 WHERE e.kind = 'review' AND e.device = ?1 AND e.time_ms >= ?2
                   AND NOT EXISTS (SELECT 1 FROM card_event v WHERE v.kind = 'void' AND v.target = e.id)",
                rusqlite::params![device, open.started_ms, Rating::Again.number()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(crate::collection::CollectionError::from)?;
        let elapsed = self
            .host
            .clock
            .now()
            .unix_ms
            .saturating_sub(open.started_ms);
        let small = |n: i64| u32::try_from(n.max(0)).unwrap_or(u32::MAX);
        Ok(SessionSummary {
            answered: small(answered),
            again: small(again),
            studied_ms: small(studied),
            elapsed_ms: small(elapsed),
        })
    }
}
