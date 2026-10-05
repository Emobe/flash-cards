//! Temporary methods for the scheduling spike (step 0.5). Deleted when real scheduling methods
//! arrive (Phase 1).

use fc_core::Core;
use fc_core::scheduling::{self, Rating, Scheduler, SchedulingError};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{ApiError, Method, OpContext};

#[derive(Debug, Clone, Copy, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum SpikeRating {
    Again,
    Hard,
    Good,
    Easy,
}

impl From<SpikeRating> for Rating {
    fn from(rating: SpikeRating) -> Self {
        match rating {
            SpikeRating::Again => Rating::Again,
            SpikeRating::Hard => Rating::Hard,
            SpikeRating::Good => Rating::Good,
            SpikeRating::Easy => Rating::Easy,
        }
    }
}

fn to_api(error: SchedulingError) -> ApiError {
    match error {
        SchedulingError::InvalidInput => ApiError::invalid_input(
            "Desired retention must be between 0.7 and 0.99. Change it and try again.",
        ),
        SchedulingError::Fsrs(_) => ApiError::internal(),
    }
}

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ScheduleInput {
    pub ratings: Vec<SpikeRating>,
    pub desired_retention: f64,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ScheduledReview {
    pub interval_days: u32,
    pub stability: f64,
    pub difficulty: f64,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ScheduleOutput {
    pub reviews: Vec<ScheduledReview>,
}

/// Schedules a new card through the given ratings, reviewing it on each due day.
pub struct SpikeSchedule;

impl Method for SpikeSchedule {
    const NAME: &'static str = "spikeSchedule";
    type Input = ScheduleInput;
    type Output = ScheduleOutput;

    fn call(_: &Core, input: ScheduleInput, _: &OpContext) -> Result<ScheduleOutput, ApiError> {
        let ratings: Vec<Rating> = input.ratings.into_iter().map(Rating::from).collect();
        let reviews = Scheduler::new(None, input.desired_retention as f32)
            .and_then(|scheduler| scheduler.schedule_sequence(&ratings))
            .map_err(to_api)?
            .into_iter()
            .map(|r| ScheduledReview {
                interval_days: r.interval_days,
                stability: f64::from(r.memory.stability),
                difficulty: f64::from(r.memory.difficulty),
            })
            .collect();
        Ok(ScheduleOutput { reviews })
    }
}

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct OptimiseInput {
    pub cards: u32,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct OptimiseOutput {
    pub parameters: Vec<f64>,
}

/// Optimises FSRS parameters on made-up review histories. Timing is measured by the caller.
pub struct SpikeOptimise;

impl Method for SpikeOptimise {
    const NAME: &'static str = "spikeOptimise";
    type Input = OptimiseInput;
    type Output = OptimiseOutput;

    fn call(_: &Core, input: OptimiseInput, _: &OpContext) -> Result<OptimiseOutput, ApiError> {
        if !(1..=2000).contains(&input.cards) {
            return Err(ApiError::invalid_input(
                "Cards must be between 1 and 2000. Change it and try again.",
            ));
        }
        let histories = scheduling::synthetic_histories(input.cards);
        let parameters = scheduling::optimise(&histories).map_err(to_api)?;
        Ok(OptimiseOutput {
            parameters: parameters.into_iter().map(f64::from).collect(),
        })
    }
}
