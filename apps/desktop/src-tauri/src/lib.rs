//! Tauri host shared by desktop and mobile. The mobile entry point lets
//! Android (step 0.2) reuse this crate unchanged.

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running the Tauri application");
}
