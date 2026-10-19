//! Opening the operating system's own settings, and web pages, from the
//! app's Settings: notification permissions, the default mail app, help
//! pages. Only these panes, and only `https://` pages, in the system's
//! browser.

use serde::Deserialize;

/// A pane of the system's settings.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Pane {
    /// Whether Regards may show notifications.
    Notifications,
    /// Which app opens `mailto:` links.
    DefaultApps,
}

/// Where each pane is, or why there is none to open.
fn pane_address(pane: Pane) -> Result<&'static str, &'static str> {
    match pane {
        Pane::Notifications if cfg!(windows) => Ok("ms-settings:notifications"),
        Pane::DefaultApps if cfg!(windows) => Ok("ms-settings:defaultapps"),
        Pane::Notifications if cfg!(target_os = "macos") => {
            Ok("x-apple.systempreferences:com.apple.preference.notifications")
        }
        Pane::DefaultApps if cfg!(target_os = "macos") => {
            Err("on macOS, the default mail app is chosen in Mail, under Settings › General")
        }
        _ => Err("open your desktop's settings to change this"),
    }
}

#[tauri::command]
pub(crate) fn open_system_settings(pane: Pane) -> Result<(), String> {
    let address = pane_address(pane)?;
    open::that_detached(address).map_err(|error| format!("could not open the settings: {error}"))
}

/// A web page in the system's browser: `https://` only.
#[tauri::command]
pub(crate) fn open_url(url: String) -> Result<(), String> {
    let safe = url
        .strip_prefix("https://")
        .is_some_and(|rest| !rest.is_empty() && !rest.chars().any(|c| c.is_whitespace() || c.is_control()));
    if !safe {
        return Err("only https:// pages are opened".into());
    }
    open::that_detached(&url).map_err(|error| format!("could not open the browser: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_secure_pages_and_known_panes() {
        assert!(open_url("http://example.test".into()).is_err());
        assert!(open_url("javascript:alert(1)".into()).is_err());
        assert!(open_url("https://".into()).is_err());
        assert!(open_url("https://example.test/a b".into()).is_err());
        if cfg!(windows) {
            assert_eq!(pane_address(Pane::DefaultApps), Ok("ms-settings:defaultapps"));
        }
        if cfg!(target_os = "macos") {
            assert!(pane_address(Pane::DefaultApps).is_err());
        }
    }
}
