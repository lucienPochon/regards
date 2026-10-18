//! Regards desktop app: a Tauri 2 shell around the React UI.
//!
//! The WebView gets a deliberately narrow command set (ADR-0006): it never
//! talks to the daemon directly, only through the commands defined here.
//!
//! The app starts the daemon itself (ADR-0005) and keeps the per-launch token
//! the daemon hands it, which is what gives this process the `ui` principal.

// Release builds on Windows have no console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use regards_core::paths::DataDirs;
use regards_ipc::launcher::{self, LaunchError, Launcher};
use regards_ipc::protocol::{ClientKind, Principal};
use regards_ipc::{Client, UiToken};
use serde::Serialize;
use std::path::PathBuf;
use std::time::Duration;
use tauri::{Manager, RunEvent, State};
use tokio::sync::Mutex;

/// Overrides where the `regards` program is looked for (development and tests).
const DAEMON_PROGRAM_ENV: &str = "REGARDS_DAEMON_BIN";

/// The app's link to the daemon.
struct Bridge {
    /// `Err` when the data directory or the `regards` program could not be located.
    launcher: Result<Launcher, String>,
    /// Token of the daemon this process started, if it is still the one running.
    token: Mutex<Option<UiToken>>,
}

impl Bridge {
    fn new() -> Self {
        Self {
            launcher: locate().map(|(dirs, program)| Launcher::new(dirs, program)),
            token: Mutex::new(None),
        }
    }

    /// Stop the daemon if this process started it; ADR-0005's default of
    /// quitting with the app until background mode exists.
    async fn stop_own_daemon(&self) {
        let (Ok(launcher), Some(token)) = (&self.launcher, self.token.lock().await.take()) else {
            return;
        };
        // A daemon that no longer knows the token was replaced by someone else; leave it alone.
        if let Ok(mut client) = Client::connect_with(launcher.endpoint(), ClientKind::Desktop, Some(&token)).await {
            let _ = client.shutdown().await;
        }
    }
}

fn locate() -> Result<(DataDirs, PathBuf), String> {
    let dirs = DataDirs::resolve().map_err(|error| error.to_string())?;
    let program = match std::env::var_os(DAEMON_PROGRAM_ENV).filter(|v| !v.is_empty()) {
        Some(path) => PathBuf::from(path),
        None => launcher::sibling_program().map_err(|error| format!("cannot locate the app: {error}"))?,
    };
    Ok((dirs, program))
}

/// What the UI shows about the background service.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DaemonStatus {
    running: bool,
    endpoint: String,
    version: Option<String>,
    build_id: Option<String>,
    pid: Option<u32>,
    uptime_secs: Option<u64>,
    /// Whether this app holds desktop access (the `ui` principal).
    desktop_access: bool,
    /// Whether this check started the service.
    started: bool,
    /// Whether a service from another build, or one started elsewhere, was replaced.
    replaced: bool,
    error: Option<String>,
}

impl DaemonStatus {
    fn failed(endpoint: String, error: impl ToString) -> Self {
        Self {
            running: false,
            endpoint,
            version: None,
            build_id: None,
            pid: None,
            uptime_secs: None,
            desktop_access: false,
            started: false,
            replaced: false,
            error: Some(error.to_string()),
        }
    }
}

/// Connect to the daemon, starting or replacing it as needed, and report on it.
#[tauri::command]
async fn daemon_status(bridge: State<'_, Bridge>) -> Result<DaemonStatus, String> {
    let launcher = match &bridge.launcher {
        Ok(launcher) => launcher,
        Err(error) => return Ok(DaemonStatus::failed(String::new(), error)),
    };
    let endpoint = launcher.endpoint().to_string();

    // Held for the whole check, so concurrent calls never start two daemons.
    let mut token = bridge.token.lock().await;
    let mut connected = match launcher
        .connect_or_start(ClientKind::Desktop, token.clone(), true)
        .await
    {
        Ok(connected) => connected,
        Err(error) => return Ok(DaemonStatus::failed(endpoint, explain(error))),
    };
    if let Some(launched) = &connected.launched {
        *token = Some(launched.ui_token.clone());
    }

    let status = match connected.client.health().await {
        Ok(health) => DaemonStatus {
            running: true,
            endpoint,
            version: Some(health.version),
            build_id: Some(health.build_id),
            pid: Some(health.pid),
            uptime_secs: Some(health.uptime_secs),
            desktop_access: connected.client.principal() == Principal::Ui,
            started: connected.launched.is_some(),
            replaced: connected.replaced,
            error: None,
        },
        Err(error) => DaemonStatus::failed(endpoint, error),
    };
    Ok(status)
}

fn explain(error: LaunchError) -> String {
    match error {
        LaunchError::ProgramMissing(path) => format!(
            "The Regards service program is missing ({}). In a development checkout, run \
             `cargo build -p regards-cli` first.",
            path.display()
        ),
        other => other.to_string(),
    }
}

fn main() {
    let app = tauri::Builder::default()
        // Must come first: a second app instance would replace this one's daemon.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .manage(Bridge::new())
        .invoke_handler(tauri::generate_handler![daemon_status])
        .build(tauri::generate_context!())
        .expect("failed to start the Regards desktop app");

    app.run(|app, event| {
        if let RunEvent::Exit = event {
            let bridge = app.state::<Bridge>();
            tauri::async_runtime::block_on(async {
                let _ = tokio::time::timeout(Duration::from_secs(3), bridge.stop_own_daemon()).await;
            });
        }
    });
}
