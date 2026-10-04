//! Core library for the app. Holds domain logic shared by the desktop and
//! mobile apps, the web client, the dev CLI and the sync server.
//!
//! Placeholder until Phase 1. It must stay free of Tauri, UI and
//! platform-specific dependencies (see `docs/adr/0001-workspace-layout.md`).

/// Version of the core library, taken from its Cargo manifest.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_matches_manifest() {
        assert_eq!(version(), "0.0.0");
    }
}
