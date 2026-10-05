//! FSRS scheduling (ADR 0004). A thin layer over the `fsrs` crate with our own types, so the crate
//! does not leak into `fc-api` or the UI. Only what step 0.5 needs: scheduling one card through
//! reviews and optimising parameters from review history. Queues and deck options are Phase 1.

mod machine;

use fsrs::{ComputeParametersInput, DEFAULT_PARAMETERS, FSRS, FSRSItem, FSRSReview};

use crate::id::Id;

pub use machine::{
    CardNow, CardState, Due, MAX_INTERVAL_DAYS, Outcome, Steps, answer, fuzz, preview,
};

/// How well a card was remembered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rating {
    Again,
    Hard,
    Good,
    Easy,
}

impl Rating {
    /// The 1 to 4 scale FSRS uses, and card events store.
    pub(crate) fn number(self) -> u32 {
        match self {
            Rating::Again => 1,
            Rating::Hard => 2,
            Rating::Good => 3,
            Rating::Easy => 4,
        }
    }
}

impl Rating {
    pub(crate) fn from_number(number: i64) -> Option<Self> {
        Some(match number {
            1 => Self::Again,
            2 => Self::Hard,
            3 => Self::Good,
            4 => Self::Easy,
            _ => return None,
        })
    }
}

/// What FSRS remembers about a card between reviews.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Memory {
    pub stability: f32,
    pub difficulty: f32,
}

/// The card after one review.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reviewed {
    pub memory: Memory,
    /// Whole days until the card is due again, at least 1.
    pub interval_days: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SchedulingError {
    /// Desired retention outside 0.7 to 0.99, or an empty or malformed training set.
    InvalidInput,
    /// `fsrs` failed (for example too little history to optimise).
    Fsrs(String),
}

/// One review in a card's history: days since the card's first review, and the rating.
#[derive(Debug, Clone, Copy)]
pub struct HistoryReview {
    pub day: u32,
    pub rating: Rating,
}

pub struct Scheduler {
    fsrs: FSRS,
    desired_retention: f32,
}

impl Scheduler {
    /// `parameters` are the 21 FSRS-6 parameters, or `None` for the defaults.
    pub fn new(
        parameters: Option<&[f32]>,
        desired_retention: f32,
    ) -> Result<Self, SchedulingError> {
        if !(0.7..=0.99).contains(&desired_retention) {
            return Err(SchedulingError::InvalidInput);
        }
        let fsrs = FSRS::new(parameters.unwrap_or(&[]))
            .map_err(|e| SchedulingError::Fsrs(e.to_string()))?;
        Ok(Self {
            fsrs,
            desired_retention,
        })
    }

    /// Outcome of each rating for a card. `elapsed_days` is the time since its last review.
    pub fn next(
        &self,
        memory: Option<Memory>,
        elapsed_days: u32,
    ) -> Result<[Reviewed; 4], SchedulingError> {
        let states = self
            .fsrs
            .next_states(
                memory.map(|m| fsrs::MemoryState {
                    stability: m.stability,
                    difficulty: m.difficulty,
                }),
                self.desired_retention,
                elapsed_days,
            )
            .map_err(|e| SchedulingError::Fsrs(e.to_string()))?;
        let reviewed = |s: fsrs::ItemState| Reviewed {
            memory: Memory {
                stability: s.memory.stability,
                difficulty: s.memory.difficulty,
            },
            interval_days: s.interval.round().max(1.0) as u32,
        };
        Ok([
            reviewed(states.again),
            reviewed(states.hard),
            reviewed(states.good),
            reviewed(states.easy),
        ])
    }

    /// Schedules a new card through `ratings`, reviewing it on each due day.
    pub fn schedule_sequence(&self, ratings: &[Rating]) -> Result<Vec<Reviewed>, SchedulingError> {
        let mut out = Vec::with_capacity(ratings.len());
        let mut memory = None;
        let mut elapsed = 0;
        for rating in ratings {
            let reviewed = self.next(memory, elapsed)?[*rating as usize];
            memory = Some(reviewed.memory);
            elapsed = reviewed.interval_days;
            out.push(reviewed);
        }
        Ok(out)
    }
}

/// The 21 FSRS-6 parameters for a set of 17, 19 or 21 numbers (older FSRS versions have fewer, and
/// `fsrs` fills them the same way it does when it loads them). `None` for any other count or a
/// number that is not finite.
pub fn fill_parameters(parameters: &[f32]) -> Option<Vec<f32>> {
    fsrs::check_and_fill_parameters(parameters).ok()
}

