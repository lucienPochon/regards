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
use mail_builder::mime::{BodyPart, MimePart};
use serde::{Deserialize, Serialize};

/// At most this many References are carried on (the parent's oldest are dropped).
const MAX_REFERENCES: usize = 20;

/// Where an automatic copy goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CopyKind {
    Cc,
    Bcc,
}

/// A copy every new draft of an account gets: Cc or Bcc to an address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoCopy {
    pub kind: CopyKind,
    pub address: String,
}

impl AutoCopy {
    /// Add it to a new draft's recipients, unless the address is one of them.
    pub fn apply(&self, to: &[Person], cc: &mut Vec<Person>, bcc: &mut Vec<Person>) {
        let there = to.iter().chain(cc.iter()).chain(bcc.iter()).any(|person| {
            person
                .address
                .as_deref()
                .is_some_and(|address| address.eq_ignore_ascii_case(&self.address))
        });
        if there {
            return;
        }
        let copy = Person {
            name: None,
            address: Some(self.address.clone()),
        };
        match self.kind {
            CopyKind::Cc => cc.push(copy),
            CopyKind::Bcc => bcc.push(copy),
        }
    }
}

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
    /// The formatted version, already sanitised ([`crate::html::sanitize_outgoing`]);
    /// `text` is then its plain-text version.
    pub html: Option<String>,
    pub attachments: Vec<Attachment>,
}

