//! The due forecast.

use rusqlite::params;

use super::{DeckFilter, StudyError};
use crate::collection::Collection;
use crate::id::Id;

/// Review cards due on one study day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForecastDay {
    pub day: i64,
    pub reviews: u32,
}

/// What is due, from the schedule as it is now. It shows what will be due, not what the queue will
/// offer: daily limits are ignored, new cards are not in it, and a card that comes back again inside
/// the window after being answered is not predicted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DueForecast {
    pub today: i64,
    /// Review cards that were due before today.
    pub overdue: u32,
    /// Learning and relearning cards due before the end of today.
    pub learning_today: u32,
    /// Today and each of the next `days` days, in order (`days + 1` entries). Today's entry is the
    /// review cards due today, not the overdue ones.
    pub days: Vec<ForecastDay>,
}

impl Collection {
    /// Review cards due today and on each of the next `days` study days, plus the overdue ones.
    /// Suspended cards and cards that are deleted are left out. A buried card is counted on the day
    /// it is due, since it comes back by then. With a deck, only that deck and what is inside it.
    pub fn due_forecast(&self, days: u32, deck: Option<Id>) -> Result<DueForecast, StudyError> {
        let filter = deck.map(|d| DeckFilter::load(self, d)).transpose()?;
        let now = self.host.clock.now();
        let start_hour = self.day_start_hour()?;
        let today = self.today()?;
        let day_end_ms = (today + 1) * 86_400_000 + i64::from(start_hour) * 3_600_000
            - i64::from(now.utc_offset_minutes) * 60_000;
        let last = today + i64::from(days);
        let mut forecast = DueForecast {
            today,
            overdue: 0,
            learning_today: 0,
            days: (today..=last)
                .map(|day| ForecastDay { day, reviews: 0 })
                .collect(),
        };
        let mut statement = self.conn.prepare(
            "SELECT k.deck, s.state, COALESCE(s.due_day, ?1), COUNT(*)
             FROM card_schedule s
             JOIN card k ON k.id = s.card
             JOIN note n ON n.id = k.note
             WHERE k.deleted = 0 AND n.deleted = 0 AND k.suspended = 0
               AND ((s.state = 2 AND s.due_day <= ?2)
                 OR (s.state IN (1, 3) AND COALESCE(s.due_ms, 0) < ?3))
             GROUP BY k.deck, s.state, s.due_day",
        )?;
        let rows = statement.query_map(params![today, last, day_end_ms], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, u32>(3)?,
            ))
        })?;
        for row in rows {
            let (stored, state, due_day, count) = row?;
            if filter.as_ref().is_some_and(|f| !f.contains(&stored)) {
                continue;
            }
            if state == 2 {
                if due_day < today {
                    forecast.overdue += count;
                } else {
                    forecast.days[(due_day - today) as usize].reviews += count;
                }
            } else {
                forecast.learning_today += count;
            }
        }
        Ok(forecast)
    }
}
