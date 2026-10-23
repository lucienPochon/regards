//! Background mode (ADR-0017 §1, on by default): Regards starts at login and
//! keeps running when its window closes, so snoozed mail comes back and
//! scheduled messages go while the computer is on (ADR-0011).
//!
//! - **Tray:** an icon shows Regards is running. Its menu opens the window,
//!   turns background mode on or off, and quits. Quitting stops the service
//!   too, and with it scheduled work.
//! - **Closing the window** hides it while background mode is on; otherwise it
//!   quits, as before. The first time, a notification says Regards is still
//!   running and where to quit it (the onboarding ADR-0017 asks for).
//! - **At login** the app starts with [`BACKGROUND_ARG`] and no window.
//!   Development builds never register themselves to start at login.
//! - **The settings** live in `desktop.json` in the data directory: background
//!   mode, and whether to show notifications ([`crate::notify`]), both on by
//!   default; and whether to check for updates ([`crate::updates`]), not
//!   asked yet by default.

use crate::badge::BadgeChoices;
use crate::notify::NotifyChoices;
use regards_core::paths::DataDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager, Wry};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_window_state::{StateFlags, WindowExt};

/// Given when the app starts at login: start in the tray, with no window.
pub(crate) const BACKGROUND_ARG: &str = "--background";

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    /// Closing the window leaves Regards in the tray.
    background: bool,
    /// Start at login; `None`: as `background` (one setting before).
    start_at_login: Option<bool>,
    notifications: bool,
    /// Told once that closing the window leaves Regards running.
    told_tray: bool,
    /// Check for updates once a day; `None` until the person was asked.
    update_checks: Option<bool>,
    /// The latest version the person was told of.
    told_update: Option<String>,
    /// Which notifications to show.
    notify: NotifyChoices,
    /// The service logs in detail (Settings › Support).
    detailed_logs: bool,
    /// The unread count on the taskbar or dock.
    badge: BadgeChoices,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            background: true,
            start_at_login: None,
            notifications: true,
            told_tray: false,
            update_checks: None,
            told_update: None,
            notify: NotifyChoices::default(),
            detailed_logs: false,
            badge: BadgeChoices::default(),
        }
    }
}