/// A file going with the message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub name: String,
    pub mime: String,
    pub data: Vec<u8>,
    /// An image in the text (`<img src="cid:…">`), not a file beside it.
    pub content_id: Option<String>,
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
        if self.recipients().is_empty() {
            return Err(ComposeError::NoRecipients);
        }
        self.assemble(false)
    }

    /// The copy of a draft kept in the Drafts folder, for other devices: it
    /// may have nobody to go to yet, and it keeps its Bcc so they come back.
    pub fn build_draft(&self) -> Result<Vec<u8>, ComposeError> {
        self.assemble(true)
    }

    fn assemble(&self, draft: bool) -> Result<Vec<u8>, ComposeError> {
        let from = self.from.address.as_deref().ok_or(ComposeError::NoSender)?;
        check(from)?;
        for person in self.to.iter().chain(&self.cc).chain(&self.bcc) {
            check(person.address.as_deref().unwrap_or(""))?;
        }
        let mut builder = MessageBuilder::new()
            .from(address(&self.from))
            .subject(self.subject.as_str())
            .message_id(self.message_id.as_str())
            .date(self.date.div_euclid(1000));
        // Images the text has; one taken out of it is not sent at all.
        let images: Vec<&Attachment> = match &self.html {
            Some(html) => self
                .attachments
                .iter()
                .filter(|a| {
                    a.content_id
                        .as_ref()
                        .is_some_and(|cid| html.contains(&format!("cid:{cid}")))
                })
                .collect(),
            None => Vec::new(),
        };
        // Files beside the text. Images are only in formatted text.
        let files: Vec<&Attachment> = self.attachments.iter().filter(|a| a.content_id.is_none()).collect();
        let document = self.html.as_ref().map(|html| {
            format!("<!doctype html><html><head><meta charset=\"utf-8\"></head><body>{html}</body></html>")
        });
        match document {
            // Images in the text: the HTML and its images together, the text beside them.
            Some(document) if !images.is_empty() => {
                let mut related = vec![MimePart::new("text/html", BodyPart::Text(document.into()))];
                for image in &images {
                    let cid = image.content_id.clone().unwrap_or_default();
                    related.push(
                        MimePart::new(image.mime.clone(), image.data.as_slice())
                            .inline()
                            .cid(cid),
                    );
                }
                let alternative = MimePart::new(
                    "multipart/alternative",
                    vec![
                        MimePart::new("text/plain", BodyPart::Text(normalise(&self.text).into())),
                        MimePart::new("multipart/related", related),
                    ],
                );
                let body = if files.is_empty() {
                    alternative
                } else {
                    let mut parts = vec![alternative];
                    for file in &files {
                        parts.push(
                            MimePart::new(file.mime.clone(), file.data.as_slice()).attachment(file_name(&file.name)),
                        );
                    }
                    MimePart::new("multipart/mixed", parts)
                };
                builder = builder.body(body);
            }
            document => {
                builder = builder.text_body(normalise(&self.text));
                if let Some(document) = document {
                    builder = builder.html_body(document);
                }
                for file in &files {
                    builder = builder.attachment(file.mime.as_str(), file_name(&file.name), file.data.as_slice());
                }
            }
        }
        if !self.to.is_empty() {
            builder = builder.to(list(&self.to));
        }
        if !self.cc.is_empty() {
            builder = builder.cc(list(&self.cc));
        }
        if draft && !self.bcc.is_empty() {
            builder = builder.bcc(list(&self.bcc));
        }
        if let Some(parent) = &self.in_reply_to {
            builder = builder.in_reply_to(parent.as_str());
        }
        if !self.references.is_empty() {
            builder = builder.references(self.references.clone());
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

/// A signature as it goes into a body: after a blank line; nothing for none.
pub fn signature_block(signature: &str) -> String {
    let signature = signature.trim_end();
    if signature.trim().is_empty() {
        String::new()
    } else {
        format!("\n\n{signature}")
    }
}

/// `body` sent from another identity: the signature `old` becomes `new`.
/// Where `old` is not in it (none, or edited away), `new` goes above the
/// quoted or forwarded message, or at the end.
pub fn swap_signature(body: &str, old: &str, new: &str) -> String {
    let (old, new) = (signature_block(old), signature_block(new));
    if !old.is_empty()
        && let Some(at) = body.find(&old)
    {
        return format!("{}{new}{}", &body[..at], &body[at + old.len()..]);
    }
    if new.is_empty() {
        return body.to_owned();
    }
    let at = quote_start(body).unwrap_or(body.trim_end().len());
    format!("{}{new}{}", &body[..at], &body[at..])
}

/// Something to ask about before a message goes (plan §5.9, send validation).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SendCheck {
    /// Nobody to send it to: it cannot go.
    NoRecipients,
    /// No subject.
    NoSubject,
    /// The text speaks of an attachment (`word`), and nothing is attached.
    MissingAttachment { word: String },
    /// Something still to fill in, as a template leaves it: `{{…}}` or `[[…]]`.
    Placeholder { text: String },
}

/// Words that say a file comes with a message, matched at the start of a
/// word, in any case: English, French, German, Spanish, Italian, Dutch.
const ATTACHMENT_WORDS: &[&str] = &[
    "attach",
    "enclosed",
    "pièce jointe",
    "pièces jointes",
    "ci-joint",
    "en pj",
    "anhang",
    "angehängt",
    "anbei",
    "adjunto",
    "adjunta",
    "allegato",
    "allegata",
    "bijlage",
    "bijgevoegd",
];

/// What to ask about before a message with these parts goes. Only what the
/// person wrote counts: not the quoted reply or forwarded message below it.
pub fn send_checks(recipients: usize, subject: &str, body: &str, attachments: usize) -> Vec<SendCheck> {
    let mut checks = Vec::new();
    if recipients == 0 {
        checks.push(SendCheck::NoRecipients);
    }
    if subject.trim().is_empty() {
        checks.push(SendCheck::NoSubject);
    }
    let written = written_part(body);
    if attachments == 0
        && let Some(word) = attachment_word(&written)
    {
        checks.push(SendCheck::MissingAttachment { word });
    }
    if let Some(text) = placeholder(&written).or_else(|| placeholder(subject)) {
        checks.push(SendCheck::Placeholder { text });
    }
    checks
}

/// A body without its quoted reply, forwarded message and `>` lines.
fn written_part(body: &str) -> String {
    let end = quote_start(body).unwrap_or(body.len());
    body[..end]
        .lines()
        .filter(|line| !line.trim_start().starts_with('>'))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The first attachment word starting a word of `text`, as written there.
fn attachment_word(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    for word in ATTACHMENT_WORDS {
        let mut from = 0;
        while let Some(at) = lower[from..].find(word) {
            let start = from + at;
            if lower[..start].chars().next_back().is_none_or(|c| !c.is_alphanumeric()) {
                let rest = &lower[start..];
                let end = rest
                    .char_indices()
                    .find(|(i, c)| *i >= word.len() && !c.is_alphanumeric())
                    .map_or(rest.len(), |(i, _)| i);
                return Some(rest[..end].to_owned());
            }
            from = start + word.len();
        }
    }
    None
}

/// The first `{{…}}` or `[[…]]` in `text`, if short enough to be one.
fn placeholder(text: &str) -> Option<String> {
    for (open, close) in [("{{", "}}"), ("[[", "]]")] {
        let mut from = 0;
        while let Some(at) = text[from..].find(open) {
            let start = from + at;
            if let Some(length) = text[start + open.len()..].find(close)
                && length <= 60
                && !text[start + open.len()..start + open.len() + length].contains('\n')
            {
                return Some(text[start..start + open.len() + length + close.len()].to_owned());
            }
            from = start + open.len();
        }
    }
    None
}

/// Where a quoted reply ("On …, Ada wrote:" then "> " lines) or a forwarded
/// message begins in a body made by [`quote`] or [`forwarded`]: at its blank line.
fn quote_start(body: &str) -> Option<usize> {
    let forward = body.find("\n\n---------- Forwarded message ----------\n");
    let mut offset = 0;
    let mut reply = None;
    let lines: Vec<&str> = body.split('\n').collect();
    for (index, line) in lines.iter().enumerate() {
        let quoted_next = lines.get(index + 1).is_some_and(|next| next.starts_with('>'));
        let blank_before = index >= 2 && lines[index - 1].is_empty();
        if line.ends_with(" wrote:") && quoted_next && blank_before {
            // Back over the blank line before the intro line.
            reply = Some(offset - 2);
            break;
        }
        offset += line.len() + 1;
    }
    match (reply, forward) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
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

    #[test]
    fn what_to_ask_before_a_message_goes() {
        assert_eq!(send_checks(1, "Plans", "See you", 0), []);
        assert_eq!(
            send_checks(0, " ", "Hi", 0),
            [SendCheck::NoRecipients, SendCheck::NoSubject]
        );
        assert_eq!(
            send_checks(1, "Report", "Please find the report Attached.", 0),
            [SendCheck::MissingAttachment {
                word: "attached".into()
            }]
        );
        assert_eq!(send_checks(1, "Report", "Please find the report attached.", 1), []);
        assert_eq!(
            send_checks(1, "Rapport", "Voici le rapport en pièce jointe.", 0),
            [SendCheck::MissingAttachment {
                word: "pièce jointe".into()
            }]
        );
        assert_eq!(send_checks(1, "Hi", "Anbei die Unterlagen", 0).len(), 1);
        assert_eq!(
            send_checks(1, "Hi", "I reattached nothing; detachment", 0),
            [],
            "not a word's start"
        );
        // Only what was written: the quoted original may speak of attachments.
        let reply = format!("Thanks!{}", quote("See the attached plan.", None, Some(0)));
        assert_eq!(send_checks(1, "Re: Plan", &reply, 0), []);
        assert_eq!(send_checks(1, "Hi", "> attached below\nThanks", 0), []);
        assert_eq!(
            send_checks(1, "Hi {{name}}", "Dear [[client]], hello", 0),
            [SendCheck::Placeholder {
                text: "[[client]]".into()
            }]
        );
        assert_eq!(
            send_checks(1, "Hi {{name}}", "Hello", 0),
            [SendCheck::Placeholder {
                text: "{{name}}".into()
            }]
        );
        assert_eq!(send_checks(1, "Hi", "a {{ b", 0), [], "not closed");
    }

    #[test]
    fn an_automatic_copy_is_added_once() {
        let person = |address: &str| Person {
            name: None,
            address: Some(address.into()),
        };
        let copy = AutoCopy {
            kind: CopyKind::Bcc,
            address: "archive@example.test".into(),
        };
        let (mut cc, mut bcc) = (Vec::new(), Vec::new());
        copy.apply(&[person("bob@example.test")], &mut cc, &mut bcc);
        assert!(cc.is_empty());
        assert_eq!(bcc, [person("archive@example.test")]);
        // Already addressed, in any field or case: not again.
        copy.apply(&[], &mut cc, &mut bcc);
        assert_eq!(bcc.len(), 1);
        let mut cc = vec![person("Archive@Example.test")];
        let mut bcc = Vec::new();
        copy.apply(&[], &mut cc, &mut bcc);
        assert!(bcc.is_empty());
    }

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
            html: None,
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
    fn images_in_the_text_go_with_the_html_and_files_beside() {
        use mail_parser::{MessageParser, MimeHeaders, PartType};
        let mut message = outgoing();
        message.html = Some("<p>See <img src=\"cid:lake@regards\" alt=\"\"></p>".into());
        message.attachments = vec![
            Attachment {
                name: "lake.png".into(),
                mime: "image/png".into(),
                data: b"\x89PNG fake".to_vec(),
                content_id: Some("lake@regards".into()),
            },
            Attachment {
                name: "plan.pdf".into(),
                mime: "application/pdf".into(),
                data: b"%PDF".to_vec(),
                content_id: None,
            },
            // Taken out of the text: not sent at all.
            Attachment {
                name: "removed.png".into(),
                mime: "image/png".into(),
                data: b"\x89PNG gone".to_vec(),
                content_id: Some("removed@regards".into()),
            },
        ];
        let mime = message.build().unwrap();
        let parsed = MessageParser::new().parse(&mime).unwrap();
        let types: Vec<String> = parsed
            .parts
            .iter()
            .map(|part| {
                let kind = part
                    .content_type()
                    .map(|t| format!("{}/{}", t.ctype(), t.subtype().unwrap_or("")));
                kind.unwrap_or_default()
            })
            .collect();
        assert_eq!(
            types,
            [
                "multipart/mixed",
                "multipart/alternative",
                "text/plain",
                "multipart/related",
                "text/html",
                "image/png",
                "application/pdf"
            ]
        );
        let image = parsed
            .parts
            .iter()
            .find(|part| part.content_type().is_some_and(|t| t.ctype() == "image"))
            .unwrap();
        assert_eq!(image.content_id(), Some("lake@regards"));
        assert!(matches!(image.body, PartType::Binary(_) | PartType::InlineBinary(_)));
        assert!(parsed.body_html(0).unwrap().contains("cid:lake@regards"));
        // The one file is a file; the image is not.
        assert_eq!(
            parsed
                .attachments()
                .filter(|a| a.attachment_name() == Some("plan.pdf"))
                .count(),
            1
        );

        // Without formatting there is no text to put an image in: only the file goes.
        message.html = None;
        let plain_mime = message.build().unwrap();
        let plain = MessageParser::new().parse(&plain_mime).unwrap();
        assert_eq!(plain.attachments().count(), 1);
    }

    #[test]
    fn attachments_travel_with_their_names_and_types() {
        let mut message = outgoing();
        message.attachments = vec![
            Attachment {
                name: "C:\\Users\\ada\\Rapport été.pdf".into(),
                mime: guess_mime("Rapport été.pdf").into(),
                data: b"%PDF-1.4 binary \x00\xff".to_vec(),
                content_id: None,
            },
            Attachment {
                name: "notes.txt".into(),
                mime: guess_mime("notes.txt").into(),
                data: b"plain notes".to_vec(),
                content_id: None,
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
    fn formatted_messages_carry_text_and_html() {
        let mut message = outgoing();
        let html = crate::html::sanitize_outgoing("<p>Hello <strong>Bob</strong></p><ul><li>one</li></ul>");
        message.text = crate::html::to_text(&html);
        message.html = Some(html);
        let mime = message.build().unwrap();
        let body = crate::body::parse(&mime);
        assert!(body.has_html);
        assert!(body.text.contains("Hello Bob"), "{}", body.text);
        let rendered = crate::html::render(&mime, false, None).unwrap().html;
        assert!(rendered.contains("<strong>Bob</strong>"), "{rendered}");
        assert!(String::from_utf8_lossy(&mime).contains("multipart/alternative"));
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
    #[test]
    fn signatures_go_after_a_blank_line_and_swap_with_the_sender() {
        assert_eq!(signature_block(""), "");
        assert_eq!(signature_block("  \n"), "");
        assert_eq!(signature_block("Ada\nEngines Ltd\n"), "\n\nAda\nEngines Ltd");

        // A reply made with a signature: the signature is swapped in place.
        let quote = quote("Plans?", Some(&person(Some("Bob"), "bob@x.test")), Some(0));
        let body = format!("Yes!{}{quote}", signature_block("Ada"));
        assert_eq!(
            swap_signature(&body, "Ada", "Ada (work)"),
            format!("Yes!\n\nAda (work){quote}")
        );
        assert_eq!(
            swap_signature(&body, "Ada", ""),
            format!("Yes!{quote}"),
            "no signature from the new one"
        );

        // Made without one: the new one goes above the quote, not inside it.
        let plain = format!("Yes!{quote}");
        assert_eq!(swap_signature(&plain, "", "Ada"), format!("Yes!\n\nAda{quote}"));
        // ...or above a forwarded message, or at the end.
        let fwd = forwarded("Hi", None, &[], "Plans", None);
        assert_eq!(
            swap_signature(&format!("See below{fwd}"), "", "Ada"),
            format!("See below\n\nAda{fwd}")
        );
        assert_eq!(swap_signature("Hello\n\n", "", "Ada"), "Hello\n\nAda\n\n");
        // Edited away: left as written, the new one added.
        assert_eq!(swap_signature("Hello", "Ada", "Bea"), "Hello\n\nBea");
    }

    #[test]
    fn a_drafts_copy_may_have_nobody_yet_and_keeps_its_bcc() {
        let mut draft = Outgoing {
            from: person(Some("Ada"), "ada@example.test"),
            to: vec![],
            cc: vec![],
            bcc: vec![person(None, "secret@example.test")],
            subject: "Plans".into(),
            text: "Half written".into(),
            message_id: "draft-1@example.test".into(),
            in_reply_to: None,
            references: vec![],
            date: 0,
            html: None,
            attachments: vec![],
        };
        let copy = String::from_utf8(draft.build_draft().unwrap()).unwrap();
        assert!(
            copy.contains("Bcc: <secret@example.test>") || copy.contains("Bcc: secret@example.test"),
            "{copy}"
        );
        draft.bcc.clear();
        assert!(draft.build_draft().is_ok(), "nobody to send to yet");
        assert!(
            matches!(draft.build(), Err(ComposeError::NoRecipients)),
            "but it cannot be sent so"
        );
    }
}
