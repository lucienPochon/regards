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
use regards_core::secret::Secret;
use regards_ipc::launcher::{self, Connected, LaunchError, Launcher};
use regards_ipc::protocol::{
    AccountAddParams, AccountRemoveParams, AccountSummary, ClientKind, ConnectionSecurity, Principal, ServerSettings,
    methods,
};
use regards_ipc::{Client, ClientError, UiToken};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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

impl Bridge {
    /// Connect with desktop access, starting or replacing the daemon as needed.
    async fn connect(&self) -> Result<Connected, String> {
        let launcher = self.launcher.as_ref().map_err(Clone::clone)?;
        // Held while connecting, so concurrent calls never start two daemons.
        let mut token = self.token.lock().await;
        let connected = launcher
            .connect_or_start(ClientKind::Desktop, token.clone(), true)
            .await
            .map_err(explain)?;
        if let Some(launched) = &connected.launched {
            *token = Some(launched.ui_token.clone());
        }
        Ok(connected)
    }
}

/// Connect to the daemon, starting or replacing it as needed, and report on it.
#[tauri::command]
async fn daemon_status(bridge: State<'_, Bridge>) -> Result<DaemonStatus, String> {
    let endpoint = match &bridge.launcher {
        Ok(launcher) => launcher.endpoint().to_string(),
        Err(error) => return Ok(DaemonStatus::failed(String::new(), error)),
    };
    let mut connected = match bridge.connect().await {
        Ok(connected) => connected,
        Err(error) => return Ok(DaemonStatus::failed(endpoint, error)),
    };

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

/// The account form as the UI sends it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountInput {
    display_name: String,
    email: String,
    username: Option<String>,
    password: Secret,
    imap_host: String,
    imap_port: u16,
    smtp_host: String,
    smtp_port: u16,
    smtp_security: ConnectionSecurity,
}

#[tauri::command]
async fn account_list(bridge: State<'_, Bridge>) -> Result<Vec<AccountSummary>, String> {
    let mut client = bridge.connect().await?.client;
    client.call(methods::ACCOUNT_LIST, Value::Null).await.map_err(describe)
}

/// Check the sign-in with the server, then save the account (password in the OS credential store).
#[tauri::command]
async fn account_add(bridge: State<'_, Bridge>, input: AccountInput) -> Result<AccountSummary, String> {
    let smtp_host = match input.smtp_host.trim() {
        "" => input.imap_host.clone(),
        host => host.to_owned(),
    };
    let params = AccountAddParams {
        display_name: input.display_name,
        email: input.email,
        username: input.username,
        password: input.password,
        imap: ServerSettings {
            host: input.imap_host,
            port: input.imap_port,
            security: ConnectionSecurity::Tls,
        },
        smtp: ServerSettings {
            host: smtp_host,
            port: input.smtp_port,
            security: input.smtp_security,
        },
    };
    let mut client = bridge.connect().await?.client;
    client.call(methods::ACCOUNT_ADD, params).await.map_err(describe)
}

#[tauri::command]
async fn account_remove(bridge: State<'_, Bridge>, id: String) -> Result<(), String> {
    let mut client = bridge.connect().await?.client;
    let _: Value = client
        .call(methods::ACCOUNT_REMOVE, AccountRemoveParams { id })
        .await
        .map_err(describe)?;
    Ok(())
}

/// Daemon errors carry messages written for people; show those as they are.
fn describe(error: ClientError) -> String {
    match error {
        ClientError::Rpc(error) => error.message,
        other => other.to_string(),
    }
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
        .invoke_handler(tauri::generate_handler![
            daemon_status,
            account_list,
            account_add,
            account_remove
        ])
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
