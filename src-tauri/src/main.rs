// Muse desktop shell.
//
// This app deliberately has no local frontend and no custom commands.
// It opens https://muse.ai directly in the operating system's WebView
// (WebView2 on Windows, WKWebView on macOS, WebKitGTK on Linux).
//
// Login persistence:
//   - Nothing here reads, copies, or re-injects cookies or tokens.
//   - The WebView stores muse.ai cookies / localStorage in its own
//     persistent app-data directory, exactly like a browser profile.
//   - That directory is tied to the `identifier` in tauri.conf.json.
//     Do NOT change the identifier after first login, or the stored
//     session will live under a different directory and look "logged out".
//   - Do NOT enable incognito / a non-persistent data store, and do NOT
//     clear WebView data on exit. This file intentionally does neither.
//   - If muse.ai itself expires the session, or you log out inside the
//     site, you will need to log in again. That is the site's session
//     policy, not something this shell controls.
//
// The binary stays small because the UI is the real website, not a
// bundled Chromium (Electron) runtime.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running the Muse desktop app");
}