/// The namespace of a parameter set's ID (ADR 0007, part 6).
const PARAMETERS_NAMESPACE: Id = Id::from_bytes(*b"fc-fsrs-params-1");

/// The 21 values as little-endian `f32` bytes, the form a parameter set is stored and named by.
pub(crate) fn parameter_bytes(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// The ID of a parameter set: `UUIDv5("fc-fsrs-parameters-1", the values as little-endian f32)`,
/// so every device gives the same ID to the same values and the defaults need no seeding.
pub(crate) fn parameter_set_id(values: &[f32]) -> Id {
    Id::new_v5(PARAMETERS_NAMESPACE, &parameter_bytes(values))
}

/// The study day a moment belongs to, counted from 1970-01-01 (ADR 0007, part 2). The day starts
/// at `start_hour` (0 to 23) local time, where local time is UTC plus `utc_offset_minutes`.
pub fn study_day(unix_ms: i64, utc_offset_minutes: i32, start_hour: u8) -> i64 {
    (unix_ms + i64::from(utc_offset_minutes) * 60_000 - i64::from(start_hour) * 3_600_000)
        .div_euclid(86_400_000)
}

/// Whole study days from `last` to `today`, never negative: after travelling west "today" can be
/// earlier than the day of the last answer, and then it counts as the same day.
pub fn elapsed_days(today: i64, last: i64) -> u32 {
    today.saturating_sub(last).clamp(0, i64::from(u32::MAX)) as u32
}

/// The default FSRS-6 parameters.
pub fn default_parameters() -> &'static [f32] {
    &DEFAULT_PARAMETERS
}

/// Optimises parameters from review histories. Runs for as long as training takes and cannot be
/// cancelled, so hosts should call it where blocking is acceptable.
pub fn optimise(histories: &[Vec<HistoryReview>]) -> Result<Vec<f32>, SchedulingError> {
    // One training item per review after the first, holding every review up to it.
    let mut train_set = Vec::new();
    for history in histories {
        if history.is_empty() {
            return Err(SchedulingError::InvalidInput);
        }
        let mut reviews = Vec::new();
        let mut previous_day = history[0].day;
        for (i, review) in history.iter().enumerate() {
            reviews.push(FSRSReview {
                rating: review.rating.number(),
                delta_t: review.day.saturating_sub(previous_day),
            });
            previous_day = review.day;
            if i > 0 {
                train_set.push(FSRSItem {
                    reviews: reviews.clone(),
                });
            }
        }
    }
    fsrs::compute_parameters(ComputeParametersInput {
        train_set,
        ..Default::default()
    })
    .map_err(|e| SchedulingError::Fsrs(e.to_string()))
}

