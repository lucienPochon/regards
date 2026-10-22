//! A message written in a window of its own, as Spark's composer opens one:
//! the same page, which reads the draft from its window's label
//! (`compose-<draft id>`). One window per draft: asked again, it comes to
//! the front.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// The label of a draft's window; the page reads the draft from it.
fn label(draft_id: i64) -> String {
    format!("compose-{draft_id}")
}

/// Open the draft in its own window, or bring that window to the front.
/// Async: on Windows, a window made from a synchronous command can hang.
#[tauri::command]
pub(crate) async fn composer_window(app: AppHandle, draft_id: i64, title: String) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(&label(draft_id)) {
        let _ = window.unminimize();
        return window.set_focus().map_err(|error| error.to_string());
    }
    let title = match title.trim() {
        "" => "New message",
        title => title,
    };
    let builder = WebviewWindowBuilder::new(&app, label(draft_id), WebviewUrl::App("index.html".into()))
        .title(title)
        .inner_size(860.0, 720.0)
        .min_inner_size(560.0, 420.0);
    // The main window's frame (tauri.windows.conf.json, tauri.macos.conf.json).
    #[cfg(windows)]
    let builder = builder.decorations(false).shadow(true);
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    builder.build().map(drop).map_err(|error| error.to_string())
}

/// Whether the draft is open in a window of its own.
#[tauri::command]
pub(crate) fn composer_window_open(app: AppHandle, draft_id: i64) -> bool {
    app.get_webview_window(&label(draft_id)).is_some()
}
