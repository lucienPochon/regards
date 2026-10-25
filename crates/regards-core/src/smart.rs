//! The Smart Inbox's classifier (plan §5.4): which section a message goes in,
//! and why. Deterministic and explainable: the same rules give the section and
//! the answer to "why is this here?", from what the headers say, what the person
//! chose for the sender, and whether they have written to them. Nothing leaves
//! the device.

use serde::{Deserialize, Serialize};

/// What a message's headers say about how it was sent: bit flags, kept with
/// the message.
pub mod traits {
    /// From a mailing list: `List-Id`, `List-Unsubscribe` or `List-Post`.
    pub const LIST: u32 = 1;
    /// `Precedence: bulk`, `list` or `junk`.
    pub const BULK: u32 = 2;
    /// Sent by a program: `Auto-Submitted` (other than `no`) or an
    /// auto-reply header.
    pub const AUTO: u32 = 4;
    /// From an address that takes no replies (`noreply@`, `notifications@` …).
    pub const NOREPLY: u32 = 8;
    /// A delivery report or bounce (`multipart/report`, `mailer-daemon@`).
    pub const REPORT: u32 = 16;
    /// Sent through a bulk-mail service (`Feedback-ID` and the like).
    pub const FEEDBACK: u32 = 32;
}

/// The Smart Inbox's sections, in the order they are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    Priority,
    People,
    NewSenders,
    Notifications,
    Newsletters,
}

impl Section {
    pub const ALL: [Section; 5] = [
        Section::Priority,
        Section::People,
        Section::NewSenders,
        Section::Notifications,
        Section::Newsletters,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Priority => "priority",
            Self::People => "people",
            Self::NewSenders => "new_senders",
            Self::Notifications => "notifications",
            Self::Newsletters => "newsletters",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|section| section.as_str() == text)
    }
}

/// Where the person said a sender's mail goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Policy {
    People,
    Notifications,
    Newsletters,
}

impl Policy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::People => "people",
            Self::Notifications => "notifications",
            Self::Newsletters => "newsletters",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        [Self::People, Self::Notifications, Self::Newsletters]
            .into_iter()
            .find(|policy| policy.as_str() == text)
    }

    fn section(self) -> Section {
        match self {
            Self::People => Section::People,
            Self::Notifications => Section::Notifications,
            Self::Newsletters => Section::Newsletters,
        }
    }
}

/// What is known about a sender, for one account.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Sender {
    pub policy: Option<Policy>,
    pub priority: bool,
    /// The account has written to them.
    pub written_to: bool,
    /// The person blocked them: their mail goes to Junk (the section still
    /// says what it would be).
    pub blocked: bool,
    /// The person made them quiet: their mail comes in without a notification.
    pub quiet: bool,
}

/// How mail from new senders is shown (plan §5.4, screening).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Screening {
    /// No screening: new senders' mail shows with People.
    Off,
    /// In its own section, New senders.
    #[default]
    Section,
    /// Kept out of the inbox until the sender is accepted.
    Hold,
}

impl Screening {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Section => "section",
            Self::Hold => "hold",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        [Self::Off, Self::Section, Self::Hold]
            .into_iter()
            .find(|mode| mode.as_str() == text)
    }
}

/// The key a whole domain's choices are kept under: `@example.org`, for
/// `ada@example.org` or `@Example.org`. `None` for something that is not an
/// address or a domain.
pub fn domain_key(address: &str) -> Option<String> {
    let domain = address.trim().rsplit_once('@')?.1.trim().to_ascii_lowercase();
    (!domain.is_empty() && !domain.contains(char::is_whitespace)).then(|| format!("@{domain}"))
}

/// Why a message is in its section: the rule that placed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// The person chose this section for the sender.
    YourChoice,
    /// The person made the sender a priority.
    PrioritySender,
    /// It is from one of the account's own addresses.
    FromYou,
    /// A delivery report or bounce.
    DeliveryReport,
    /// A program sent it.
    SentAutomatically,
    /// Its address takes no replies.
    NoReplyAddress,
    /// It came through a mailing list or has an unsubscribe link.
    MailingList,
    /// It is marked as bulk mail.
    BulkMail,
    /// It came through a bulk-mail service.
    BulkSender,
    /// The account has written to the sender.
    YouWroteToThem,
    /// Nobody here has written to the sender yet.
    NewSender,
}

