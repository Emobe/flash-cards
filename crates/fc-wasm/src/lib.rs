//! Web host for the core: the wasm exports a browser worker calls, the counterpart of `fc-native`.
//! See `docs/adr/0003-web-client.md`. Compiles to an empty crate on native targets.

#![forbid(unsafe_code)]
#![cfg(target_arch = "wasm32")]

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use fc_api::{
    ApiError, Clock, Core, EventSink, Host, Id, Notice, OpContext, Reading, dispatch,
    forward_events,
};
use js_sys::{Date, Function, Object, Reflect, Uint8Array};
use sqlite_wasm_rs::WasmOsCallback;
use sqlite_wasm_vfs::sahpool::{OpfsSAHPoolCfg, install};
use wasm_bindgen::prelude::*;

thread_local! {
    static CORE: Core = Core::new();
    static FORWARDING: Cell<bool> = const { Cell::new(false) };
    static ON_NOTICE: RefCell<Option<Function>> = const { RefCell::new(None) };
}

/// Sends notices to the JS callback given to `init`. Zero-sized so it is `Send + Sync` without
/// `unsafe`: the callback lives in a `thread_local`.
struct JsSink;

impl EventSink for JsSink {
    fn send(&self, notice: Notice) {
        let Ok(json) = serde_json::to_string(&notice) else {
            return;
        };
        ON_NOTICE.with(|callback| {
            if let Some(callback) = callback.borrow().as_ref() {
                let _ = callback.call1(&JsValue::NULL, &JsValue::from_str(&json));
            }
        });
    }
}

/// Stores the notice callback (it receives each notice as a JSON string) and installs a panic hook
/// that logs the message to the console before the call traps.
#[wasm_bindgen]
pub fn init(on_notice: Function) {
    ON_NOTICE.with(|callback| *callback.borrow_mut() = Some(on_notice));
    // Core events reach the page even with no call in progress. Once per instance, so a second
    // `init` does not send every event twice.
    if !FORWARDING.replace(true) {
        CORE.with(|core| forward_events(core, Arc::new(JsSink)));
    }
    std::panic::set_hook(Box::new(|info| {
        console_error(&info.to_string());
    }));
}

fn console_error(message: &str) {
    let console = Reflect::get(&js_sys::global(), &JsValue::from_str("console")).ok();
    if let Some(console) = console
        && let Ok(error) = Reflect::get(&console, &JsValue::from_str("error"))
        && let Some(error) = error.dyn_ref::<Function>()
    {
        let _ = error.call1(&console, &JsValue::from_str(message));
    }
}

fn thrown(error: &ApiError) -> JsValue {
    JsValue::from_str(&serde_json::to_string(error).unwrap_or_default())
}

/// The browser's clock: `Date.now()` and the time zone offset. Zero-sized, so `Send + Sync`.
#[derive(Debug)]
struct JsClock;

impl Clock for JsClock {
    fn now(&self) -> Reading {
        Reading {
            // `Date.now()` is a whole number of milliseconds.
            unix_ms: Date::now() as i64,
            // `getTimezoneOffset()` is minutes *behind* UTC, so the sign flips.
            utc_offset_minutes: -(Date::new_0().get_timezone_offset() as i32),
        }
    }
}

/// Installs the `opfs-sahpool` VFS (not as the default) and opens the collection in it, creating
/// it on first use. Throws an `ApiError` as a JSON string, for example `updateRequired` for a
/// collection from a newer app. Fails while another tab holds the OPFS handles.
/// `installation_id` is a UUID string the worker keeps outside the collection (ADR 0006).
#[wasm_bindgen]
pub async fn open(name: String, installation_id: String) -> Result<(), JsValue> {
    let installation_id = installation_id.parse::<Id>().map_err(|_| {
        console_error("The installation ID is not a UUID");
        thrown(&ApiError::internal())
    })?;
    let host = Host {
        clock: Arc::new(JsClock),
        installation_id,
    };
    install::<WasmOsCallback>(&OpfsSAHPoolCfg::default(), false)
        .await
        .map_err(|e| {
            console_error(&format!("Could not install the OPFS VFS: {e}"));
            thrown(&ApiError::internal())
        })?;
    CORE.with(|core| core.open_collection(&format!("file:{name}?vfs=opfs-sahpool"), host))
        .map_err(|e| {
            console_error(&format!("Could not open the collection: {e:?}"));
            thrown(&ApiError::from(e))
        })
}

/// Runs one API method. Returns `{ output: string, bytes: Uint8Array }`, or throws the `ApiError`
/// as a JSON string. Cancellation never fires here: a busy call cannot see a flag (ADR 0003).
#[wasm_bindgen]
pub fn call(
    method: &str,
    input: &str,
    bytes: Option<Vec<u8>>,
    op: Option<u32>,
) -> Result<JsValue, JsValue> {
    let input = serde_json::from_str(input).map_err(|_| thrown(&ApiError::internal()))?;
    let cancelled = || false;
    let mut ctx = OpContext::new(&cancelled).with_sink(&JsSink, op);
    if let Some(bytes) = bytes.as_deref() {
        ctx = ctx.with_attachment(bytes);
    }
    let reply = CORE
        .with(|core| dispatch(core, method, input, &ctx))
        .map_err(|e| thrown(&e))?;
    let output = serde_json::to_string(&reply.output).map_err(|_| thrown(&ApiError::internal()))?;
    let result = Object::new();
    let attachment = reply.attachment.unwrap_or_default();
    Reflect::set(&result, &"output".into(), &JsValue::from_str(&output))?;
    Reflect::set(
        &result,
        &"bytes".into(),
        &Uint8Array::from(attachment.as_slice()),
    )?;
    Ok(result.into())
}
