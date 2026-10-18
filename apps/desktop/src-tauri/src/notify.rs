//! Notifications (ADR-0011): a snooze ended, a reminder came due, a scheduled
//! message was held, a message was not sent. The daemon keeps these notices;
//! this shows the new ones and marks them shown.
//!
//! - **When:** after changes from the daemon, a moment later so a burst comes
//!   in one look, and once at start for what happened while the app was closed.
//! - **How many:** up to three, one each; more become one summary.
//! - **Not shown** while Regards' window has the focus (the app shows them), or
//!   when turned off in the tray. Either way they are marked shown.
//! - **What they say:** who and the subject, never the message's text.

use crate::Bridge;
use crate::background::Background;
use regards_core::headers::Person;
use regards_ipc::protocol::{NoticeKind, NoticeListResult, NoticeShownParams, NoticeView, methods};
use serde_json::Value;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::Notify;

/// More notices than this at once become one summary.
const ONE_EACH: usize = 3;
/// How long a burst of changes is given to settle before looking.
const SETTLE: Duration = Duration::from_millis(500);

#[derive(Default)]
pub(crate) struct Notifier {
    wake: Notify,
}

impl Notifier {
    /// Something changed: look for new notices soon.
    pub(crate) fn poke(&self) {
        self.wake.notify_one();
    }
}

pub(crate) async fn run(app: AppHandle) {
    loop {
        app.state::<Notifier>().wake.notified().await;
        tokio::time::sleep(SETTLE).await;
        if let Err(error) = show_new(&app).await {
            eprintln!("notifications: {error}");
        }
    }
}

async fn show_new(app: &AppHandle) -> Result<(), String> {
    let bridge = app.state::<Bridge>();
    let mut client = bridge.connect().await?.client;
    let listed: NoticeListResult = client
        .call(methods::NOTICE_LIST, Value::Null)
        .await
        .map_err(|error| error.to_string())?;
    let Some(up_to) = listed.notices.last().map(|notice| notice.id) else {
        return Ok(());
    };
    let focused = app
        .get_webview_window("main")
        .is_some_and(|window| window.is_visible().unwrap_or(false) && window.is_focused().unwrap_or(false));
    if app.state::<Background>().notifications() && !focused {
        for (title, body) in texts(&listed.notices) {
            if let Err(error) = app.notification().builder().title(title).body(body).show() {
                eprintln!("notification: {error}");
            }
        }
    }
    client
        .call::<_, Value>(methods::NOTICE_SHOWN, NoticeShownParams { up_to })
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// Title and body of each notification to show for `notices`.
fn texts(notices: &[NoticeView]) -> Vec<(String, String)> {
    if notices.len() > ONE_EACH {
        return vec![summary(notices)];
    }
    notices.iter().map(text).collect()
}

fn text(notice: &NoticeView) -> (String, String) {
    let subject = if notice.subject.trim().is_empty() {
        "(no subject)".to_owned()
    } else {
        notice.subject.clone()
    };
    let who = people(&notice.people);
    match notice.kind {
        NoticeKind::SnoozeEnded => ("Back from snooze".into(), format!("{who}: {subject}")),
        NoticeKind::ReminderDue => ("No reply yet".into(), format!("To {who}: {subject}")),
        NoticeKind::SendHeld => (
            "Waiting in the Outbox".into(),
            format!("“{subject}” was due while this computer was off. Send it now, or edit it."),
        ),
        NoticeKind::SendFailed => ("Not sent".into(), format!("“{subject}” to {who}. The Outbox says why.")),
        NoticeKind::SendUncertain => (
            "It may not have been sent".into(),
            format!("“{subject}” to {who}. Check before sending it again."),
        ),
    }
}

/// "3 back from snooze, 1 reminder, 1 message not sent".
fn summary(notices: &[NoticeView]) -> (String, String) {
    let count = |kinds: &[NoticeKind]| notices.iter().filter(|n| kinds.contains(&n.kind)).count();
    let parts: Vec<String> = [
        (
            count(&[NoticeKind::SnoozeEnded]),
            "back from snooze",
            "back from snooze",
        ),
        (count(&[NoticeKind::ReminderDue]), "reminder", "reminders"),
        (
            count(&[NoticeKind::SendHeld]),
            "message waiting in the Outbox",
            "messages waiting in the Outbox",
        ),
        (
            count(&[NoticeKind::SendFailed, NoticeKind::SendUncertain]),
            "message not sent",
            "messages not sent",
        ),
    ]
    .into_iter()
    .filter(|(n, _, _)| *n > 0)
    .map(|(n, one, many)| format!("{n} {}", if n == 1 { one } else { many }))
    .collect();
    ("Regards".into(), parts.join(", "))
}

/// "Lena Okafor", "Lena Okafor and Bob", "Lena Okafor and 2 others".
fn people(people: &[Person]) -> String {
    let name = |person: &Person| {
        person
            .name
            .clone()
            .filter(|name| !name.trim().is_empty())
            .or_else(|| person.address.clone())
            .unwrap_or_else(|| "someone".into())
    };
    match people {
        [] => "someone".into(),
        [one] => name(one),
        [one, two] => format!("{} and {}", name(one), name(two)),
        [one, rest @ ..] => format!("{} and {} others", name(one), rest.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice(kind: NoticeKind, subject: &str, who: &[&str]) -> NoticeView {
        NoticeView {
            id: 1,
            account_id: "a".into(),
            kind,
            message_id: None,
            outbox_id: None,
            people: who
                .iter()
                .map(|name| Person {
                    name: Some((*name).into()),
                    address: None,
                })
                .collect(),
            subject: subject.into(),
            created_at: 0,
        }
    }

    #[test]
    fn a_few_are_told_one_by_one_with_who_and_the_subject() {
        let shown = texts(&[
            notice(NoticeKind::SnoozeEnded, "Seeds", &["Lena"]),
            notice(NoticeKind::ReminderDue, "", &["Bob", "Ada", "Cy"]),
            notice(NoticeKind::SendFailed, "Rota", &["Lena", "Bob"]),
        ]);
        assert_eq!(
            shown,
            [
                ("Back from snooze".to_owned(), "Lena: Seeds".to_owned()),
                (
                    "No reply yet".to_owned(),
                    "To Bob and 2 others: (no subject)".to_owned()
                ),
                (
                    "Not sent".to_owned(),
                    "“Rota” to Lena and Bob. The Outbox says why.".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn many_at_once_become_one_summary() {
        let many = [
            notice(NoticeKind::SnoozeEnded, "a", &[]),
            notice(NoticeKind::SnoozeEnded, "b", &[]),
            notice(NoticeKind::SnoozeEnded, "c", &[]),
            notice(NoticeKind::ReminderDue, "d", &[]),
            notice(NoticeKind::SendUncertain, "e", &[]),
        ];
        assert_eq!(
            texts(&many),
            [(
                "Regards".to_owned(),
                "3 back from snooze, 1 reminder, 1 message not sent".to_owned()
            )]
        );
    }
}