/// The section a message goes in, and why. The first rule that applies wins:
/// the person's choices, then their own mail, then what the headers say, then
/// whether they have written to the sender.
pub fn classify(traits: u32, sender: Sender, from_you: bool) -> (Section, Reason) {
    use traits::*;
    if let Some(policy) = sender.policy {
        return (policy.section(), Reason::YourChoice);
    }
    if sender.priority {
        return (Section::Priority, Reason::PrioritySender);
    }
    if from_you {
        return (Section::People, Reason::FromYou);
    }
    let rules = [
        (REPORT, Section::Notifications, Reason::DeliveryReport),
        (AUTO, Section::Notifications, Reason::SentAutomatically),
        (NOREPLY, Section::Notifications, Reason::NoReplyAddress),
        (LIST, Section::Newsletters, Reason::MailingList),
        (BULK, Section::Newsletters, Reason::BulkMail),
        (FEEDBACK, Section::Notifications, Reason::BulkSender),
    ];
    if let Some((_, section, reason)) = rules.into_iter().find(|(bit, _, _)| traits & bit != 0) {
        return (section, reason);
    }
    if sender.written_to {
        (Section::People, Reason::YouWroteToThem)
    } else {
        (Section::NewSenders, Reason::NewSender)
    }
}

/// Traits from header values: each argument is the raw value of that header,
/// if present. `from` is the sender's address.
#[derive(Debug, Clone, Copy, Default)]
pub struct HeaderValues<'a> {
    pub from: Option<&'a str>,
    pub list_id: Option<&'a str>,
    pub list_unsubscribe: Option<&'a str>,
    pub list_post: Option<&'a str>,
    pub precedence: Option<&'a str>,
    pub auto_submitted: Option<&'a str>,
    pub auto_response_suppress: Option<&'a str>,
    pub autoreply: Option<&'a str>,
    pub content_type: Option<&'a str>,
    pub feedback_id: Option<&'a str>,
}

/// The local parts of addresses that take no replies, or are a program's.
const NOREPLY_PARTS: &[&str] = &[
    "noreply",
    "no-reply",
    "no_reply",
    "donotreply",
    "do-not-reply",
    "do_not_reply",
    "notifications",
    "notification",
    "notify",
    "alerts",
    "alert",
];

