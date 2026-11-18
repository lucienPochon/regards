//! The message headers Regards shows in lists and uses for threading.
//!
//! Parsed with `mail-parser`, which decodes RFC 2047 encoded words and
//! legacy charsets and never panics on malformed input.

use mail_parser::{Address, HeaderValue, MessageParser};
use serde::{Deserialize, Serialize};

/// A sender or recipient.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Person {
    pub name: Option<String>,
    pub address: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Headers {
    pub subject: String,
    pub from: Option<Person>,
    pub to: Vec<Person>,
    pub cc: Vec<Person>,
    /// The Date header, Unix time in milliseconds.
    pub date: Option<i64>,
    /// Message-ID without angle brackets.
    pub message_id: Option<String>,
    pub in_reply_to: Vec<String>,
    pub references: Vec<String>,
    /// Where replies should go, when the sender asked for somewhere else.
    #[serde(default)]
    pub reply_to: Vec<Person>,
    /// Only in drafts' copies: mail as sent carries no Bcc.
    #[serde(default)]
    pub bcc: Vec<Person>,
    /// The addresses `Delivered-To`, `X-Original-To` and `X-Forwarded-To`
    /// name: where the mail went, which a forwarder may keep out of `To`.
    #[serde(default)]
    pub delivered_to: Vec<String>,
    /// How it was sent, for the Smart Inbox ([`crate::smart::traits`]).
    #[serde(default)]
    pub traits: u32,
}

impl Headers {
    /// The addresses the message was written to: the people in `To` and `Cc`,
    /// then the delivery headers' (a forwarder may keep the address it went
    /// to out of `To`). Each once, first mention first: a reply goes out from
    /// the first of these that is one of the accounts'.
    pub fn written_to(&self) -> Vec<String> {
        let mut found: Vec<String> = Vec::new();
        let people = self.to.iter().chain(&self.cc).filter_map(|who| who.address.as_deref());
        let delivered = self.delivered_to.iter().map(String::as_str);
        for address in people.chain(delivered) {
            let address = address.trim();
            if address.contains('@') && !found.iter().any(|other| other.eq_ignore_ascii_case(address)) {
                found.push(address.to_owned());
            }
        }
        found
    }
}

/// Parse a header block (for example `BODY[HEADER.FIELDS (…)]`). Never fails:
/// anything unreadable is left empty.
pub fn parse(raw: &[u8]) -> Headers {
    let Some(message) = MessageParser::default().parse_headers(raw) else {
        return Headers::default();
    };
    Headers {
        subject: message.subject().map(clean).unwrap_or_default(),
        from: message.from().and_then(Address::first).map(person),
        to: people(message.to()),
        cc: people(message.cc()),
        date: message.date().map(|date| date.to_timestamp().saturating_mul(1000)),
        message_id: message.message_id().map(str::to_owned),
        in_reply_to: ids(message.in_reply_to()),
        references: ids(message.references()),
        reply_to: people(message.reply_to()),
        bcc: people(message.bcc()),
        delivered_to: delivered_to(&message),
        traits: crate::smart::traits_of(&crate::smart::HeaderValues {
            from: message.from().and_then(Address::first).and_then(|addr| addr.address()),
            list_id: message.header_raw("List-Id"),
            list_unsubscribe: message.header_raw("List-Unsubscribe"),
            list_post: message.header_raw("List-Post"),
            precedence: message.header_raw("Precedence"),
            auto_submitted: message.header_raw("Auto-Submitted"),
            auto_response_suppress: message.header_raw("X-Auto-Response-Suppress"),
            autoreply: message.header_raw("X-Autoreply"),
            content_type: message.header_raw("Content-Type"),
            feedback_id: message.header_raw("Feedback-ID"),
        }),
    }
}

fn person(addr: &mail_parser::Addr<'_>) -> Person {
    Person {
        name: addr.name().map(clean).filter(|name| !name.is_empty()),
        address: addr.address().map(|address| address.trim().to_owned()),
    }
}

fn people(address: Option<&Address<'_>>) -> Vec<Person> {
    address.map(|a| a.iter().map(person).collect()).unwrap_or_default()
}

