//! Tauri host shared by desktop and mobile. The mobile entry point lets
//! Android (step 0.2) reuse this crate unchanged.
//!
//! Exposes the core to the UI through three fixed commands: `call`, `subscribe` and `cancel`
//! (`docs/adr/0002-ui-core-bridge.md`), each guarded by a session token from a fourth command,
//! `handshake` (`docs/adr/0005-card-sandbox.md`).

mod card;
mod gate;
mod host;
mod hub;
mod ops;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use fc_api::{ApiError, Clock, Core, Notice, OpContext, autobackup, forward_events};
use tauri::ipc::{Channel, Response};
use tauri::webview::PageLoadEvent;
use tauri::{Manager, State, Webview};

use gate::Gate;
use hub::NoticeHub;
use ops::Operations;

type Hub = NoticeHub<Channel<Notice>>;

/// Hands the main frame its session token, once per page load. Card frames on Android can invoke
/// commands too, but cannot claim the token before the main frame does, and cannot read it.
#[tauri::command]
fn handshake(gate: State<'_, Arc<Gate>>) -> Result<String, ApiError> {
    gate.issue().ok_or_else(gate::connect_error)
}

/// Runs one core method. The reply is a frame: `u32` little-endian JSON length, the JSON output,
/// then the reply attachment (empty when there is none). Errors reject the JS promise with
/// `{ kind, message }`. `attachment` is base64 (ADR 0002, finding 7). `op` is the client's
/// operation ID, which `cancel` and progress notices refer to.
// Tauri injects each `State` as its own argument, so the count is not ours to reduce.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
async fn call(
    gate: State<'_, Arc<Gate>>,
    core: State<'_, Arc<Core>>,
    hub: State<'_, Arc<Hub>>,
    ops: State<'_, Arc<Operations>>,
    token: String,
    method: String,
    input: serde_json::Value,
    attachment: Option<String>,
    op: Option<u32>,
) -> Result<Response, ApiError> {
    gate.authorize(&token)?;
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
fn subscribe(
    gate: State<'_, Arc<Gate>>,
    webview: Webview,
    hub: State<'_, Arc<Hub>>,
    token: String,
    on_notice: Channel<Notice>,
) -> Result<(), ApiError> {
    gate.authorize(&token)?;
    hub.subscribe(webview.label(), on_notice);
    Ok(())
}

/// Asks operation `op` to stop at its next checkpoint. Safe to call before the operation starts.
#[tauri::command]
fn cancel(
    gate: State<'_, Arc<Gate>>,
    ops: State<'_, Arc<Operations>>,
    token: String,
    op: u32,
) -> Result<(), ApiError> {
    gate.authorize(&token)?;
    ops.cancel(op, Instant::now());
    Ok(())
}

/// Opens (or creates) the collection in the app data directory. A failure is logged and the app
/// still starts: methods that need a collection answer "No collection is open." The real startup
/// screen for this (a newer collection, a file in use) comes with the app shell in step 2.1.
///
/// Around the open (step 1.13b): a collection that the open would migrate is copied into
/// `backups/` first, and once it is open an automatic backup runs in the background if the last
/// one is older than the interval in the settings.
fn open_collection(app: &tauri::App) {
    let core = Arc::clone(&app.state::<Arc<Core>>());
    let result = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())
        .and_then(|dir| {
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let file = dir.join("collection.db");
            let location = file.to_str().ok_or("the data directory is not UTF-8")?;
            let config_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
            let host = host::host(&config_dir)?;
            let backups = dir.join(BACKUPS_FOLDER);
            let copy = autobackup::copy_before_migration(&file, &backups, host.clock.now());
            core.open_collection(location, host)
                .map_err(|e| e.to_string())?;
            Ok((backups, copy))
        });
    match result {
        Ok((backups, copy)) => {
            // The migration ran anyway: it is one transaction that leaves the old collection if
            // it fails. The message goes where the settings screen will show it.
            let copy_failed = copy.err().map(|error| {
                eprintln!("Could not copy the collection before updating it: {error}");
                format!(
                    "The copy made before updating the collection failed. {}",
                    error.message
                )
            });
            std::thread::spawn(move || backup_on_start(&core, &backups, copy_failed));
        }
        Err(error) => eprintln!("Could not open the collection: {error}"),
    }
}

/// The folder next to `collection.db` for automatic backups and copies made before an update.
const BACKUPS_FOLDER: &str = "backups";

fn backup_on_start(core: &Core, backups: &std::path::Path, copy_failed: Option<String>) {
    let now = host::SystemClock.now();
    // A backup that works clears the settings' `last_error`, so a failed copy is saved after it. A
    // failed backup has saved its own message, which says more about the state of the backups.
    match autobackup::run_if_due(core, backups, now) {
        Ok(outcome) => {
            eprintln!("Automatic backup: {outcome:?}");
            if let Some(message) = copy_failed {
                let _ = core.with_collection(|c| c.set_backup_error(Some(&message)));
            }
        }
        Err(error) => eprintln!("Automatic backup failed: {error}"),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let core = Arc::new(Core::new());
    let hub = Arc::new(Hub::default());
    // Core events (a merge, a background sync) reach the webview even with no call in progress.
    forward_events(&core, hub.clone());
    tauri::Builder::default()
        .manage(core)
        .manage(hub)
        .manage(Arc::new(Operations::default()))
        .manage(Arc::new(Gate::default()))
        .setup(|app| {
            open_collection(app);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![handshake, call, subscribe, cancel])
        .register_uri_scheme_protocol("card", |_ctx, request| card::respond(request.uri().path()))
        // A new main-frame load starts a new session: the token is claimed again by the new page.
        // Card frames must not reset it (ADR 0005), so only the `main` webview counts.
        .on_page_load(|webview, payload| {
            #[cfg(debug_assertions)]
            eprintln!(
                "[page-load] {} {:?} {}",
                webview.label(),
                payload.event(),
                payload.url()
            );
            if payload.event() == PageLoadEvent::Started && webview.label() == "main" {
                webview.state::<Arc<Gate>>().reset();
            }
        })
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