pub fn traits_of(values: &HeaderValues<'_>) -> u32 {
    use traits::*;
    let present = |value: Option<&str>| value.is_some_and(|v| !v.trim().is_empty());
    let lower = |value: Option<&str>| value.map(|v| v.trim().to_ascii_lowercase()).unwrap_or_default();
    let mut found = 0;
    if present(values.list_id) || present(values.list_unsubscribe) || present(values.list_post) {
        found |= LIST;
    }
    if matches!(lower(values.precedence).as_str(), "bulk" | "list" | "junk") {
        found |= BULK;
    }
    let auto_submitted = lower(values.auto_submitted);
    if (!auto_submitted.is_empty() && auto_submitted != "no")
        || present(values.auto_response_suppress)
        || present(values.autoreply)
    {
        found |= AUTO;
    }
    let local = lower(values.from)
        .rsplit_once('@')
        .map(|(local, _)| local.to_owned())
        .unwrap_or_default();
    // "noreply", "no-reply+abc", "notifications-xyz": the start decides.
    if NOREPLY_PARTS
        .iter()
        .any(|part| local == *part || local.starts_with(&format!("{part}+")) || local.starts_with(&format!("{part}-")))
    {
        found |= NOREPLY;
    }
    if lower(values.content_type).starts_with("multipart/report")
        || matches!(local.as_str(), "mailer-daemon" | "postmaster")
    {
        found |= REPORT;
    }
    if present(values.feedback_id) {
        found |= FEEDBACK;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers<'a>(from: &'a str) -> HeaderValues<'a> {
        HeaderValues {
            from: Some(from),
            ..HeaderValues::default()
        }
    }

    /// A labelled corpus of header shapes seen in everyday mail.
    #[test]
    fn everyday_mail_lands_where_a_person_would_put_it() {
        use Section::*;
        let stranger = Sender::default();
        let known = Sender {
            written_to: true,
            ..Sender::default()
        };
        let cases: Vec<(&str, HeaderValues<'_>, Sender, Section)> = vec![
            ("a friend writes", headers("lena@example.net"), known, People),
            (
                "someone new writes",
                headers("hello@new-person.example"),
                stranger,
                NewSenders,
            ),
            (
                "a newsletter with an unsubscribe link",
                HeaderValues {
                    list_unsubscribe: Some("<mailto:u@news.example>"),
                    ..headers("news@shop.example")
                },
                stranger,
                Newsletters,
            ),
            (
                "a discussion list",
                HeaderValues {
                    list_id: Some("<garden.lists.example>"),
                    ..headers("ada@example.org")
                },
                known,
                Newsletters,
            ),
            (
                "bulk mail by Precedence",
                HeaderValues {
                    precedence: Some("Bulk"),
                    ..headers("offers@shop.example")
                },
                stranger,
                Newsletters,
            ),
            (
                "a receipt from a no-reply address, with an unsubscribe link",
                HeaderValues {
                    list_unsubscribe: Some("<https://shop.example/u>"),
                    ..headers("no-reply@shop.example")
                },
                stranger,
                Notifications,
            ),
            (
                "a code-review notification on a list",
                HeaderValues {
                    list_id: Some("<repo.example.github.com>"),
                    ..headers("notifications@github.example")
                },
                stranger,
                Notifications,
            ),
            (
                "an automatic reply",
                HeaderValues {
                    auto_submitted: Some("auto-replied"),
                    ..headers("lena@example.net")
                },
                known,
                Notifications,
            ),
            (
                "a bounce",
                HeaderValues {
                    content_type: Some("multipart/report; report-type=delivery-status"),
                    ..headers("MAILER-DAEMON@mx.example")
                },
                stranger,
                Notifications,
            ),
            (
                "a bank alert from a bulk service",
                HeaderValues {
                    feedback_id: Some("123:bank:alerts"),
                    ..headers("service@bank.example")
                },
                stranger,
                Notifications,
            ),
            (
                "Auto-Submitted: no is a person",
                HeaderValues {
                    auto_submitted: Some("no"),
                    ..headers("bob@example.org")
                },
                known,
                People,
            ),
            (
                "'notifications' inside a word is not a robot",
                headers("notificationsteam-lead@example.org"),
                known,
                People,
            ),
        ];
        let mut wrong = Vec::new();
        for (what, values, sender, expected) in &cases {
            let (section, _) = classify(traits_of(values), *sender, false);
            if section != *expected {
                wrong.push(format!("{what}: {section:?}, expected {expected:?}"));
            }
        }
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    #[test]
    fn the_persons_choices_come_first_and_their_own_mail_is_people() {
        let list = traits::LIST;
        let chose = Sender {
            policy: Some(Policy::People),
            ..Sender::default()
        };
        assert_eq!(classify(list, chose, false), (Section::People, Reason::YourChoice));
        let priority = Sender {
            priority: true,
            ..Sender::default()
        };
        assert_eq!(
            classify(list, priority, false),
            (Section::Priority, Reason::PrioritySender)
        );
        assert_eq!(
            classify(list, Sender::default(), true),
            (Section::People, Reason::FromYou)
        );
        assert_eq!(
            classify(0, Sender::default(), false),
            (Section::NewSenders, Reason::NewSender)
        );
    }

    #[test]
    fn traits_are_read_from_a_fetched_header_block() {
        let block = b"From: Shop <no-reply@shop.example>\r\nSubject: Your receipt\r\n\
            List-Unsubscribe: <https://shop.example/u>\r\nPrecedence: bulk\r\n\r\n";
        let found = crate::headers::parse(block).traits;
        assert_eq!(found, traits::NOREPLY | traits::LIST | traits::BULK);
        let plain = crate::headers::parse(b"From: Lena <lena@example.net>\r\nSubject: Hi\r\n\r\n");
        assert_eq!(plain.traits, 0);
    }

    #[test]
    fn sections_and_policies_read_back() {
        for section in Section::ALL {
            assert_eq!(Section::parse(section.as_str()), Some(section));
        }
        assert_eq!(Policy::parse("newsletters"), Some(Policy::Newsletters));
        assert_eq!(Policy::parse("priority"), None);
    }
}
