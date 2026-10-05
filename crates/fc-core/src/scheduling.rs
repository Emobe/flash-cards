//! FSRS scheduling (ADR 0004). A thin layer over the `fsrs` crate with our own types, so the crate
//! does not leak into `fc-api` or the UI. Only what step 0.5 needs: scheduling one card through
//! reviews and optimising parameters from review history. Queues and deck options are Phase 1.

use fsrs::{ComputeParametersInput, DEFAULT_PARAMETERS, FSRS, FSRSItem, FSRSReview};

/// How well a card was remembered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rating {
    Again,
    Hard,
    Good,
    Easy,
}

impl Rating {
    /// The 1 to 4 scale FSRS uses.
    fn number(self) -> u32 {
        match self {
            Rating::Again => 1,
            Rating::Hard => 2,
            Rating::Good => 3,
            Rating::Easy => 4,
        }
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
