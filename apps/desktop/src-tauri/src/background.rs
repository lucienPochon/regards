//! Background mode (ADR-0017 §1, on by default): Regards starts at login and
//! keeps running when its window closes, so snoozed mail comes back and
//! scheduled messages go while the computer is on (ADR-0011).
//!
//! - **Tray:** an icon shows Regards is running. Its menu opens the window,
//!   turns background mode on or off, and quits. Quitting stops the service
//!   too, and with it scheduled work.
//! - **Closing the window** hides it while background mode is on; otherwise it
//!   quits, as before.
//! - **At login** the app starts with [`BACKGROUND_ARG`] and no window.
//!   Development builds never register themselves to start at login.
//! - **The settings** live in `desktop.json` in the data directory: background
//!   mode, and whether to show notifications ([`crate::notify`]), both on by default.

use regards_core::paths::DataDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager, Wry};
use tauri_plugin_autostart::ManagerExt;

/// Given when the app starts at login: start in the tray, with no window.
pub(crate) const BACKGROUND_ARG: &str = "--background";

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    background: bool,
    notifications: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            background: true,
            notifications: true,
        }
    }
}

pub(crate) struct Background {
    file: Option<PathBuf>,
    on: AtomicBool,
    notifications: AtomicBool,
    /// The tray menu's check marks, kept in step with the settings.
    check: Mutex<Option<CheckMenuItem<Wry>>>,
    notify_check: Mutex<Option<CheckMenuItem<Wry>>>,
}

impl Background {
    /// The saved setting; on when there is none yet.
    pub(crate) fn load(dirs: Option<&DataDirs>) -> Self {
        let file = dirs.map(DataDirs::desktop_settings);
        let settings: Settings = file
            .as_deref()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            file,
            on: AtomicBool::new(settings.background),
            notifications: AtomicBool::new(settings.notifications),
            check: Mutex::new(None),
            notify_check: Mutex::new(None),
        }
    }

    pub(crate) fn on(&self) -> bool {
        self.on.load(Ordering::SeqCst)
    }

    pub(crate) fn notifications(&self) -> bool {
        self.notifications.load(Ordering::SeqCst)
    }

    /// Turn background mode on or off: saved, start at login, and the tray.
    pub(crate) fn set(&self, app: &AppHandle, on: bool) -> Result<(), String> {
        self.save(on, self.notifications())?;
        self.on.store(on, Ordering::SeqCst);
        start_at_login(app, on);
        if let Some(check) = self.check.lock().expect("never poisoned").as_ref() {
            let _ = check.set_checked(on);
        }
        Ok(())
    }

    /// Show notifications, or not: saved, and the tray.
    pub(crate) fn set_notifications(&self, on: bool) -> Result<(), String> {
        self.save(self.on(), on)?;
        self.notifications.store(on, Ordering::SeqCst);
        if let Some(check) = self.notify_check.lock().expect("never poisoned").as_ref() {
            let _ = check.set_checked(on);
        }
        Ok(())
    }

    fn save(&self, background: bool, notifications: bool) -> Result<(), String> {
        let file = self.file.as_deref().ok_or("the data directory could not be located")?;
        let json = serde_json::to_vec_pretty(&Settings {
            background,
            notifications,
        })
        .expect("plain data always serializes");
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(file, json).map_err(|error| format!("cannot save the setting: {error}"))
    }
}

/// The tray icon and its menu; start at login in line with the setting; the
/// window shown unless Regards started at login.
pub(crate) fn setup(app: &App) -> tauri::Result<()> {
    let background = app.state::<Background>();
    let on = background.on();
    let open = MenuItem::with_id(app, "open", "Open Regards", true, None::<&str>)?;
    let check = CheckMenuItem::with_id(app, "background", "Run in the background", true, on, None::<&str>)?;
    let notify = CheckMenuItem::with_id(
        app,
        "notifications",
        "Show notifications",
        true,
        background.notifications(),
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Quit Regards", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&open, &check, &notify, &PredefinedMenuItem::separator(app)?, &quit],
    )?;
    *background.check.lock().expect("never poisoned") = Some(check.clone());
    *background.notify_check.lock().expect("never poisoned") = Some(notify.clone());

    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Regards")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "open" => show(app),
            "background" => {
                // The menu has already toggled its check mark.
                let on = check.is_checked().unwrap_or(true);
                if let Err(error) = app.state::<Background>().set(app, on) {
                    eprintln!("background mode: {error}");
                }
            }
            "notifications" => {
                let on = notify.is_checked().unwrap_or(true);
                if let Err(error) = app.state::<Background>().set_notifications(on) {
                    eprintln!("notifications: {error}");
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;

    start_at_login(app.handle(), on);
    let at_login = std::env::args().any(|arg| arg == BACKGROUND_ARG);
    if !(at_login && on) {
        show(app.handle());
    }
    Ok(())
}

/// Bring the window back (from the tray, or a second launch).
pub(crate) fn show(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Register or remove the login item to match the setting. It is written
/// again each time, so it follows the app when it moves or updates.
fn start_at_login(app: &AppHandle, on: bool) {
    if cfg!(debug_assertions) {
        return;
    }
    let launcher = app.autolaunch();
    let result = if on {
        launcher.enable()
    } else {
        match launcher.is_enabled() {
            Ok(true) => launcher.disable(),
            other => other.map(|_| ()),
        }
    };
    if let Err(error) = result {
        eprintln!("start at login: {error}");
    }
}
