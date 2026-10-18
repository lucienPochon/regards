//! Writing mail (Phase 2): the MIME of an outgoing message, and how a reply
//! or forward starts out.
//!
//! - **Message-ID** is ours (`<random@sender's domain>`), never the machine's
//!   host name.
//! - **Date** is in UTC, so the message does not reveal the sender's time zone.
//! - **Bcc** recipients are never written into the message; they only go in
//!   the SMTP envelope.

use crate::headers::Person;
use mail_builder::MessageBuilder;
use mail_builder::headers::address::Address;

/// At most this many References are carried on (the parent's oldest are dropped).
const MAX_REFERENCES: usize = 20;

/// An outgoing message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    pub from: Person,
    pub to: Vec<Person>,
    pub cc: Vec<Person>,
    pub bcc: Vec<Person>,
    pub subject: String,
    pub text: String,
    /// Message-ID without angle brackets.
    pub message_id: String,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    /// Unix time in milliseconds.
    pub date: i64,
    pub attachments: Vec<Attachment>,
}

/// A file going with the message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub name: String,
    pub mime: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ComposeError {
    #[error("{0:?} is not a valid email address")]
    BadAddress(String),
    #[error("the message has no recipients")]
    NoRecipients,
    #[error("the sender has no address")]
    NoSender,
}

impl Outgoing {
    /// Everyone the message goes to, Bcc included, each once.
    pub fn recipients(&self) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        self.to
            .iter()
            .chain(&self.cc)
            .chain(&self.bcc)
            .filter_map(|person| person.address.clone())
            .filter(|address| seen.insert(address.to_ascii_lowercase()))
            .collect()
    }

    /// Check the addresses, then build the message.
    pub fn build(&self) -> Result<Vec<u8>, ComposeError> {
        let from = self.from.address.as_deref().ok_or(ComposeError::NoSender)?;
        check(from)?;
        for person in self.to.iter().chain(&self.cc).chain(&self.bcc) {
            check(person.address.as_deref().unwrap_or(""))?;
        }
        if self.recipients().is_empty() {
            return Err(ComposeError::NoRecipients);
        }
        let mut builder = MessageBuilder::new()
            .from(address(&self.from))
            .subject(self.subject.as_str())
            .message_id(self.message_id.as_str())
            .date(self.date.div_euclid(1000))
            .text_body(normalise(&self.text));
        if !self.to.is_empty() {
            builder = builder.to(list(&self.to));
        }
        if !self.cc.is_empty() {
            builder = builder.cc(list(&self.cc));
        }
        if let Some(parent) = &self.in_reply_to {
            builder = builder.in_reply_to(parent.as_str());
        }
        if !self.references.is_empty() {
            builder = builder.references(self.references.clone());
        }
        for attachment in &self.attachments {
            builder = builder.attachment(
                attachment.mime.as_str(),
                file_name(&attachment.name),
                attachment.data.as_slice(),
            );
        }
        Ok(builder.write_to_vec().expect("writing to memory cannot fail"))
    }
}

/// A file's name as it may travel: no folders, no control characters, not empty.
pub fn file_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let clean: String = base.chars().filter(|c| !c.is_control()).collect();
    let clean = clean.trim().trim_matches('.').trim();
    if clean.is_empty() {
        "attachment".to_owned()
    } else {
        clean.chars().take(200).collect()
    }
}

/// The media type for a file name, from its extension; `application/octet-stream` when unknown.
pub fn guess_mime(name: &str) -> &'static str {
    let extension = name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "svg" => "image/svg+xml",
        "txt" | "log" | "md" => "text/plain",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "ics" => "text/calendar",
        "vcf" => "text/vcard",
        "eml" => "message/rfc822",
        "zip" => "application/zip",
        "json" => "application/json",
        "xml" => "application/xml",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "odt" => "application/vnd.oasis.opendocument.text",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        _ => "application/octet-stream",
    }
}

fn address(person: &Person) -> Address<'_> {
    Address::new_address(
        person.name.as_deref().filter(|name| !name.trim().is_empty()),
        person.address.as_deref().unwrap_or(""),
    )
}

fn list(people: &[Person]) -> Address<'_> {
    Address::new_list(people.iter().map(address).collect())
}

/// A plausible address: something@domain, nothing that breaks a header or a command.
fn check(address: &str) -> Result<(), ComposeError> {
    let valid = address.split_once('@').is_some_and(|(local, domain)| {
        !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
    }) && address.len() <= 320
        && !address
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '<' | '>' | ',' | '"'));
    if valid {
        Ok(())
    } else {
        Err(ComposeError::BadAddress(address.to_owned()))
    }
}

