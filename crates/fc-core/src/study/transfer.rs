//! Carrying a schedule onto another card (ADR 0014, decision 6): the `transfer` event a card made
//! by a note type change starts with, and the reset a card gets when the person chose "Start as
//! new". Both record their full result like an answer, so the fold takes them as they are. The
//! writes run inside the caller's write, so a type change and its schedules commit together.

use super::CARD_EVENT;
use super::event::{CardEvent, EventKind, event};
use super::schedule::{Applied, Cached, after_change, read_cached, write_cached};
use crate::collection::CollectionError;
use crate::id::Id;
use crate::scheduling::{CardState, study_day};
use crate::sync::WriteTx;

/// What a card gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Carry {
    /// The schedule of this card.
    Schedule(Id),
    /// Back to new, if it has a schedule.
    Reset,
}

/// What the write needs of the moment and the device, read before it starts.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Stamp {
    pub unix_ms: i64,
    pub utc_offset: i32,
    pub start_hour: u8,
    pub device: Id,
}

/// What `write_carries` did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Carried {
    /// `(card, source)` for each schedule that was carried.
    pub transferred: Vec<(Id, Id)>,
    /// Cards that were reset.
    pub reset: Vec<Id>,
}

/// Whether two cache rows are the same schedule, whatever events made them.
fn same_schedule(a: &Cached, b: &Cached) -> bool {
    (a.state, a.step, a.memory, a.due_day, a.due_ms, a.last_day)
        == (b.state, b.step, b.memory, b.due_day, b.due_ms, b.last_day)
}

/// Writes one event per card in `carries`, each built on the card's last event, and updates the
/// cache. A card is left alone when there is nothing to change: the source has no schedule, the
/// card already has that schedule (an undo with nothing studied in between), or it is new and is to
/// be reset.
pub(crate) fn write_carries(
    w: &mut WriteTx<'_>,
    stamp: &Stamp,
    carries: &[(Id, Carry)],
) -> Result<Carried, CollectionError> {
    let mut done = Carried::default();
    for (card, carry) in carries {
        let target = read_cached(w.local(), *card)?;
        // After the last event of the card it takes the schedule from, so the history reads in order.
        let mut not_before = i64::MIN;
        let (kind, source, applied, last_day) = match carry {
            Carry::Schedule(source) => {
                let Some(from) = read_cached(w.local(), *source)? else {
                    continue;
                };
                if target.as_ref().is_some_and(|t| same_schedule(t, &from)) {
                    continue;
                }
                not_before = event(w.local(), from.last_event)?.map_or(i64::MIN, |e| e.time_ms);
                let applied = Applied {
                    state_before: target.as_ref().map_or(CardState::New, |t| t.state),
                    state: from.state,
                    step: from.step,
                    memory: from.memory,
                    due_day: from.due_day,
                    due_ms: from.due_ms,
                };
                (EventKind::Transfer, Some(*source), applied, from.last_day)
            }
            Carry::Reset => {
                let Some(t) = target.as_ref().filter(|t| t.state != CardState::New) else {
                    continue;
                };
                let applied = Applied {
                    state_before: t.state,
                    state: CardState::New,
                    step: 0,
                    memory: None,
                    due_day: None,
                    due_ms: None,
                };
                (EventKind::Reset, None, applied, t.last_day)
            }
        };
        // Never earlier than the event it builds on, as for an answer.
        if let Some(t) = &target {
            let previous = event(w.local(), t.last_event)?.map_or(i64::MIN, |e| e.time_ms);
            not_before = not_before.max(previous);
        }
        let time_ms = stamp.unix_ms.max(not_before.saturating_add(1));
        let id = w.new_id()?;
        let written = CardEvent {
            id,
            card: *card,
            kind,
            time_ms,
            utc_offset: stamp.utc_offset,
            device: stamp.device,
            previous: target.as_ref().map(|t| t.last_event),
            day: study_day(time_ms, stamp.utc_offset, stamp.start_hour),
            rating: None,
            duration_ms: None,
            preset: None,
            desired_retention: None,
            parameters: None,
            steps: None,
            state_before: Some(applied.state_before),
            state: Some(applied.state),
            step: Some(applied.step),
            memory: applied.memory,
            due_day: applied.due_day,
            due_ms: applied.due_ms,
            target: None,
            source,
            last_day: Some(last_day),
        };
        w.insert_row(CARD_EVENT.entity, id, written.values())?;
        let row = after_change(target.as_ref(), id, last_day, &applied);
        write_cached(w.local(), *card, &row)?;
        match source {
            Some(source) => done.transferred.push((*card, source)),
            None => done.reset.push(*card),
        }
    }
    Ok(done)
}
