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
//! - **A click** brings Regards to the front on the message a notification is
//!   about: new mail, back from snooze, no reply yet (Windows; elsewhere the
//!   plugin cannot tell of clicks yet).

use crate::Bridge;
use crate::background::Background;
use crate::lang::{tr, tr_with};
use regards_core::headers::Person;
use regards_ipc::protocol::{
    ArrivalView, ArrivedResult, NoticeKind, NoticeListResult, NoticeShownParams, NoticeView, Section, methods,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;
use tauri::{AppHandle, Manager, State};
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
        for shown in texts(&notices, &mail) {
            if let Err(error) = show(app, shown) {
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

/// One notification: what it says, and the message a click on it opens
/// (`None`: just Regards).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Shown {
    title: String,
    body: String,
    opens: Option<i64>,
}

/// What a click on a notification does: Regards comes to the front, on the
/// message it is about if there is one.
#[cfg_attr(not(windows), allow(dead_code))]
fn clicked(app: &AppHandle, opens: Option<i64>) {
    match opens {
        Some(id) => crate::links::open(app, [format!("regards://message/{id}")], false),
        None => crate::background::show(app),
    }
}

/// On Windows, through the toast API itself: the plugin cannot tell when a
/// notification was clicked.
#[cfg(windows)]
fn show(app: &AppHandle, shown: Shown) -> Result<(), String> {
    use tauri_winrt_notification::Toast;
    // As the plugin does: the installed app's id, or PowerShell's for a build
    // run from target, which has none registered.
    let from_target = tauri::utils::platform::current_exe().ok().is_some_and(|exe| {
        exe.parent().is_some_and(|dir| {
            dir.ends_with(std::path::Path::new("target").join("debug"))
                || dir.ends_with(std::path::Path::new("target").join("release"))
        })
    });
    let app_id = if from_target {
        Toast::POWERSHELL_APP_ID.to_owned()
    } else {
        app.config().identifier.clone()
    };
    let (handle, opens) = (app.clone(), shown.opens);
    Toast::new(&app_id)
        .title(&shown.title)
        .text1(&shown.body)
        .on_activated(move |_| {
            clicked(&handle, opens);
            Ok(())
        })
        .show()
        .map_err(|error| error.to_string())
}

/// Elsewhere through the plugin, which opens nothing on a click yet.
#[cfg(not(windows))]
fn show(app: &AppHandle, shown: Shown) -> Result<(), String> {
    use tauri_plugin_notification::NotificationExt;
    app.notification()
        .builder()
        .title(shown.title)
        .body(shown.body)
        .show()
        .map_err(|error| error.to_string())
}

/// Each notification to show for `notices` and `mail`.
fn texts(notices: &[NoticeView], mail: &[ArrivalView]) -> Vec<Shown> {
    if notices.len() + mail.len() > ONE_EACH {
        return vec![summary(notices, mail.len())];
    }
    mail.iter().map(mail_text).chain(notices.iter().map(text)).collect()
}

/// New mail: who, and the subject.
fn mail_text(arrival: &ArrivalView) -> Shown {
    let subject = if arrival.subject.trim().is_empty() {
        "(no subject)".to_owned()
    } else {
        arrival.subject.clone()
    };
    Shown {
        title: people(arrival.from.as_slice()),
        body: subject,
        opens: Some(arrival.message_id),
    }
}

fn text(notice: &NoticeView) -> Shown {
    let subject = if notice.subject.trim().is_empty() {
        tr("(no subject)").to_owned()
    } else {
        notice.subject.clone()
    };
    let who = people(&notice.people);
    let said = |text: &'static str| tr_with(text, &[("who", &who), ("subject", &subject)]);
    let (title, body): (String, String) = match notice.kind {
        NoticeKind::SnoozeEnded => (tr("Back from snooze").into(), format!("{who}: {subject}")),
        NoticeKind::ReminderDue => (tr("No reply yet").into(), said("To {who}: {subject}")),
        NoticeKind::SendHeld => (
            tr("Waiting in the Outbox").into(),
            said("“{subject}” was due while this computer was off. Send it now, or edit it."),
        ),
        NoticeKind::SendFailed => (
            tr("Not sent").into(),
            said("“{subject}” to {who}. The Outbox says why."),
        ),
        NoticeKind::SendUncertain => (
            tr("It may not have been sent").into(),
            said("“{subject}” to {who}. Check before sending it again."),
        ),
        NoticeKind::AgentAsks => (
            tr("An agent is waiting for you").into(),
            said("{subject}. Approve or reject it in Settings › Agents."),
        ),
        NoticeKind::AgentSending => (
            tr("An agent is sending a message").into(),
            said("“{subject}” to {who}. Call it back from the Outbox before it goes."),
        ),
    };
    // Back from snooze, or no reply yet: that message. The others are about
    // the Outbox or an agent, and open Regards as it is.
    let opens = match notice.kind {
        NoticeKind::SnoozeEnded | NoticeKind::ReminderDue => notice.message_id,
        _ => None,
    };
    Shown { title, body, opens }
}

/// "2 new messages, 3 back from snooze, 1 reminder, 1 message not sent".
fn summary(notices: &[NoticeView], new_mail: usize) -> Shown {
    let count = |kinds: &[NoticeKind]| notices.iter().filter(|n| kinds.contains(&n.kind)).count();
    let parts: Vec<String> = [
        (new_mail, "1 new message", "{count} new messages"),
        (
            count(&[NoticeKind::SnoozeEnded]),
            "1 back from snooze",
            "{count} back from snooze",
        ),
        (count(&[NoticeKind::ReminderDue]), "1 reminder", "{count} reminders"),
        (
            count(&[NoticeKind::SendHeld]),
            "1 message waiting in the Outbox",
            "{count} messages waiting in the Outbox",
        ),
        (
            count(&[NoticeKind::SendFailed, NoticeKind::SendUncertain]),
            "1 message not sent",
            "{count} messages not sent",
        ),
        (
            count(&[NoticeKind::AgentAsks]),
            "1 agent request waiting for you",
            "{count} agent requests waiting for you",
        ),
        (
            count(&[NoticeKind::AgentSending]),
            "1 message an agent is sending",
            "{count} messages an agent is sending",
        ),
    ]
    .into_iter()
    .filter(|(n, _, _)| *n > 0)
    .map(|(n, one, many)| crate::lang::count(n, one, many))
    .collect();
    Shown {
        title: "Regards".into(),
        body: parts.join(", "),
        opens: None,
    }
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
        [one, two] => tr_with("{one} and {two}", &[("one", &name(one)), ("two", &name(two))]),
        [one, rest @ ..] => tr_with(
            "{one} and {count} others",
            &[("one", &name(one)), ("count", &rest.len().to_string())],
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the notifications say, without where they lead.
    fn said(shown: Vec<Shown>) -> Vec<(String, String)> {
        shown.into_iter().map(|s| (s.title, s.body)).collect()
    }

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
        let shown = said(texts(
            &[
                notice(NoticeKind::SnoozeEnded, "Seeds", &["Lena"]),
                notice(NoticeKind::ReminderDue, "", &["Bob", "Ada", "Cy"]),
                notice(NoticeKind::SendFailed, "Rota", &["Lena", "Bob"]),
            ],
            &[],
        ));
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
            said(texts(&[], std::slice::from_ref(&person))),
            [("Lena Okafor".to_owned(), "Seeds".to_owned())]
        );
        assert_eq!(
            said(texts(
                &[notice(NoticeKind::SnoozeEnded, "a", &[])],
                &[person.clone(), person.clone(), person]
            )),
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
            said(texts(&many, &[])),
            [(
                "Regards".to_owned(),
                "3 back from snooze, 1 reminder, 1 message not sent".to_owned()
            )]
        );
    }

    #[test]
    fn a_click_opens_the_message_it_is_about() {
        let mut snoozed = notice(NoticeKind::SnoozeEnded, "Seeds", &[]);
        snoozed.message_id = Some(7);
        let mut failed = notice(NoticeKind::SendFailed, "Rota", &[]);
        failed.outbox_id = Some(3);
        let mut mail = arrival("work", Some(Section::People), "Hi");
        mail.message_id = 9;
        let opens: Vec<Option<i64>> = texts(&[snoozed.clone(), failed], std::slice::from_ref(&mail))
            .into_iter()
            .map(|shown| shown.opens)
            .collect();
        assert_eq!(opens, [Some(9), Some(7), None], "the Outbox ones open Regards as it is");
        // A summary is about several: Regards, as it is.
        let many = [snoozed.clone(), snoozed.clone(), snoozed.clone(), snoozed];
        assert_eq!(texts(&many, &[])[0].opens, None);
    }
}