/// The delivery headers' addresses, each once. A forwarder names the address
/// the mail went to in `Delivered-To`, `X-Original-To` or `X-Forwarded-To`,
/// which it may keep out of `To`.
fn delivered_to(message: &mail_parser::Message<'_>) -> Vec<String> {
    const NAMES: [&str; 3] = ["Delivered-To", "X-Original-To", "X-Forwarded-To"];
    let mut found: Vec<String> = Vec::new();
    for name in NAMES {
        let Some(value) = message.header_raw(name) else {
            continue;
        };
        for address in value.split(',') {
            let address = address.trim();
            if address.contains('@') && !found.iter().any(|other| other.eq_ignore_ascii_case(address)) {
                found.push(address.to_owned());
            }
        }
    }
    found
}

fn ids(value: &HeaderValue<'_>) -> Vec<String> {
    match value.as_text_list() {
        Some(list) => list.iter().map(|id| id.to_string()).collect(),
        None => value.as_text().map(|id| vec![id.to_owned()]).unwrap_or_default(),
    }
}

/// Single line, no control characters.
fn clean(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_encoded_words_charsets_and_addresses() {
        let raw = b"From: =?UTF-8?Q?Ren=C3=A9e_Dupont?= <renee@example.test>\r\n\
            To: Ada <ada@example.test>, bob@example.test\r\n\
            Subject: =?ISO-8859-1?Q?R=E9union_demain?=\r\n\
            Date: Sun, 18 Oct 2026 10:00:00 +0200\r\n\
            Message-ID: <abc@example.test>\r\n\
            In-Reply-To: <parent@example.test>\r\n\
            References: <root@example.test> <parent@example.test>\r\n\r\n";
        let headers = parse(raw);
        assert_eq!(headers.subject, "Réunion demain");
        let from = headers.from.unwrap();
        assert_eq!(from.name.as_deref(), Some("Renée Dupont"));
        assert_eq!(from.address.as_deref(), Some("renee@example.test"));
        assert_eq!(headers.to.len(), 2);
        assert_eq!(headers.to[1].address.as_deref(), Some("bob@example.test"));
        assert_eq!(headers.date, Some(1_792_310_400_000));
        assert_eq!(headers.message_id.as_deref(), Some("abc@example.test"));
        assert_eq!(headers.in_reply_to, ["parent@example.test"]);
        assert_eq!(headers.references, ["root@example.test", "parent@example.test"]);
    }

    #[test]
    fn missing_or_garbage_headers_leave_fields_empty() {
        assert_eq!(parse(b""), Headers::default());
        let headers = parse(b"Subject: folded\r\n   over lines\r\nX-Junk: \xff\xfe\r\n\r\n");
        assert_eq!(headers.subject, "folded over lines");
        assert!(headers.from.is_none());
        assert!(headers.date.is_none());
    }

    #[test]
    fn shift_jis_subject_is_decoded() {
        let headers = parse(b"Subject: =?Shift_JIS?B?k/qWe4zq?=\r\n\r\n");
        assert_eq!(headers.subject, "日本語");
    }

    #[test]
    fn delivery_headers_name_the_address_the_mail_went_to() {
        let raw = b"To: someone@else.test\r\n\
            Delivered-To: ME@QQ.test\r\n\
            x-original-to: me@qq.test, other@qq.test\r\n\r\n";
        let headers = parse(raw);
        assert_eq!(
            headers.delivered_to,
            ["ME@QQ.test", "other@qq.test"],
            "either case, each once"
        );
    }

    #[test]
    fn written_to_is_the_people_then_the_delivery_headers_each_once() {
        let raw = b"From: bob@example.test\r\n\
            To: Ada <ada@example.test>, me@qq.test\r\n\
            Cc: me@QQ.test\r\n\
            Delivered-To: me@qq.test\r\n\
            X-Forwarded-To: forwarded@qq.test\r\n\r\n";
        let headers = parse(raw);
        assert_eq!(
            headers.written_to(),
            ["ada@example.test", "me@qq.test", "forwarded@qq.test"],
            "in order written, To and Cc before the delivery headers, each once"
        );
        assert_eq!(parse(b"Subject: nothing\r\n\r\n").written_to(), Vec::<String>::new());
    }
}
