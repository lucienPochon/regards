//! The unread count on the taskbar or dock: the inboxes' unread mail (not
//! what is set aside or snoozed) of the accounts chosen, every one by
//! default, kept current with the change feed. Windows' taskbar takes no
//! number: there it is a dot while anything is unread.

use crate::Bridge;
use crate::background::Background;
use regards_core::mailbox::Role;
use regards_ipc::protocol::{AccountRef, AccountSummary, MailboxListResult, methods};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use tauri::{AppHandle, Manager, State};
use tokio::sync::Notify;

/// How long a burst of changes is given to settle before counting.
const SETTLE: Duration = Duration::from_millis(700);

/// Kept in `desktop.json` (see [`crate::background`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct BadgeChoices {
    pub(crate) on: bool,
    /// The accounts counted; `None`: every one.
    pub(crate) accounts: Option<Vec<String>>,
}

impl Default for BadgeChoices {
    fn default() -> Self {
        Self {
            on: true,
            accounts: None,
        }
    }
}

impl BadgeChoices {
    fn counts(&self, account_id: &str) -> bool {
        self.accounts
            .as_ref()
            .is_none_or(|accounts| accounts.iter().any(|id| id == account_id))
    }
}

#[derive(Default)]
pub(crate) struct Badge {
    wake: Notify,
}

impl Badge {
    /// Something changed: count again soon.
    pub(crate) fn poke(&self) {
        self.wake.notify_one();
    }
}

pub(crate) async fn run(app: AppHandle) {
    loop {
        app.state::<Badge>().wake.notified().await;
        tokio::time::sleep(SETTLE).await;
        let choices = app.state::<Background>().badge_choices();
        let count = if choices.on {
            match unread(&app, &choices).await {
                Ok(count) => count,
                Err(error) => {
                    eprintln!("badge: {error}");
                    continue;
                }
            }
        } else {
            0
        };
        show(&app, count);
    }
}

async fn unread(app: &AppHandle, choices: &BadgeChoices) -> Result<u32, String> {
    let bridge = app.state::<Bridge>();
    let mut client = bridge.connect().await?.client;
    let accounts: Vec<AccountSummary> = client
        .call(methods::ACCOUNT_LIST, Value::Null)
        .await
        .map_err(|error| error.to_string())?;
    let mut total = 0;
    for account in accounts.iter().filter(|account| choices.counts(&account.id)) {
        let listed: MailboxListResult = client
            .call(
                methods::MAILBOX_LIST,
                AccountRef {
                    account_id: account.id.clone(),
                },
            )
            .await
            .map_err(|error| error.to_string())?;
        total += listed
            .mailboxes
            .iter()
            .filter(|mailbox| mailbox.role == Some(Role::Inbox))
            .map(|mailbox| mailbox.unread)
            .sum::<u32>();
    }
    Ok(total)
}

fn show(app: &AppHandle, count: u32) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    #[cfg(windows)]
    let shown = window.set_overlay_icon((count > 0).then(dot));
    #[cfg(not(windows))]
    let shown = window.set_badge_count((count > 0).then_some(i64::from(count)));
    if let Err(error) = shown {
        eprintln!("badge: {error}");
    }
}

/// A 16-pixel dot in Regards' green with a white ring, for Windows' taskbar.
#[cfg(windows)]
fn dot() -> tauri::image::Image<'static> {
    const SIZE: u32 = 16;
    let mut pixels = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    let center = (SIZE as f32 - 1.0) / 2.0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let distance = ((x as f32 - center).powi(2) + (y as f32 - center).powi(2)).sqrt();
            let pixel: [u8; 4] = if distance <= 5.5 {
                [0x1d, 0x7a, 0x61, 0xff]
            } else if distance <= 7.5 {
                [0xff, 0xff, 0xff, 0xff]
            } else {
                [0, 0, 0, 0]
            };
            pixels.extend_from_slice(&pixel);
        }
    }
    tauri::image::Image::new_owned(pixels, SIZE, SIZE)
}

#[tauri::command]
pub(crate) fn badge_settings(background: State<'_, Background>) -> BadgeChoices {
    background.badge_choices()
}

#[tauri::command]
pub(crate) fn set_badge_settings(
    background: State<'_, Background>,
    badge: State<'_, Badge>,
    settings: BadgeChoices,
) -> Result<(), String> {
    background.set_badge_choices(settings)?;
    badge.poke();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_account_counts_unless_some_are_chosen() {
        let mut choices = BadgeChoices::default();
        assert!(choices.on && choices.counts("work"));
        choices.accounts = Some(vec!["home".into()]);
        assert!(choices.counts("home") && !choices.counts("work"));
    }
}
