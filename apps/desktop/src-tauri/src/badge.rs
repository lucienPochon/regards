//! The unread count on the taskbar or dock: the inboxes' unread mail (not
//! what is set aside or snoozed) of the accounts chosen, every one by
//! default, kept current with the change feed. Windows' taskbar takes no
//! number, only a small picture over the icon: there the count is drawn into
//! one.

use crate::Bridge;
use crate::background::Background;
use regards_core::mailbox::Role;
use regards_ipc::protocol::{AccountRef, AccountSummary, MailboxListResult, methods};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use tauri::{AppHandle, Manager, State};
use tokio::sync::Notify;

#[cfg(any(windows, test))]
mod draw;

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
    let shown = window.set_overlay_icon((count > 0).then(|| {
        // The taskbar shows it at its small-icon size: 16 pixels at 100 %.
        let scale = window.scale_factor().unwrap_or(1.0);
        let size = ((16.0 * scale).round() as u32).max(16);
        tauri::image::Image::new_owned(draw::badge(count, size), size, size)
    }));
    #[cfg(not(windows))]
    let shown = window.set_badge_count((count > 0).then_some(i64::from(count)));
    if let Err(error) = shown {
        eprintln!("badge: {error}");
    }
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
    fn the_badge_fills_its_square_and_leaves_the_corners_clear() {
        for (count, size) in [(1, 16), (47, 24), (100, 32), (7, 20)] {
            let pixels = draw::badge(count, size);
            assert_eq!(pixels.len(), (size * size * 4) as usize);
            let alpha = |x: u32, y: u32| pixels[((y * size + x) * 4 + 3) as usize];
            assert_eq!(alpha(0, 0), 0, "{count} at {size}");
            assert_eq!(alpha(size / 2, size - 2), 255, "{count} at {size}");
        }
    }

    #[test]
    fn past_ninety_nine_the_badge_stops_counting() {
        assert_eq!(draw::badge(100, 24), draw::badge(5000, 24));
        assert_ne!(draw::badge(99, 24), draw::badge(100, 24));
        assert_ne!(draw::badge(1, 16), draw::badge(7, 16));
    }

    #[test]
    fn every_account_counts_unless_some_are_chosen() {
        let mut choices = BadgeChoices::default();
        assert!(choices.on && choices.counts("work"));
        choices.accounts = Some(vec!["home".into()]);
        assert!(choices.counts("home") && !choices.counts("work"));
    }
}
