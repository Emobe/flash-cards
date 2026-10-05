//! Temporary method for the card sandbox spike (step 0.6): bundled sample media for the demo cards,
//! delivered the way real media will be, as an attachment (ADR 0005). Deleted in step 1.10 when the
//! real media store arrives.

use fc_core::Core;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{ApiError, ErrorKind, Method, OpContext};

const SAMPLE_PNG: &[u8] = include_bytes!("spike_media/sample.png");
const SAMPLE_WAV: &[u8] = include_bytes!("spike_media/sample.wav");

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct CardMediaInput {
    pub name: String,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct CardMediaOutput {
    /// For the `Blob` that wraps the attachment, so the frame can decode it.
    pub content_type: String,
}

/// Returns a bundled sample file (`sample.png` or `sample.wav`) as the reply attachment.
pub struct SpikeCardMedia;

impl Method for SpikeCardMedia {
    const NAME: &'static str = "spikeCardMedia";
    const ATTACHMENT_OUT: bool = true;
    type Input = CardMediaInput;
    type Output = CardMediaOutput;

    fn call(_: &Core, input: CardMediaInput, ctx: &OpContext) -> Result<CardMediaOutput, ApiError> {
        let (bytes, content_type) = match input.name.as_str() {
            "sample.png" => (SAMPLE_PNG, "image/png"),
            "sample.wav" => (SAMPLE_WAV, "audio/wav"),
            other => {
                return Err(ApiError::new(
                    ErrorKind::NotFound,
                    format!("There is no media file called \"{other}\"."),
                ));
            }
        };
        ctx.set_reply_attachment(bytes.to_vec());
        Ok(CardMediaOutput {
            content_type: content_type.into(),
        })
    }
}
