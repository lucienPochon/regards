//! Notifications (ADR-0011, plan §5.12): new mail, a snooze ended, a reminder
//! came due, a scheduled message was held, a message was not sent, an agent
//! waits. The daemon keeps these; this shows the new ones the person's
//! choices let through ([`NotifyChoices`]) and marks them all shown.
//!
//! - **New mail,** per account: all of it, People and Priority (the default:
//!   no new senders, notifications or newsletters), Priority only, or none.
//! - **The other notices,** by kind, all on by default.
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
use regards_ipc::protocol::{
    ArrivalView, ArrivedResult, NoticeKind, NoticeListResult, NoticeShownParams, NoticeView, Section, methods,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::Notify;

/// More notices than this at once become one summary.
const ONE_EACH: usize = 3;
/// How long a burst of changes is given to settle before looking.
const SETTLE: Duration = Duration::from_millis(500);

/// Which new mail of an account to tell of.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NewMail {
    All,
    /// People and Priority: not new senders, notifications or newsletters.
    #[default]
    Smart,
    Priority,
    None,
}

impl NewMail {
    fn lets(self, section: Option<Section>) -> bool {
        match self {
            Self::All => true,
            Self::Smart => matches!(section, Some(Section::Priority | Section::People)),
            Self::Priority => section == Some(Section::Priority),
            Self::None => false,
        }
    }
}

/// What to tell of, kept in `desktop.json` (see [`crate::background`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct NotifyChoices {
    /// New mail, per account id; an account not here: [`NewMail::Smart`].
    pub(crate) new_mail: HashMap<String, NewMail>,
    pub(crate) snooze_ended: bool,
    pub(crate) reminder_due: bool,
    /// A message not sent, perhaps not sent, or held after the computer was off.
    pub(crate) sending: bool,
    /// An agent waits for the person, or is sending.
    pub(crate) agents: bool,
}

impl Default for NotifyChoices {
    fn default() -> Self {
        Self {
            new_mail: HashMap::new(),
            snooze_ended: true,
            reminder_due: true,
            sending: true,
            agents: true,
        }
    }
}

impl NotifyChoices {
    fn lets_notice(&self, kind: NoticeKind) -> bool {
        match kind {
            NoticeKind::SnoozeEnded => self.snooze_ended,
            NoticeKind::ReminderDue => self.reminder_due,
            NoticeKind::SendHeld | NoticeKind::SendFailed | NoticeKind::SendUncertain => self.sending,
            NoticeKind::AgentAsks | NoticeKind::AgentSending => self.agents,
        }
    }

    fn lets_mail(&self, arrival: &ArrivalView) -> bool {
        self.new_mail
            .get(&arrival.account_id)
            .copied()
            .unwrap_or_default()
            .lets(arrival.section)
    }
}

#[tauri::command]
pub(crate) fn notification_settings(background: State<'_, Background>) -> NotifyChoices {
    background.notify_choices()
}

#[tauri::command]
pub(crate) fn set_notification_settings(
    background: State<'_, Background>,
    settings: NotifyChoices,
) -> Result<(), String> {
    background.set_notify_choices(settings)
}

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
    let arrived: ArrivedResult = client
        .call(methods::MAIL_ARRIVED, Value::Null)
        .await
        .map_err(|error| error.to_string())?;
    if listed.notices.is_empty() && arrived.arrivals.is_empty() {
        return Ok(());
    }
    let focused = app
        .get_webview_window("main")
        .is_some_and(|window| window.is_visible().unwrap_or(false) && window.is_focused().unwrap_or(false));
    let background = app.state::<Background>();
    if background.notifications() && !focused {
        let choices = background.notify_choices();
        let notices: Vec<NoticeView> = listed
            .notices
            .iter()
            .filter(|notice| choices.lets_notice(notice.kind))
            .cloned()
            .collect();
        let mail: Vec<ArrivalView> = arrived
            .arrivals
            .iter()
            .filter(|arrival| choices.lets_mail(arrival))
            .cloned()
            .collect();
        for (title, body) in texts(&notices, &mail) {
            if let Err(error) = app.notification().builder().title(title).body(body).show() {
                eprintln!("notification: {error}");
            }
        }
    }
    // All looked at, told of or not.
    if let Some(up_to) = listed.notices.last().map(|notice| notice.id) {
        client
            .call::<_, Value>(methods::NOTICE_SHOWN, NoticeShownParams { up_to })
            .await
            .map_err(|error| error.to_string())?;
    }
    if let Some(up_to) = arrived.arrivals.last().map(|arrival| arrival.id) {
        client
            .call::<_, Value>(methods::MAIL_ARRIVED_SHOWN, NoticeShownParams { up_to })
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Title and body of each notification to show for `notices`.
fn texts(notices: &[NoticeView], mail: &[ArrivalView]) -> Vec<(String, String)> {
    if notices.len() + mail.len() > ONE_EACH {
        return vec![summary(notices, mail.len())];
    }
    mail.iter().map(mail_text).chain(notices.iter().map(text)).collect()
}

/// New mail: who, and the subject.
fn mail_text(arrival: &ArrivalView) -> (String, String) {
    let subject = if arrival.subject.trim().is_empty() {
        "(no subject)".to_owned()
    } else {
        arrival.subject.clone()
    };
    (people(arrival.from.as_slice()), subject)
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
        NoticeKind::AgentAsks => (
            "An agent is waiting for you".into(),
            format!("{subject}. Approve or reject it in Settings › Agents."),
        ),
        NoticeKind::AgentSending => (
            "An agent is sending a message".into(),
            format!("“{subject}” to {who}. Call it back from the Outbox before it goes."),
        ),
    }
}