/// Line ends as CRLF are the builder's job; here only stray CRs go.
fn normalise(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// A new Message-ID on the sender's domain: 128 random bits, hex.
pub fn new_message_id(from_address: &str) -> String {
    let domain = from_address
        .rsplit_once('@')
        .map(|(_, domain)| domain)
        .filter(|domain| !domain.is_empty())
        .unwrap_or("regards.invalid");
    let mut bytes = [0u8; 16];
    // Without randomness a send must not go ahead with a guessable or repeated ID.
    getrandom::fill(&mut bytes).expect("the operating system's random source");
    let random: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{random}@{domain}")
}

/// "Re: subject", without piling up prefixes.
pub fn reply_subject(subject: &str) -> String {
    prefixed("Re: ", subject, &["re:", "aw:", "sv:", "antw:", "réf:", "ref:"])
}

/// "Fwd: subject", without piling up prefixes.
pub fn forward_subject(subject: &str) -> String {
    prefixed("Fwd: ", subject, &["fwd:", "fw:", "wg:", "tr:", "rv:"])
}

fn prefixed(prefix: &str, subject: &str, known: &[&str]) -> String {
    let subject = subject.trim();
    let lower = subject.to_lowercase();
    if known.iter().any(|p| lower.starts_with(p)) {
        subject.to_owned()
    } else {
        format!("{prefix}{subject}")
    }
}

/// References for a reply: the parent's, then the parent itself, the oldest dropped first.
pub fn reply_references(parent_references: &[String], parent_id: Option<&str>) -> Vec<String> {
    let mut references: Vec<String> = parent_references.to_vec();
    references.extend(parent_id.map(str::to_owned));
    let mut seen = std::collections::HashSet::new();
    references.retain(|id| seen.insert(id.clone()));
    let excess = references.len().saturating_sub(MAX_REFERENCES);
    // Keep the first (thread root) and the most recent ones.
    if excess > 0 {
        references.drain(1..=excess);
    }
    references
}

/// Who a reply goes to. Reply: the Reply-To, else the sender. Reply all: also
/// the other recipients, in Cc, leaving out `me` (the account's addresses).
pub fn reply_recipients(
    from: Option<&Person>,
    reply_to: &[Person],
    to: &[Person],
    cc: &[Person],
    me: &[&str],
    all: bool,
) -> (Vec<Person>, Vec<Person>) {
    let is_me = |person: &Person| {
        person
            .address
            .as_deref()
            .is_some_and(|address| me.iter().any(|mine| mine.eq_ignore_ascii_case(address)))
    };
    let mut primary: Vec<Person> = if reply_to.is_empty() {
        from.cloned().into_iter().collect()
    } else {
        reply_to.to_vec()
    };
    // Replying to a message I sent goes to its recipients instead.
    if primary.iter().all(is_me) && !to.is_empty() {
        primary = to.to_vec();
    }
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut keep = |person: &Person| {
        person
            .address
            .as_deref()
            .is_some_and(|address| seen.insert(address.to_ascii_lowercase()))
    };
    let primary: Vec<Person> = primary.into_iter().filter(|p| keep(p)).collect();
    let others: Vec<Person> = if all {
        to.iter()
            .chain(cc)
            .filter(|person| !is_me(person))
            .filter(|p| keep(p))
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    (primary, others)
}

/// The quoted original under a reply: "On <when>, <who> wrote:" and "> " lines.
pub fn quote(text: &str, from: Option<&Person>, date: Option<i64>) -> String {
    let who = from
        .and_then(|p| p.name.clone().or_else(|| p.address.clone()))
        .unwrap_or_else(|| "someone".into());
    let intro = match date {
        Some(ms) => format!("On {}, {who} wrote:", utc(ms)),
        None => format!("{who} wrote:"),
    };
    let quoted: Vec<String> = normalise(text.trim_end())
        .lines()
        .map(|line| {
            if line.is_empty() {
                ">".to_owned()
            } else {
                format!("> {line}")
            }
        })
        .collect();
    format!("\n\n{intro}\n{}\n", quoted.join("\n"))
}

/// The original under a forward, with its headers.
pub fn forwarded(text: &str, from: Option<&Person>, to: &[Person], subject: &str, date: Option<i64>) -> String {
    let show = |p: &Person| match (&p.name, &p.address) {
        (Some(name), Some(address)) => format!("{name} <{address}>"),
        (Some(name), None) => name.clone(),
        (None, Some(address)) => address.clone(),
        (None, None) => String::new(),
    };
    let mut block = String::from("\n\n---------- Forwarded message ----------\n");
    if let Some(from) = from {
        block.push_str(&format!("From: {}\n", show(from)));
    }
    if let Some(ms) = date {
        block.push_str(&format!("Date: {}\n", utc(ms)));
    }
    block.push_str(&format!("Subject: {subject}\n"));
    if !to.is_empty() {
        block.push_str(&format!("To: {}\n", to.iter().map(show).collect::<Vec<_>>().join(", ")));
    }
    block.push('\n');
    block.push_str(&normalise(text.trim_end()));
    block.push('\n');
    block
}

/// "2026-10-18 10:05 UTC".
fn utc(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02} UTC",
        rest / 3600,
        rest % 3600 / 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(name: Option<&str>, address: &str) -> Person {
        Person {
            name: name.map(str::to_owned),
            address: Some(address.to_owned()),
        }
    }

    fn outgoing() -> Outgoing {
        Outgoing {
            from: person(Some("Ada Lovelace"), "ada@example.test"),
            to: vec![person(Some("Bob"), "bob@example.test")],
            cc: vec![person(None, "carol@example.test")],
            bcc: vec![person(None, "secret@example.test")],
            subject: "Réunion: notes".into(),
            text: "Hello Bob,\n\n.a line starting with a dot\nMerci!\n".into(),
            message_id: "abc123@example.test".into(),
            in_reply_to: Some("parent@example.test".into()),
            references: vec!["root@example.test".into(), "parent@example.test".into()],
            date: 1_792_317_900_000,
            attachments: vec![],
        }
    }

    #[test]
    fn the_message_has_our_headers_and_reads_back() {
        let mime = outgoing().build().unwrap();
        let text = String::from_utf8_lossy(&mime);
        assert!(text.contains("Message-ID: <abc123@example.test>"), "{text}");
        assert!(text.contains("+0000"), "the date is in UTC: {text}");
        assert!(!text.to_lowercase().contains("bcc"), "Bcc is never in the message");
        assert!(!text.contains("secret@example.test"));

        // Reading it back gives the same message.
        let parsed = crate::headers::parse(&mime);
        assert_eq!(parsed.subject, "Réunion: notes");
        assert_eq!(parsed.from, Some(person(Some("Ada Lovelace"), "ada@example.test")));
        assert_eq!(parsed.to, [person(Some("Bob"), "bob@example.test")]);
        assert_eq!(parsed.cc, [person(None, "carol@example.test")]);
        assert_eq!(parsed.message_id.as_deref(), Some("abc123@example.test"));
        assert_eq!(parsed.in_reply_to, ["parent@example.test"]);
        assert_eq!(parsed.references, ["root@example.test", "parent@example.test"]);
        assert_eq!(parsed.date, Some(1_792_317_900_000));
        let body = crate::body::parse(&mime);
        assert_eq!(
            body.text.replace("\r\n", "\n").trim_end(),
            "Hello Bob,\n\n.a line starting with a dot\nMerci!"
        );

        assert_eq!(
            outgoing().recipients(),
            ["bob@example.test", "carol@example.test", "secret@example.test"]
        );
    }

    #[test]
    fn attachments_travel_with_their_names_and_types() {
        let mut message = outgoing();
        message.attachments = vec![
            Attachment {
                name: "C:\\Users\\ada\\Rapport été.pdf".into(),
                mime: guess_mime("Rapport été.pdf").into(),
                data: b"%PDF-1.4 binary \x00\xff".to_vec(),
            },
            Attachment {
                name: "notes.txt".into(),
                mime: guess_mime("notes.txt").into(),
                data: b"plain notes".to_vec(),
            },
        ];
        let mime = message.build().unwrap();
        let body = crate::body::parse(&mime);
        assert_eq!(
            body.text.replace("\r\n", "\n").trim_end().lines().next(),
            Some("Hello Bob,")
        );
        let files: Vec<(Option<&str>, &str, usize)> = body
            .attachments
            .iter()
            .map(|a| (a.name.as_deref(), a.mime.as_str(), a.size))
            .collect();
        assert_eq!(
            files,
            [
                (Some("Rapport été.pdf"), "application/pdf", 18),
                (Some("notes.txt"), "text/plain", 11)
            ]
        );
    }

    #[test]
    fn file_names_and_types() {
        assert_eq!(file_name("/home/ada/report.pdf"), "report.pdf");
        assert_eq!(file_name("C:\\x\\..\\bad\u{0}name.txt"), "badname.txt");
        assert_eq!(file_name("  "), "attachment");
        assert_eq!(file_name(".."), "attachment");
        assert_eq!(guess_mime("Photo.JPG"), "image/jpeg");
        assert_eq!(guess_mime("archive"), "application/octet-stream");
        assert_eq!(guess_mime("x.unknown"), "application/octet-stream");
    }

    #[test]
    fn bad_or_missing_addresses_stop_the_message() {
        let mut bad = outgoing();
        bad.to.push(person(None, "eve@example.test>\r\nBcc: x@y.test"));
        assert!(matches!(bad.build(), Err(ComposeError::BadAddress(_))));
        let mut nobody = outgoing();
        nobody.to.clear();
        nobody.cc.clear();
        nobody.bcc.clear();
        assert_eq!(nobody.build(), Err(ComposeError::NoRecipients));
        for address in ["", "ada", "ada@", "@example.test", "ada@localhost", "a b@example.test"] {
            assert!(check(address).is_err(), "{address:?}");
        }
    }

    #[test]
    fn message_ids_are_random_and_on_the_senders_domain() {
        let a = new_message_id("ada@example.test");
        let b = new_message_id("ada@example.test");
        assert_ne!(a, b);
        assert!(a.ends_with("@example.test") && a.len() == 32 + 1 + 12, "{a}");
        assert!(new_message_id("broken").ends_with("@regards.invalid"));
    }

    #[test]
    fn subjects_do_not_pile_up_prefixes() {
        assert_eq!(reply_subject("Plans"), "Re: Plans");
        assert_eq!(reply_subject("Re: Plans"), "Re: Plans");
        assert_eq!(reply_subject("AW: Pläne"), "AW: Pläne");
        assert_eq!(reply_subject(""), "Re: ");
        assert_eq!(forward_subject("Plans"), "Fwd: Plans");
        assert_eq!(forward_subject("TR: Plans"), "TR: Plans");
    }

    #[test]
    fn references_keep_the_root_and_the_latest() {
        assert_eq!(reply_references(&["a".into()], Some("b")), ["a", "b"]);
        assert_eq!(reply_references(&[], Some("b")), ["b"]);
        let long: Vec<String> = (0..30).map(|n| format!("m{n}")).collect();
        let kept = reply_references(&long, Some("parent"));
        assert_eq!(kept.len(), MAX_REFERENCES);
        assert_eq!(kept[0], "m0");
        assert_eq!(kept.last().unwrap(), "parent");
    }

    #[test]
    fn replies_go_to_the_right_people() {
        let ada = person(Some("Ada"), "ada@example.test");
        let bob = person(Some("Bob"), "bob@example.test");
        let me = person(Some("Me"), "me@example.test");
        let list = person(None, "list@example.test");
        let carol = person(None, "carol@example.test");
        let to_me_and_bob = vec![me.clone(), bob.clone()];
        let cc_carol = vec![carol.clone()];

        let (to, cc) = reply_recipients(Some(&ada), &[], &to_me_and_bob, &cc_carol, &["me@example.test"], false);
        assert_eq!(to, vec![ada.clone()]);
        assert!(cc.is_empty());

        let (to, cc) = reply_recipients(Some(&ada), &[], &to_me_and_bob, &cc_carol, &["ME@example.test"], true);
        assert_eq!(to, vec![ada.clone()]);
        assert_eq!(cc, vec![bob.clone(), carol], "everyone but me");

        let only_me = vec![me.clone()];
        let reply_to = vec![list.clone()];
        let (to, _) = reply_recipients(Some(&ada), &reply_to, &only_me, &[], &["me@example.test"], false);
        assert_eq!(to, vec![list], "Reply-To wins");

        // My own sent message: the reply goes to its recipients.
        let to_bob = vec![bob.clone()];
        let (to, _) = reply_recipients(Some(&me), &[], &to_bob, &[], &["me@example.test"], false);
        assert_eq!(to, vec![bob]);
    }

    #[test]
    fn quotes_and_forwards() {
        let ada = person(Some("Ada"), "ada@example.test");
        let quoted = quote("Hi,\r\n\r\nSee you.\n", Some(&ada), Some(1_792_317_900_000));
        assert_eq!(
            quoted,
            "\n\nOn 2026-10-18 10:05 UTC, Ada wrote:\n> Hi,\n>\n> See you.\n"
        );
        let forward = forwarded("Body", Some(&ada), &[person(None, "bob@example.test")], "Plans", None);
        assert!(
            forward.contains("From: Ada <ada@example.test>\nSubject: Plans\nTo: bob@example.test\n\nBody"),
            "{forward}"
        );
        assert_eq!(utc(0), "1970-01-01 00:00 UTC");
        assert_eq!(utc(951_782_400_000), "2000-02-29 00:00 UTC");
    }
}
