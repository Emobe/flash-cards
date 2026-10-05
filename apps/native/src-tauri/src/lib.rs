//! Tauri host shared by desktop and mobile. The mobile entry point lets
//! Android (step 0.2) reuse this crate unchanged.
//!
//! Exposes the core to the UI through three fixed commands: `call`, `subscribe` and `cancel`
//! (`docs/adr/0002-ui-core-bridge.md`).

mod hub;
mod ops;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use fc_api::{ApiError, Core, Notice, OpContext};
use tauri::ipc::{Channel, Response};
use tauri::{State, Webview};

use hub::NoticeHub;
use ops::Operations;

type Hub = NoticeHub<Channel<Notice>>;

/// Runs one core method. The reply is a frame: `u32` little-endian JSON length, the JSON output,
/// then the reply attachment (empty when there is none). Errors reject the JS promise with
/// `{ kind, message }`. `attachment` is base64 (ADR 0002, finding 7). `op` is the client's
/// operation ID, which `cancel` and progress notices refer to.
#[tauri::command]
async fn call(
    core: State<'_, Arc<Core>>,
    hub: State<'_, Arc<Hub>>,
    ops: State<'_, Arc<Operations>>,
    method: String,
    input: serde_json::Value,
    attachment: Option<String>,
    op: Option<u32>,
) -> Result<Response, ApiError> {
    let attachment = attachment
        .map(|text| STANDARD.decode(text))
        .transpose()
        .map_err(|_| {
            ApiError::invalid_input(
                "The app sent an attachment that could not be read. Update the app and try again.",
            )
        })?;
    let flag = op.map(|op| ops.start(op, Instant::now()));
    let (core, ops, hub_for_op) = (Arc::clone(&core), Arc::clone(&ops), Arc::clone(&hub));
    let result = tauri::async_runtime::spawn_blocking(move || {
        let cancelled = || flag.as_ref().is_some_and(|f| f.load(Ordering::SeqCst));
        let mut ctx = OpContext::new(&cancelled).with_sink(&*hub_for_op, op);
        if let Some(bytes) = attachment.as_deref() {
            ctx = ctx.with_attachment(bytes);
        }
        fc_api::dispatch(&core, &method, input, &ctx)
    })
    .await;
    if let Some(op) = op {
        ops.finish(op);
        hub.finish(op);
    }
    // A panic inside a method surfaces as a join error: report it as `internal`, never its text.
    let reply = result.map_err(|_| ApiError::internal())??;
    let json = serde_json::to_vec(&reply.output).map_err(|_| ApiError::internal())?;
    Ok(Response::new(frame(
        &json,
        reply.attachment.as_deref().unwrap_or(&[]),
    )))
}

fn frame(json: &[u8], attachment: &[u8]) -> Vec<u8> {
    let len = u32::try_from(json.len()).expect("JSON output is under 4 GiB");
    let mut out = Vec::with_capacity(4 + json.len() + attachment.len());
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(json);
    out.extend_from_slice(attachment);
    out
}

/// Registers the webview's notice channel (events and progress). A reload subscribes again and
/// replaces the old channel.
#[tauri::command]
fn subscribe(webview: Webview, hub: State<'_, Arc<Hub>>, on_notice: Channel<Notice>) {
    hub.subscribe(webview.label(), on_notice);
}

/// Asks operation `op` to stop at its next checkpoint. Safe to call before the operation starts.
#[tauri::command]
fn cancel(ops: State<'_, Arc<Operations>>, op: u32) {
    ops.cancel(op, Instant::now());
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(Arc::new(Core::new()))
        .manage(Arc::new(Hub::default()))
        .manage(Arc::new(Operations::default()))
        .invoke_handler(tauri::generate_handler![call, subscribe, cancel])
        .run(tauri::generate_context!())
        .expect("error while running the Tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_has_length_prefix_json_and_attachment() {
        let out = frame(b"{}", &[9, 8]);
        assert_eq!(out, [2, 0, 0, 0, b'{', b'}', 9, 8]);
    }
}