pub(crate) struct Background {
    file: Option<PathBuf>,
    on: AtomicBool,
    login: AtomicBool,
    notifications: AtomicBool,
    told_tray: AtomicBool,
    update_checks: Mutex<Option<bool>>,
    told_update: Mutex<Option<String>>,
    notify: Mutex<NotifyChoices>,
    detailed_logs: AtomicBool,
    badge: Mutex<BadgeChoices>,
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
            login: AtomicBool::new(settings.start_at_login.unwrap_or(settings.background)),
            notifications: AtomicBool::new(settings.notifications),
            told_tray: AtomicBool::new(settings.told_tray),
            update_checks: Mutex::new(settings.update_checks),
            told_update: Mutex::new(settings.told_update),
            notify: Mutex::new(settings.notify),
            detailed_logs: AtomicBool::new(settings.detailed_logs),
            badge: Mutex::new(settings.badge),
            check: Mutex::new(None),
            notify_check: Mutex::new(None),
        }
    }

    /// Closing the window leaves Regards in the tray.
    pub(crate) fn on(&self) -> bool {
        self.on.load(Ordering::SeqCst)
    }

    pub(crate) fn login(&self) -> bool {
        self.login.load(Ordering::SeqCst)
    }

    pub(crate) fn notifications(&self) -> bool {
        self.notifications.load(Ordering::SeqCst)
    }

    /// Turn background mode on or off, both its halves: saved, start at
    /// login, and the tray.
    pub(crate) fn set(&self, app: &AppHandle, on: bool) -> Result<(), String> {
        self.set_close_to_tray(on)?;
        self.set_start_at_login(app, on)
    }

    /// Whether closing the window leaves Regards in the tray: saved, and the tray.
    pub(crate) fn set_close_to_tray(&self, on: bool) -> Result<(), String> {
        self.save(Settings {
            background: on,
            ..self.current()
        })?;
        self.on.store(on, Ordering::SeqCst);
        if let Some(check) = self.check.lock().expect("never poisoned").as_ref() {
            let _ = check.set_checked(on);
        }
        Ok(())
    }

    /// Whether Regards starts at login: saved, and the login item.
    pub(crate) fn set_start_at_login(&self, app: &AppHandle, on: bool) -> Result<(), String> {
        self.save(Settings {
            start_at_login: Some(on),
            ..self.current()
        })?;
        self.login.store(on, Ordering::SeqCst);
        start_at_login(app, on);
        Ok(())
    }

    /// Show notifications, or not: saved, and the tray.
    pub(crate) fn set_notifications(&self, on: bool) -> Result<(), String> {
        self.save(Settings {
            notifications: on,
            ..self.current()
        })?;
        self.notifications.store(on, Ordering::SeqCst);
        if let Some(check) = self.notify_check.lock().expect("never poisoned").as_ref() {
            let _ = check.set_checked(on);
        }
        Ok(())
    }

    pub(crate) fn update_checks(&self) -> Option<bool> {
        *self.update_checks.lock().expect("never poisoned")
    }

    /// Check for updates once a day, or not: saved.
    pub(crate) fn set_update_checks(&self, on: bool) -> Result<(), String> {
        self.save(Settings {
            update_checks: Some(on),
            ..self.current()
        })?;
        *self.update_checks.lock().expect("never poisoned") = Some(on);
        Ok(())
    }

    pub(crate) fn badge_choices(&self) -> BadgeChoices {
        self.badge.lock().expect("never poisoned").clone()
    }

    /// What the taskbar or dock counts: saved.
    pub(crate) fn set_badge_choices(&self, choices: BadgeChoices) -> Result<(), String> {
        self.save(Settings {
            badge: choices.clone(),
            ..self.current()
        })?;
        *self.badge.lock().expect("never poisoned") = choices;
        Ok(())
    }

    pub(crate) fn detailed_logs(&self) -> bool {
        self.detailed_logs.load(Ordering::SeqCst)
    }

    /// Log in detail from the service's next start: saved.
    pub(crate) fn set_detailed_logs(&self, on: bool) -> Result<(), String> {
        self.save(Settings {
            detailed_logs: on,
            ..self.current()
        })?;
        self.detailed_logs.store(on, Ordering::SeqCst);
        Ok(())
    }

    pub(crate) fn notify_choices(&self) -> NotifyChoices {
        self.notify.lock().expect("never poisoned").clone()
    }

    /// Which notifications to show: saved.
    pub(crate) fn set_notify_choices(&self, choices: NotifyChoices) -> Result<(), String> {
        self.save(Settings {
            notify: choices.clone(),
            ..self.current()
        })?;
        *self.notify.lock().expect("never poisoned") = choices;
        Ok(())
    }

    pub(crate) fn told_update(&self) -> Option<String> {
        self.told_update.lock().expect("never poisoned").clone()
    }

    /// The person was told of `version`: not again.
    pub(crate) fn set_told_update(&self, version: &str) {
        *self.told_update.lock().expect("never poisoned") = Some(version.to_owned());
        if let Err(error) = self.save(self.current()) {
            eprintln!("updates: {error}");
        }
    }

    /// The window was closed into the tray. The first time, say that Regards
    /// keeps running, why, and where to quit it.
    pub(crate) fn hidden(&self, app: &AppHandle) {
        if self.told_tray.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Err(error) = self.save(self.current()) {
            eprintln!("background mode: {error}");
        }
        let place = if cfg!(target_os = "macos") { "menu bar" } else { "tray" };
        let shown = app
            .notification()
            .builder()
            .title("Regards is still running")
            .body(format!(
                "It stays in the {place} so snoozed mail comes back and scheduled messages go. Quit it from the {place} icon."
            ))
            .show();
        if let Err(error) = shown {
            eprintln!("notification: {error}");
        }
    }

    fn current(&self) -> Settings {
        Settings {
            background: self.on(),
            start_at_login: Some(self.login()),
            notifications: self.notifications(),
            told_tray: self.told_tray.load(Ordering::SeqCst),
            update_checks: self.update_checks(),
            told_update: self.told_update(),
            notify: self.notify_choices(),
            detailed_logs: self.detailed_logs(),
            badge: self.badge_choices(),
        }
    }

    fn save(&self, settings: Settings) -> Result<(), String> {
        let file = self.file.as_deref().ok_or("the data directory could not be located")?;
        let json = serde_json::to_vec_pretty(&settings).expect("plain data always serializes");
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
    // The mark on Windows and Linux; on macOS, one colour the menu bar tints.
    #[cfg(target_os = "macos")]
    {
        tray = tray
            .icon(tauri::include_image!("icons/tray-template.png"))
            .icon_as_template(true);
    }
    #[cfg(not(target_os = "macos"))]
    {
        tray = tray.icon(tauri::include_image!("icons/tray.png"));
    }
    tray.build(app)?;

    start_at_login(app.handle(), background.login());
    let at_login = std::env::args().any(|arg| arg == BACKGROUND_ARG);
    if !(at_login && on) {
        show(app.handle());
    }
    Ok(())
}

/// What of the main window comes back as it was left: its size, its place
/// and whether it was maximised; not whether it was shown (at login it isn't).
pub(crate) const WINDOW_STATE: StateFlags = StateFlags::SIZE
    .union(StateFlags::POSITION)
    .union(StateFlags::MAXIMIZED);

/// The window is put back as it was left when it is first shown, not when it
/// is made: a maximised one would otherwise appear when Regards starts at login.
static PLACED: AtomicBool = AtomicBool::new(false);

/// Bring the window back (from the tray, or a second launch).
pub(crate) fn show(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if !PLACED.swap(true, Ordering::Relaxed) {
            let _ = window.restore_state(WINDOW_STATE);
        }
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Register or remove the login item to match the setting. It is written
/// again each time, so it follows the app when it moves or updates.
pub(crate) fn start_at_login(app: &AppHandle, on: bool) {
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
