//! Update checks (plan Phase 7): off until the person turns them on, as the
//! privacy statement promises, because a check tells GitHub this computer's
//! address. On, Regards asks once a day for the latest release; a newer
//! version is told once, and its page opens in the browser for the
//! download. The installer stops the running service itself
//! (`windows/hooks.nsh`), and the new app replaces an older daemon on its
//! first connection (ADR-0005).

use crate::background::Background;
use regards_core::build_info::{VERSION, is_newer};
use regards_net::TlsOptions;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_notification::NotificationExt;

/// Where the latest release is asked for; a fork's build can point elsewhere.
const LATEST: &str = match option_env!("REGARDS_RELEASES_API") {
    Some(url) => url,
    None => "https://api.github.com/repos/lucienPochon/regards/releases/latest",
};
/// Only release pages there are opened.
const RELEASE_PAGES: &str = match option_env!("REGARDS_RELEASE_PAGES") {
    Some(prefix) => prefix,
    None => "https://github.com/lucienPochon/regards/releases/",
};
const DAY_MS: i64 = 24 * 3600 * 1000;
/// After the app starts, before the first check: the network may not be up.
const FIRST_WAIT: Duration = Duration::from_secs(2 * 60);
/// How often whether a day has passed is looked at (sleep stops timers).
const LOOK_EVERY: Duration = Duration::from_secs(30 * 60);

/// A later version than this one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Update {
    version: String,
    /// Its release page, for the download.
    url: String,
    published_at: Option<String>,
}

/// What the app shows in its settings.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct UpdateStatus {
    /// `None`: not asked yet; ask once.
    checks: Option<bool>,
    current: &'static str,
    available: Option<Update>,
    /// Unix ms of the last check, this run.
    checked_at: Option<i64>,
}

#[derive(Default)]
pub(crate) struct Updates {
    found: Mutex<Option<Update>>,
    checked_at: Mutex<Option<i64>>,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}

/// Ask for the latest release; a later one than this, if there is one.
async fn check() -> Result<Option<Update>, String> {
    let answer = regards_net::https::get(LATEST, &TlsOptions::platform())
        .await
        .map_err(|error| format!("could not check for updates: {error}"))?;
    read_release(answer.status, &answer.body, VERSION)
}

/// GitHub's answer for the latest release: a later one than `current`, if any.
fn read_release(status: u16, body: &[u8], current: &str) -> Result<Option<Update>, String> {
    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
        html_url: String,
        published_at: Option<String>,
        #[serde(default)]
        draft: bool,
        #[serde(default)]
        prerelease: bool,
    }
    match status {
        200 => {}
        // No release published yet.
        404 => return Ok(None),
        status => return Err(format!("could not check for updates: GitHub answered {status}")),
    }
    let release: Release =
        serde_json::from_slice(body).map_err(|_| "could not check for updates: an unreadable answer".to_owned())?;
    let wanted = !release.draft
        && !release.prerelease
        && release.html_url.starts_with(RELEASE_PAGES)
        && is_newer(&release.tag_name, current);
    Ok(wanted.then(|| Update {
        version: release.tag_name.trim_start_matches('v').to_owned(),
        url: release.html_url,
        published_at: release.published_at,
    }))
}

/// Check, keep what was found, and tell of a version not told of yet.
async fn check_and_tell(app: &AppHandle) -> Result<Option<Update>, String> {
    let found = check().await?;
    let updates = app.state::<Updates>();
    *updates.checked_at.lock().expect("never poisoned") = Some(now_ms());
    *updates.found.lock().expect("never poisoned") = found.clone();
    if let Some(update) = &found {
        let background = app.state::<Background>();
        if background.told_update().as_deref() != Some(update.version.as_str()) {
            background.set_told_update(&update.version);
            let _ = app.emit("update_available", update);
            if background.notifications() {
                let _ = app
                    .notification()
                    .builder()
                    .title(format!("Regards {} is available", update.version))
                    .body("Open Settings › About to download it.")
                    .show();
            }
        }
    }
    Ok(found)
}

/// In the background: once a day while checks are on.
pub(crate) async fn run(app: AppHandle) {
    tokio::time::sleep(FIRST_WAIT).await;
    loop {
        let due = {
            let checked = *app.state::<Updates>().checked_at.lock().expect("never poisoned");
            checked.is_none_or(|at| now_ms() - at >= DAY_MS)
        };
        if app.state::<Background>().update_checks() == Some(true)
            && due
            && let Err(error) = check_and_tell(&app).await
        {
            eprintln!("{error}");
        }
        tokio::time::sleep(LOOK_EVERY).await;
    }
}

#[tauri::command]
pub(crate) fn update_status(background: State<'_, Background>, updates: State<'_, Updates>) -> UpdateStatus {
    UpdateStatus {
        checks: background.update_checks(),
        current: VERSION,
        available: updates.found.lock().expect("never poisoned").clone(),
        checked_at: *updates.checked_at.lock().expect("never poisoned"),
    }
}

/// Check once a day, or never; the person's answer to the one question.
#[tauri::command]
pub(crate) fn update_set_checks(background: State<'_, Background>, on: bool) -> Result<(), String> {
    background.set_update_checks(on)
}

/// Check now: the person asked, whatever the setting.
#[tauri::command]
pub(crate) async fn update_check(app: AppHandle) -> Result<Option<Update>, String> {
    check_and_tell(&app).await
}

/// Open the found release's page in the browser.
#[tauri::command]
pub(crate) fn update_open(updates: State<'_, Updates>) -> Result<(), String> {
    let url = updates
        .found
        .lock()
        .expect("never poisoned")
        .as_ref()
        .map(|update| update.url.clone())
        .ok_or("no update was found")?;
    open::that_detached(&url).map_err(|error| format!("could not open the browser: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, url: &str, prerelease: bool) -> Vec<u8> {
        format!(
            r#"{{"tag_name":"{tag}","html_url":"{url}","published_at":"2026-11-02T09:00:00Z","draft":false,"prerelease":{prerelease},"assets":[]}}"#
        )
        .into_bytes()
    }

    /// Against GitHub itself: the request is one it answers.
    #[tokio::test]
    #[ignore = "network: cargo test -p regards-desktop -- --ignored github"]
    async fn github_answers_the_check() {
        let found = check().await;
        assert!(found.is_ok(), "{found:?}");
        println!("{found:?}");
    }

    #[test]
    fn only_a_later_release_of_this_project_is_offered() {
        let page = format!("{RELEASE_PAGES}tag/v0.2.0");
        let later = read_release(200, &release("v0.2.0", &page, false), "0.1.0")
            .unwrap()
            .unwrap();
        assert_eq!((later.version.as_str(), later.url.as_str()), ("0.2.0", page.as_str()));
        assert_eq!(read_release(200, &release("v0.1.0", &page, false), "0.1.0"), Ok(None));
        assert_eq!(
            read_release(200, &release("v0.2.0", &page, true), "0.1.0"),
            Ok(None),
            "a pre-release"
        );
        assert_eq!(
            read_release(200, &release("v0.2.0", "https://example.test/evil", false), "0.1.0"),
            Ok(None),
            "a page elsewhere is never opened"
        );
        assert_eq!(read_release(404, b"{}", "0.1.0"), Ok(None), "nothing released yet");
        assert!(read_release(403, b"{}", "0.1.0").is_err());
        assert!(read_release(200, b"<html>", "0.1.0").is_err());
    }
}
