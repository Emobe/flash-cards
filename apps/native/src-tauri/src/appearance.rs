//! System bars and title bar follow the app's theme (ADR 0010 decision 3).
//!
//! Android: the status and navigation bar icons are set by `AppearancePlugin.kt`, a local Tauri
//! plugin that is called from Rust only. It is registered with no commands and no permissions, so
//! no webview, card frames included, can invoke it (ADR 0005, "No Tauri plugin commands for
//! JavaScript"). The page reaches it through the token-checked `set_system_theme` command.
//!
//! Desktop: `Window::set_theme`, so the title bar matches.

use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Manager, Runtime, Webview, Wry};

/// The Kotlin class that `register_android_plugin` looks up in the app package.
#[cfg(target_os = "android")]
const ANDROID_PACKAGE: &str = "io.github.emobe.flashcards";
#[cfg(target_os = "android")]
const ANDROID_CLASS: &str = "AppearancePlugin";

/// Managed state. Holds the handle to the Kotlin plugin on Android and nothing elsewhere.
pub struct Appearance {
    #[cfg(target_os = "android")]
    handle: tauri::plugin::PluginHandle<Wry>,
}

/// The `appearance` plugin. It has no `invoke_handler` and no permissions on purpose.
pub fn plugin() -> TauriPlugin<Wry> {
    Builder::new("appearance")
        .setup(|app, _api| {
            #[cfg(target_os = "android")]
            let state = Appearance {
                handle: _api.register_android_plugin(ANDROID_PACKAGE, ANDROID_CLASS)?,
            };
            #[cfg(not(target_os = "android"))]
            let state = Appearance {};
            app.manage(state);
            Ok(())
        })
        .build()
}

impl Appearance {
    /// Makes the platform chrome match `dark`. `follow_system` is true when the user's setting is
    /// System: the desktop window is then released to follow the desktop theme again, because
    /// pinning it would stop `prefers-color-scheme` from following the system (ADR 0010).
    pub fn apply<R: Runtime>(
        &self,
        webview: &Webview<R>,
        dark: bool,
        follow_system: bool,
    ) -> Result<(), String> {
        #[cfg(desktop)]
        {
            let theme = match (follow_system, dark) {
                (true, _) => None,
                (false, true) => Some(tauri::Theme::Dark),
                (false, false) => Some(tauri::Theme::Light),
            };
            webview.window().set_theme(theme).map_err(|e| e.to_string())
        }
        #[cfg(target_os = "android")]
        {
            let _ = (webview, follow_system);
            self.handle
                // `lightBars` is true when the page is light, so the bar icons must be dark.
                .run_mobile_plugin::<()>("setBarStyle", serde_json::json!({ "lightBars": !dark }))
                .map_err(|e| e.to_string())
        }
        #[cfg(not(any(desktop, target_os = "android")))]
        {
            let _ = (webview, dark, follow_system);
            Ok(())
        }
    }
}
