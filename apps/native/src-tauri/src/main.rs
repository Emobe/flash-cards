// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // WebKitGTK's DMA-BUF renderer shows a blank window on some GPUs (notably NVIDIA). Turn it off
    // unless the user set the variable themselves (ADR 0012, decision 4).
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        // SAFETY: this is the first thing `main` does, before Tauri or anything else starts a
        // thread, so nothing else can be reading or writing the environment.
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }

    fc_native_lib::run();
}
