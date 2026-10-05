//! Debug-only methods that exercise progress, cancellation, attachments and events (step 0.3b).
//! Registered only when `debug_assertions` is on. Deleted when real long methods arrive.

use std::time::Duration;

use fc_core::Core;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{ApiError, CoreEvent, Method, OpContext, Progress};

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SlowInput {
    pub steps: u32,
    pub step_ms: u32,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SlowOutput {
    pub completed: u32,
}

/// Runs `steps` steps of `stepMs` each, with a checkpoint and a progress report per step.
pub struct DebugSlow;

impl Method for DebugSlow {
    const NAME: &'static str = "debugSlow";
    type Input = SlowInput;
    type Output = SlowOutput;

    fn call(_: &Core, input: SlowInput, ctx: &OpContext) -> Result<SlowOutput, ApiError> {
        for step in 1..=input.steps {
            ctx.checkpoint()?;
            std::thread::sleep(Duration::from_millis(u64::from(input.step_ms)));
            ctx.progress(Progress {
                done: step,
                total: Some(input.steps),
                message: None,
            });
        }
        Ok(SlowOutput {
            completed: input.steps,
        })
    }
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct EchoOutput {
    pub length: u32,
}

/// Returns the attachment it was sent, and its length.
pub struct DebugEchoBytes;

impl Method for DebugEchoBytes {
    const NAME: &'static str = "debugEchoBytes";
    const ATTACHMENT_IN: bool = true;
    const ATTACHMENT_OUT: bool = true;
    type Input = ();
    type Output = EchoOutput;

    fn call(_: &Core, (): (), ctx: &OpContext) -> Result<EchoOutput, ApiError> {
        let bytes = ctx.attachment().ok_or_else(|| {
            ApiError::invalid_input("This request needs an attachment and none was sent.")
        })?;
        let length = u32::try_from(bytes.len())
            .map_err(|_| ApiError::invalid_input("The attachment is too large."))?;
        ctx.set_reply_attachment(bytes.to_vec());
        Ok(EchoOutput { length })
    }
}

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct EmitInput {
    pub message: String,
}

/// Emits one `CoreEvent::Debug`.
pub struct DebugEmitEvent;

impl Method for DebugEmitEvent {
    const NAME: &'static str = "debugEmitEvent";
    type Input = EmitInput;
    type Output = ();

    fn call(_: &Core, input: EmitInput, ctx: &OpContext) -> Result<(), ApiError> {
        ctx.emit(CoreEvent::Debug {
            message: input.message,
        });
        Ok(())
    }
}
