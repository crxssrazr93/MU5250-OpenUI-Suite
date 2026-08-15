//! Desktop shell for the MU5250 OpenUI dashboard.
//!
//! There is deliberately almost nothing here. The dashboard is the same React
//! app the agent serves from the router, and everything it knows about the
//! firmware lives in that one place. What the shell adds is the HTTP plugin,
//! which makes requests from Rust rather than from the webview.
//!
//! That is the whole reason this is not just a window pointed at the router.
//! The agent only accepts cross-origin requests from LAN addresses, and a
//! desktop window's origin is not one. Requests issued outside a browser carry
//! no origin to check, so the desktop build works against the agent's existing
//! policy instead of asking for it to be relaxed. Which addresses it may reach
//! is then bounded by the HTTP scope in `capabilities/default.json`.

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_http::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
