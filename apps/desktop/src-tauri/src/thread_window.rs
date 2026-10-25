//! A conversation read in a window of its own (Spark's "open in a new
//! window"): the same page, which reads the message from its window's label
//! (`thread-<message id>`). One window per message: asked again, it comes to
//! the front.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// The label of a message's window; the page reads the message from it.
fn label(message_id: i64) -> String {
    format!("thread-{message_id}")
}

/// Open the message's conversation in its own window, or bring that window
/// to the front. Async: on Windows, a window made from a synchronous command
/// can hang.
#[tauri::command]
pub(crate) async fn thread_window(app: AppHandle, message_id: i64, title: String) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(&label(message_id)) {
        let _ = window.unminimize();
        return window.set_focus().map_err(|error| error.to_string());
    }
    let title = match title.trim() {
        "" => "(no subject)",
        title => title,
    };
    let builder = WebviewWindowBuilder::new(&app, label(message_id), WebviewUrl::App("index.html".into()))
        .title(title)
        .inner_size(900.0, 760.0)
        .min_inner_size(520.0, 420.0);
    // The main window's frame (tauri.windows.conf.json, tauri.macos.conf.json).
    #[cfg(windows)]
    let builder = builder.decorations(false).shadow(true);
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    builder.build().map(drop).map_err(|error| error.to_string())
}
