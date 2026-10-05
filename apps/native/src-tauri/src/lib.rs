//! Tauri host shared by desktop and mobile. The mobile entry point lets
//! Android (step 0.2) reuse this crate unchanged.
//!
//! Exposes the core to the UI through one generic command (`docs/adr/0002-ui-core-bridge.md`).

use std::sync::Arc;

use fc_api::{ApiError, Core, OpContext};
use tauri::State;
use tauri::ipc::Response;

/// Runs one core method. The reply is a frame: `u32` little-endian JSON length, the JSON output,
/// then attachment bytes (none yet). Errors reject the JS promise with `{ kind, message }`.
#[tauri::command]
async fn call(
    core: State<'_, Arc<Core>>,
    method: String,
    input: serde_json::Value,
) -> Result<Response, ApiError> {
    let core = Arc::clone(&core);
    let result = tauri::async_runtime::spawn_blocking(move || {
        fc_api::dispatch(&core, &method, input, &OpContext::uncancellable())
    })
    .await;
    // A panic inside a method surfaces as a join error: report it as `internal`, never its text.
    let output = result.map_err(|_| ApiError::internal())??;
    let json = serde_json::to_vec(&output).map_err(|_| ApiError::internal())?;
    Ok(Response::new(frame(&json, &[])))
}

fn frame(json: &[u8], attachment: &[u8]) -> Vec<u8> {
    let len = u32::try_from(json.len()).expect("JSON output is under 4 GiB");
    let mut out = Vec::with_capacity(4 + json.len() + attachment.len());
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(json);
    out.extend_from_slice(attachment);
    out
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(Arc::new(Core::new()))
        .invoke_handler(tauri::generate_handler![call])
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
