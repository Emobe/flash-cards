fn main() {
    // App commands become permissions (`allow-<command>`) that capabilities grant per window,
    // instead of being callable from every webview. See docs/adr/0002-ui-core-bridge.md.
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "handshake",
            "call",
            "subscribe",
            "cancel",
            "set_system_theme",
        ]),
    ))
    .expect("failed to run the Tauri build script");
}
