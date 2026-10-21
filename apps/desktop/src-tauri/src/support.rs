//! Settings › Support: detailed logging, and diagnostics exported to a file
//! the person chooses. Nothing is sent anywhere: the person shares the file
//! if they wish. It holds Regards' own log (which never contains message
//! content, passwords or tokens) and a few facts about this install, never
//! an address.

use crate::Bridge;
use crate::background::Background;
use regards_core::build_info::{BUILD_ID, VERSION};
use regards_core::paths::DataDirs;
use regards_ipc::protocol::{AccountSummary, ConnectionLogSettings, SignIn, methods};
use serde::Serialize;
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager, State};

/// Our own crates at debug; the libraries stay at info (and imap-next, which
/// would log passwords, is capped there whatever is asked; see the CLI's logging).
const DETAILED: &str = "info,regards_daemon=debug,regards_imap=debug,regards_smtp=debug,regards_store=debug,\
                        regards_net=debug,regards_ipc=debug";
/// The most of each log file a report keeps: its end.
const MAX_LOG: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SupportSettings {
    /// Takes effect when the service next starts.
    detailed_logs: bool,
    /// Kept by the service, and at once: commands and replies with the mail
    /// servers, never mail, passwords, tokens or addresses.
    connection_log: bool,
}

fn filter(on: bool) -> Option<String> {
    on.then(|| DETAILED.to_owned())
}

/// At start: daemons started from now on log as the setting says.
pub(crate) fn apply(app: &AppHandle) {
    let on = app.state::<Background>().detailed_logs();
    if let Ok(launcher) = &app.state::<Bridge>().launcher {
        launcher.set_log_filter(filter(on));
    }
}

#[tauri::command]
pub(crate) async fn support_settings(
    background: State<'_, Background>,
    bridge: State<'_, Bridge>,
) -> Result<SupportSettings, String> {
    Ok(SupportSettings {
        detailed_logs: background.detailed_logs(),
        connection_log: connection_log(&bridge).await,
    })
}

/// Whether the service logs connections; off when it cannot be asked.
async fn connection_log(bridge: &Bridge) -> bool {
    let Ok(connected) = bridge.connect().await else {
        return false;
    };
    let mut client = connected.client;
    client
        .call::<_, ConnectionLogSettings>(methods::CONNECTION_LOG_GET, Value::Null)
        .await
        .is_ok_and(|settings| settings.on)
}

/// Log connections to mail servers, or stop; at once.
#[tauri::command]
pub(crate) async fn set_connection_log(bridge: State<'_, Bridge>, on: bool) -> Result<(), String> {
    let mut client = bridge.connect().await?.client;
    let _: ConnectionLogSettings = client
        .call(methods::CONNECTION_LOG_SET, ConnectionLogSettings { on })
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// Saved; the service started next logs in detail (the running one keeps
/// its level until Regards restarts).
#[tauri::command]
pub(crate) fn set_detailed_logs(
    background: State<'_, Background>,
    bridge: State<'_, Bridge>,
    on: bool,
) -> Result<(), String> {
    background.set_detailed_logs(on)?;
    if let Ok(launcher) = &bridge.launcher {
        launcher.set_log_filter(filter(on));
    }
    Ok(())
}

/// Write the diagnostics to a file the person picks; its path, or `None`
/// if they cancelled.
#[tauri::command]
pub(crate) async fn export_diagnostics(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let report = report(&app).await;
    let name = format!("regards-diagnostics-{}.txt", day(now_secs()));
    let dialog = app
        .dialog()
        .file()
        .set_title("Export diagnostics")
        .set_file_name(&name)
        .add_filter("Text", &["txt"]);
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_save_file())
        .await
        .map_err(|error| error.to_string())?;
    let Some(path) = picked else { return Ok(None) };
    let path = path.into_path().map_err(|error| error.to_string())?;
    std::fs::write(&path, report).map_err(|error| format!("could not save the diagnostics: {error}"))?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

/// What the file says.
async fn report(app: &AppHandle) -> String {
    let mut out = format!(
        "Regards diagnostics, {} UTC\nThis file stays on your computer unless you share it. It holds no mail, \
         addresses, passwords or tokens.\n\nApp: {VERSION} ({BUILD_ID}) on {} {}\nDetailed logging: {}\n",
        timestamp(now_secs()),
        std::env::consts::OS,
        std::env::consts::ARCH,
        if app.state::<Background>().detailed_logs() {
            "on"
        } else {
            "off"
        },
    );
    out.push_str(&service(app).await);
    if let Ok(dirs) = DataDirs::resolve() {
        let log = dirs.daemon_log();
        let connections = dirs.connection_log();
        if connections.exists() {
            out.push_str(
                "The connection log below names your mail servers, as commands and replies; it holds no mail, \
                 addresses, passwords or tokens.\n",
            );
        }
        for (label, path) in [
            ("Service older log", log.with_extension("log.1")),
            ("Service log", log),
            ("Older connection log", connections.with_extension("log.1")),
            ("Connection log", connections),
        ] {
            if let Ok(bytes) = std::fs::read(&path) {
                let start = bytes.len().saturating_sub(MAX_LOG);
                out.push_str(&format!("\n--- {label} ---\n"));
                out.push_str(&String::from_utf8_lossy(&bytes[start..]));
            }
        }
    }
    out
}

/// The service's version and uptime, and how many accounts of each kind.
async fn service(app: &AppHandle) -> String {
    let bridge = app.state::<Bridge>();
    let Ok(connected) = bridge.connect().await else {
        return "Service: not reachable\n".into();
    };
    let mut client = connected.client;
    let health = match client.health().await {
        Ok(health) => format!(
            "Service: {} ({}), running for {} s\n",
            health.version, health.build_id, health.uptime_secs
        ),
        Err(error) => format!("Service: {error}\n"),
    };
    let accounts: Vec<AccountSummary> = client
        .call(methods::ACCOUNT_LIST, Value::Null)
        .await
        .unwrap_or_default();
    let count = |kind: SignIn| accounts.iter().filter(|account| account.sign_in == kind).count();
    format!(
        "{health}Accounts: {} (password {}, Microsoft {}, Google {})\n",
        accounts.len(),
        count(SignIn::Password),
        count(SignIn::Microsoft),
        count(SignIn::Google)
    )
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default()
}

/// `YYYY-MM-DD` of a Unix time, UTC.
fn day(secs: i64) -> String {
    // Howard Hinnant's days-to-civil.
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// `YYYY-MM-DD HH:MM` of a Unix time, UTC.
fn timestamp(secs: i64) -> String {
    let in_day = secs.rem_euclid(86_400);
    format!("{} {:02}:{:02}", day(secs), in_day / 3_600, in_day % 3_600 / 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_utc_days() {
        assert_eq!(day(0), "1970-01-01");
        assert_eq!(timestamp(1_792_486_800), "2026-10-20 09:00");
        assert_eq!(day(951_782_400), "2000-02-29");
    }
}
