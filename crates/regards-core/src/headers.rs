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
}
