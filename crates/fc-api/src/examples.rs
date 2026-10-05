//! Temporary example methods for step 0.3. Deleted when real methods arrive (Phase 1).

use fc_core::Core;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{ApiError, Method, OpContext};

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct CoreInfo {
    pub core_version: String,
}

pub struct GetCoreInfo;

impl Method for GetCoreInfo {
    const NAME: &'static str = "getCoreInfo";
    type Input = ();
    type Output = CoreInfo;

    fn call(_: &Core, (): (), _: &OpContext) -> Result<CoreInfo, ApiError> {
        Ok(CoreInfo {
            core_version: fc_core::version().to_owned(),
        })
    }
}

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct DivideInput {
    pub dividend: f64,
    pub divisor: f64,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct DivideOutput {
    pub quotient: f64,
}

pub struct ExampleDivide;

impl Method for ExampleDivide {
    const NAME: &'static str = "exampleDivide";
    type Input = DivideInput;
    type Output = DivideOutput;

    fn call(_: &Core, input: DivideInput, _: &OpContext) -> Result<DivideOutput, ApiError> {
        fc_core::example_divide(input.dividend, input.divisor)
            .map(|quotient| DivideOutput { quotient })
            .map_err(|_| {
                ApiError::invalid_input("Can't divide by zero. Enter a divisor other than 0.")
            })
    }
}
