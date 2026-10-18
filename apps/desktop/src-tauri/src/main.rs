//! Regards desktop app: a Tauri 2 shell around the React UI.
//!
//! The WebView gets a deliberately narrow command set (ADR-0006): it never
//! talks to the daemon directly, only through the commands defined here.

// Release builds on Windows have no console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use regards_core::paths::DataDirs;
use regards_ipc::protocol::ClientKind;
use regards_ipc::{Client, ClientError, Endpoint};
use serde::Serialize;

/// What the UI shows about the background service.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DaemonStatus {
    running: bool,
    endpoint: String,
    version: Option<String>,
    build_id: Option<String>,
    same_build: Option<bool>,
    uptime_secs: Option<u64>,
    error: Option<String>,
}

impl DaemonStatus {
    fn not_running(endpoint: String) -> Self {
        Self {
            running: false,
            endpoint,
            version: None,
            build_id: None,
            same_build: None,
            uptime_secs: None,
            error: None,
        }
    }

    fn failed(endpoint: String, error: impl ToString) -> Self {
        Self {
            error: Some(error.to_string()),
            ..Self::not_running(endpoint)
        }
    }
}

#[tauri::command]
async fn daemon_status() -> DaemonStatus {
    let dirs = match DataDirs::resolve() {
        Ok(dirs) => dirs,
        Err(error) => return DaemonStatus::failed(String::new(), error),
    };
    let endpoint = Endpoint::for_dirs(&dirs);
    let mut client = match Client::connect(&endpoint, ClientKind::Desktop).await {
        Ok(client) => client,
        Err(ClientError::NotRunning { .. }) => return DaemonStatus::not_running(endpoint.to_string()),
        Err(error) => return DaemonStatus::failed(endpoint.to_string(), error),
    };
    let same_build = client.is_same_build();
    match client.health().await {
        Ok(health) => DaemonStatus {
            running: true,
            endpoint: endpoint.to_string(),
            version: Some(health.version),
            build_id: Some(health.build_id),
            same_build: Some(same_build),
            uptime_secs: Some(health.uptime_secs),
            error: None,
        },
        Err(error) => DaemonStatus::failed(endpoint.to_string(), error),
    }
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![daemon_status])
        .run(tauri::generate_context!())
        .expect("failed to start the Regards desktop app");
}