/// Deterministic made-up review histories (about 8 reviews per card, mostly Good) for the 0.5 spike,
/// so the optimiser can be run on every target without real data. Deleted with the spike.
pub fn synthetic_histories(cards: u32) -> Vec<Vec<HistoryReview>> {
    let mut seed: u32 = 12345;
    let mut next = move || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        seed >> 16
    };
    (0..cards)
        .map(|_| {
            let mut day = 0;
            let mut gap = 1;
            (0..8)
                .map(|_| {
                    let rating = match next() % 10 {
                        0 => Rating::Again,
                        1 | 2 => Rating::Hard,
                        3..=7 => Rating::Good,
                        _ => Rating::Easy,
                    };
                    let review = HistoryReview { day, rating };
                    gap = match rating {
                        Rating::Again => 1,
                        Rating::Hard => gap + gap / 2 + 1,
                        Rating::Good => gap * 2 + 1,
                        Rating::Easy => gap * 3 + 2,
                    };
                    day += gap;
                    review
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD5: [Rating; 5] = [Rating::Good; 5];

    fn intervals(ratings: &[Rating]) -> Vec<u32> {
        let scheduler = Scheduler::new(None, 0.9).unwrap();
        scheduler
            .schedule_sequence(ratings)
            .unwrap()
            .iter()
            .map(|r| r.interval_days)
            .collect()
    }

    #[test]
    fn five_good_reviews_give_growing_intervals() {
        assert_eq!(intervals(&GOOD5), [2, 11, 46, 163, 497]);
    }

    /// Checks the second review against the published FSRS-6 formulas, written out here without
    /// the crate: a Good recall after 2 days.
    #[test]
    fn second_review_matches_the_published_formulas() {
        let w = default_parameters();
        let scheduler = Scheduler::new(None, 0.9).unwrap();
        let steps = scheduler.schedule_sequence(&GOOD5[..2]).unwrap();

        // First review: S0 = w[2] for Good, D0 = w4 - e^(w5 * (G - 1)) + 1.
        let s0 = w[2];
        let d0 = w[4] - (w[5] * 2.0).exp() + 1.0;
        assert!((steps[0].memory.stability - s0).abs() < 1e-3);
        assert!((steps[0].memory.difficulty - d0).abs() < 1e-3);
        assert_eq!(steps[0].interval_days, 2);

        // Second review after t = 2 days. Decay = -w20, factor = 0.9^(1/decay) - 1.
        let decay = -w[20];
        let factor = 0.9_f32.powf(1.0 / decay) - 1.0;
        let t = 2.0;
        let r = (1.0 + factor * t / s0).powf(decay);
        let s1 = s0
            * (1.0 + w[8].exp() * (11.0 - d0) * s0.powf(-w[9]) * (((1.0 - r) * w[10]).exp() - 1.0));
        assert!(
            (steps[1].memory.stability - s1).abs() < 1e-2,
            "{} vs {s1}",
            steps[1].memory.stability
        );
    }

    #[test]
    fn a_lapse_shortens_the_interval() {
        let after_good = intervals(&[Rating::Good, Rating::Good, Rating::Good]);
        let after_lapse = intervals(&[Rating::Good, Rating::Good, Rating::Again]);
        assert_eq!(after_good[..2], after_lapse[..2]);
        assert!(after_lapse[2] < after_good[2]);
    }

    #[test]
    fn easy_is_longer_than_good_is_longer_than_hard() {
        let [again, hard, good, easy] = Scheduler::new(None, 0.9).unwrap().next(None, 0).unwrap();
        assert!(again.interval_days <= hard.interval_days);
        assert!(hard.interval_days <= good.interval_days);
        assert!(good.interval_days < easy.interval_days);
    }

    #[test]
    fn higher_desired_retention_gives_shorter_intervals() {
        let at = |retention| {
            Scheduler::new(None, retention)
                .unwrap()
                .schedule_sequence(&GOOD5)
                .unwrap()[4]
                .interval_days
        };
        assert!(at(0.95) < at(0.9));
        assert!(at(0.9) < at(0.8));
    }

    #[test]
    fn retention_outside_the_allowed_range_is_rejected() {
        assert_eq!(
            Scheduler::new(None, 0.5).err(),
            Some(SchedulingError::InvalidInput)
        );
        assert_eq!(
            Scheduler::new(None, 1.0).err(),
            Some(SchedulingError::InvalidInput)
        );
    }

    #[test]
    fn optimising_returns_21_parameters() {
        let histories = synthetic_histories(200);
        let params = optimise(&histories).unwrap();
        assert_eq!(params.len(), 21);
        assert!(params.iter().all(|p| p.is_finite()));
        // The optimised parameters are usable by the scheduler.
        Scheduler::new(Some(&params), 0.9)
            .unwrap()
            .schedule_sequence(&GOOD5)
            .unwrap();
    }

    #[test]
    fn an_empty_history_is_rejected() {
        assert_eq!(
            optimise(&[vec![]]).err(),
            Some(SchedulingError::InvalidInput)
        );
    }
}

#[cfg(test)]
mod day_tests {
    use super::*;

    const HOUR: i64 = 3_600_000;
    const DAY: i64 = 24 * HOUR;

    #[test]
    fn the_day_number_counts_from_1970_01_01() {
        assert_eq!(study_day(0, 0, 0), 0);
        assert_eq!(study_day(DAY - 1, 0, 0), 0);
        assert_eq!(study_day(DAY, 0, 0), 1);
        // 2023-11-14 22:13:20 UTC.
        assert_eq!(study_day(1_700_000_000_000, 0, 0), 19_675);
        // Before 1970 the day numbers go negative and still step once a day.
        assert_eq!(study_day(-1, 0, 0), -1);
        assert_eq!(study_day(-DAY, 0, 0), -1);
        assert_eq!(study_day(-DAY - 1, 0, 0), -2);
    }

    #[test]
    fn the_day_starts_at_the_chosen_hour() {
        for hour in [0u8, 4, 23] {
            let start = DAY + i64::from(hour) * HOUR;
            assert_eq!(study_day(start - 1, 0, hour), 0, "hour {hour}");
            assert_eq!(study_day(start, 0, hour), 1, "hour {hour}");
            assert_eq!(study_day(start + DAY - 1, 0, hour), 1, "hour {hour}");
            assert_eq!(study_day(start + DAY, 0, hour), 2, "hour {hour}");
        }
    }

    #[test]
    fn the_offset_moves_the_boundary_to_local_midnight() {
        // New Zealand summer time, UTC+13 (780), and the far ends: UTC-12 and UTC+14.
        for offset in [-720, 0, 60, 780, 840] {
            let midnight_utc = DAY - i64::from(offset) * 60_000;
            assert_eq!(study_day(midnight_utc - 1, offset, 0), 0, "offset {offset}");
            assert_eq!(study_day(midnight_utc, offset, 0), 1, "offset {offset}");
        }
        // 10:00 UTC on 1970-01-01 is already 1970-01-02 at UTC+14.
        assert_eq!(study_day(10 * HOUR, 840, 0), 1);
        // 12:00 UTC is still the previous day at UTC-12.
        assert_eq!(study_day(11 * HOUR, -720, 0), -1);
        assert_eq!(study_day(12 * HOUR, -720, 0), 0);
    }

    #[test]
    fn offset_and_start_hour_work_together() {
        // Day starts at 23:00 local at UTC-12 (-720): 23:00 local on 1970-01-01 is 11:00 UTC on
        // 1970-01-02, and the day that begins then is day 0 (the one before it is day -1).
        let start = DAY + 11 * HOUR;
        assert_eq!(study_day(start - 1, -720, 23), -1);
        assert_eq!(study_day(start, -720, 23), 0);
        assert_eq!(study_day(start + DAY, -720, 23), 1);
        // Day starts at 04:00 local at UTC+14 (840): 04:00 local is 14:00 UTC the day before.
        let start = DAY - 10 * HOUR;
        assert_eq!(study_day(start - 1, 840, 4), 0);
        assert_eq!(study_day(start, 840, 4), 1);
    }

    #[test]
    fn a_dst_change_makes_one_day_longer_or_shorter_and_nothing_else() {
        // Clocks go forward: the offset moves from +60 to +120 at 01:00 UTC. A moment just before,
        // at 00:59 UTC (01:59 local), and one just after, at 01:00 UTC (03:00 local), are the same
        // study day; the next midnight local (22:00 UTC at +120) starts the next one.
        let change = 5 * DAY + HOUR;
        let day = study_day(change - 60_000, 60, 0);
        assert_eq!(study_day(change, 120, 0), day);
        let next_midnight = 5 * DAY + 22 * HOUR;
        assert_eq!(study_day(next_midnight - 1, 120, 0), day);
        assert_eq!(study_day(next_midnight, 120, 0), day + 1);
    }

    #[test]
    fn elapsed_days_never_go_below_zero() {
        assert_eq!(elapsed_days(10, 7), 3);
        assert_eq!(elapsed_days(7, 7), 0);
        // Travelling west: today is earlier than the day of the last answer.
        assert_eq!(elapsed_days(6, 7), 0);
        assert_eq!(elapsed_days(i64::MIN, i64::MAX), 0);
        assert_eq!(elapsed_days(i64::MAX, i64::MIN), u32::MAX);
    }

    #[test]
    fn a_parameter_set_has_the_same_id_for_the_same_values_on_every_device() {
        let defaults = default_parameters();
        assert_eq!(
            parameter_set_id(defaults),
            parameter_set_id(&defaults.to_vec())
        );
        let mut changed = defaults.to_vec();
        changed[0] += 0.001;
        assert_ne!(parameter_set_id(defaults), parameter_set_id(&changed));
        assert_eq!(parameter_bytes(defaults).len(), 84);
        // Pinned: the ID is part of the sync format, so it can never change. Worked out
        // separately (SHA-1 of the namespace and the 84 bytes, outside this crate).
        assert_eq!(
            parameter_set_id(defaults).to_string(),
            "77e38a6d-65b2-5412-9163-cb372a98777b"
        );
    }
}