/// "2 new messages, 3 back from snooze, 1 reminder, 1 message not sent".
fn summary(notices: &[NoticeView], new_mail: usize) -> (String, String) {
    let count = |kinds: &[NoticeKind]| notices.iter().filter(|n| kinds.contains(&n.kind)).count();
    let parts: Vec<String> = [
        (new_mail, "new message", "new messages"),
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
        (
            count(&[NoticeKind::AgentAsks]),
            "agent request waiting for you",
            "agent requests waiting for you",
        ),
        (
            count(&[NoticeKind::AgentSending]),
            "message an agent is sending",
            "messages an agent is sending",
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
        let shown = texts(
            &[
                notice(NoticeKind::SnoozeEnded, "Seeds", &["Lena"]),
                notice(NoticeKind::ReminderDue, "", &["Bob", "Ada", "Cy"]),
                notice(NoticeKind::SendFailed, "Rota", &["Lena", "Bob"]),
            ],
            &[],
        );
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

    fn arrival(account: &str, section: Option<Section>, subject: &str) -> ArrivalView {
        ArrivalView {
            id: 1,
            message_id: 1,
            account_id: account.into(),
            from: Some(Person {
                name: Some("Lena Okafor".into()),
                address: Some("lena@example.net".into()),
            }),
            subject: subject.into(),
            section,
            received_at: 0,
        }
    }

    #[test]
    fn new_mail_is_told_as_each_account_chose() {
        let mut choices = NotifyChoices::default();
        let person = arrival("work", Some(Section::People), "Seeds");
        let newsletter = arrival("work", Some(Section::Newsletters), "Sale");
        let priority = arrival("home", Some(Section::Priority), "Call me");
        let stranger = arrival("home", Some(Section::NewSenders), "Hi");
        // Smart by default: People and Priority.
        assert!(choices.lets_mail(&person) && choices.lets_mail(&priority));
        assert!(!choices.lets_mail(&newsletter) && !choices.lets_mail(&stranger));

        choices.new_mail.insert("work".into(), NewMail::All);
        choices.new_mail.insert("home".into(), NewMail::Priority);
        assert!(choices.lets_mail(&newsletter));
        assert!(choices.lets_mail(&priority) && !choices.lets_mail(&stranger));
        choices.new_mail.insert("home".into(), NewMail::None);
        assert!(!choices.lets_mail(&priority));

        assert!(choices.lets_notice(NoticeKind::SendFailed));
        choices.sending = false;
        assert!(!choices.lets_notice(NoticeKind::SendUncertain) && choices.lets_notice(NoticeKind::AgentAsks));

        assert_eq!(
            texts(&[], std::slice::from_ref(&person)),
            [("Lena Okafor".to_owned(), "Seeds".to_owned())]
        );
        assert_eq!(
            texts(
                &[notice(NoticeKind::SnoozeEnded, "a", &[])],
                &[person.clone(), person.clone(), person]
            ),
            [("Regards".to_owned(), "3 new messages, 1 back from snooze".to_owned())]
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
            texts(&many, &[]),
            [(
                "Regards".to_owned(),
                "3 back from snooze, 1 reminder, 1 message not sent".to_owned()
            )]
        );
    }
}
