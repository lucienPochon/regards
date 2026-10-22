//! Message bodies: the readable text, whether there is an HTML version, and
//! the attachments. Parsed with `mail-parser`, which never panics on
//! malformed input.

use crate::attachment::{self, Preview, Risk};
use mail_parser::{MessageParser, MimeHeaders};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    /// The sender's name for it, without folders or direction marks
    /// ([`attachment::display_name`]).
    pub name: Option<String>,
    /// For example `application/pdf`, as the sender declared it.
    pub mime: String,
    /// Decoded size in bytes.
    pub size: usize,
    /// Shown inside the message (for example an image the HTML refers to) rather than attached.
    pub inline: bool,
    /// How risky it is to open.
    pub risk: Risk,
    /// What the app can show of it by itself, judged by its bytes.
    pub preview: Option<Preview>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Body {
    /// The text version, or text derived from the HTML when there is none.
    pub text: String,
    /// Whether the sender included an HTML version.
    pub has_html: bool,
    pub attachments: Vec<Attachment>,
}

/// Parse a complete message (RFC 5322 source). Never fails: unreadable
/// input gives an empty body.
pub fn parse(raw: &[u8]) -> Body {
    let Some(message) = MessageParser::default().parse(raw) else {
        return Body::default();
    };
    let text = message.body_text(0).map(|text| text.into_owned()).unwrap_or_default();
    let has_html = message.html_part(0).is_some_and(|part| part.is_text_html());
    let attachments = message.attachments().map(describe).collect();
    Body {
        text,
        has_html,
        attachments,
    }
}

/// How long a preview is, in characters.
pub const PREVIEW_CHARS: usize = 200;

/// The start of a message's text for its row in a list, from the first
/// octets of its text part as stored (`encoding`: its transfer encoding, so
/// base64 or quoted-printable, which may be cut anywhere). Quoted lines go,
/// whitespace (and the invisible padding of newsletter previews) collapses.
pub fn preview(start: &[u8], encoding: &str, charset: Option<&str>, html: bool) -> String {
    let mut part = format!(
        "Content-Type: text/{}; charset=\"{}\"\r\nContent-Transfer-Encoding: {encoding}\r\n\r\n",
        if html { "html" } else { "plain" },
        charset.unwrap_or("utf-8"),
    )
    .into_bytes();
    if encoding.eq_ignore_ascii_case("base64") {
        // Cut where the server cut, then back to whole groups of four.
        let mut kept: Vec<u8> = start.iter().copied().filter(|b| !b.is_ascii_whitespace()).collect();
        kept.truncate(kept.len() / 4 * 4);
        part.extend_from_slice(&kept);
    } else {
        part.extend_from_slice(start);
    }
    let text = MessageParser::default()
        .parse(&part)
        .and_then(|message| message.body_text(0).map(|text| text.into_owned()))
        .unwrap_or_default();
    let mut out = String::new();
    for line in text.lines().filter(|line| !line.trim_start().starts_with('>')) {
        for word in line
            .split(|c: char| c.is_whitespace() || is_padding(c))
            .filter(|w| !w.is_empty())
        {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(word);
        }
        if out.chars().count() >= PREVIEW_CHARS {
            break;
        }
    }
    out.chars().take(PREVIEW_CHARS).collect()
}

/// Characters newsletters use to pad their preview text: invisible here.
fn is_padding(c: char) -> bool {
    matches!(
        c,
        '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{feff}' | '\u{034f}' | '\u{00ad}' | '\u{2007}'
    )
}

/// A file a message carries, with its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    pub name: String,
    pub mime: String,
    pub data: Vec<u8>,
}

/// The files attached to a message (not those shown inside it), for
/// forwarding. Unnamed ones get a name from their type.
pub fn files(raw: &[u8]) -> Vec<File> {
    let Some(message) = MessageParser::default().parse(raw) else {
        return Vec::new();
    };
    message
        .attachments()
        .filter(|part| !is_inline(part))
        .map(|part| {
            let mime = mime_of(part);
            let name = part
                .attachment_name()
                .map(str::to_owned)
                .unwrap_or_else(|| match mime.as_str() {
                    "message/rfc822" => "message.eml".to_owned(),
                    _ => "attachment".to_owned(),
                });
            File {
                name,
                mime,
                data: part.contents().to_vec(),
            }
        })
        .collect()
}

/// The attachment at `index` of [`Body::attachments`], with its bytes under
/// the name to save it as ([`attachment::file_name`]).
pub fn attachment(raw: &[u8], index: usize) -> Option<(Attachment, File)> {
    let message = MessageParser::default().parse(raw)?;
    message.attachments().nth(index).map(with_bytes)
}

/// All of [`Body::attachments`] with their bytes, from one parse of the
/// message: for a message whose attachments are shown one after another.
pub fn attachments(raw: &[u8]) -> Vec<(Attachment, File)> {
    MessageParser::default()
        .parse(raw)
        .map(|message| message.attachments().map(with_bytes).collect())
        .unwrap_or_default()
}

fn with_bytes(part: &mail_parser::MessagePart<'_>) -> (Attachment, File) {
    let described = describe(part);
    let file = File {
        name: attachment::file_name(part.attachment_name(), &described.mime),
        mime: described.mime.clone(),
        data: part.contents().to_vec(),
    };
    (described, file)
}

fn describe(part: &mail_parser::MessagePart<'_>) -> Attachment {
    let mime = mime_of(part);
    let name = part.attachment_name();
    Attachment {
        name: name.and_then(attachment::display_name),
        risk: attachment::risk(name, &mime),
        preview: attachment::preview(name, &mime, part.contents()),
        size: part.len(),
        inline: is_inline(part),
        mime,
    }
}

fn mime_of(part: &mail_parser::MessagePart<'_>) -> String {
    part.content_type()
        .map_or_else(
            || "application/octet-stream".to_owned(),
            |ct| match ct.subtype() {
                Some(subtype) => format!("{}/{}", ct.ctype(), subtype),
                None => ct.ctype().to_owned(),
            },
        )
        .to_ascii_lowercase()
}

fn is_inline(part: &mail_parser::MessagePart<'_>) -> bool {
    part.content_disposition().is_some_and(|cd| cd.is_inline())
        || (part.content_id().is_some() && part.content_disposition().is_none_or(|cd| !cd.is_attachment()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_message() {
        let body = parse(b"Subject: hi\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nHello there\r\n");
        assert_eq!(body.text.trim(), "Hello there");
        assert!(!body.has_html);
        assert!(body.attachments.is_empty());
    }

    #[test]
    fn alternative_with_html_and_an_attachment() {
        let raw = b"Subject: report\r\n\
            MIME-Version: 1.0\r\n\
            Content-Type: multipart/mixed; boundary=\"outer\"\r\n\r\n\
            --outer\r\n\
            Content-Type: multipart/alternative; boundary=\"inner\"\r\n\r\n\
            --inner\r\n\
            Content-Type: text/plain; charset=utf-8\r\n\r\n\
            Plain version\r\n\
            --inner\r\n\
            Content-Type: text/html; charset=utf-8\r\n\r\n\
            <p>HTML <b>version</b></p>\r\n\
            --inner--\r\n\
            --outer\r\n\
            Content-Type: application/pdf; name=\"report.pdf\"\r\n\
            Content-Disposition: attachment; filename=\"report.pdf\"\r\n\
            Content-Transfer-Encoding: base64\r\n\r\n\
            JVBERi0xLjQK\r\n\
            --outer--\r\n";
        let body = parse(raw);
        assert_eq!(body.text.trim(), "Plain version");
        assert!(body.has_html);
        assert_eq!(
            body.attachments,
            [Attachment {
                name: Some("report.pdf".into()),
                mime: "application/pdf".into(),
                size: 9,
                inline: false,
                risk: Risk::Open,
                preview: None,
            }]
        );
        assert_eq!(
            files(raw),
            [File {
                name: "report.pdf".into(),
                mime: "application/pdf".into(),
                data: b"%PDF-1.4\n".to_vec(),
            }]
        );
    }

    #[test]
    fn inline_images_are_not_files_to_forward() {
        let raw = b"MIME-Version: 1.0\r\nContent-Type: multipart/related; boundary=\"r\"\r\n\r\n\
            --r\r\nContent-Type: text/html\r\n\r\n<img src=\"cid:logo@x\">\r\n\
            --r\r\nContent-Type: image/png\r\nContent-ID: <logo@x>\r\nContent-Transfer-Encoding: base64\r\n\r\niVBORw0KGgo=\r\n\
            --r--\r\n";
        assert!(files(raw).is_empty());
    }

    #[test]
    fn attachments_come_out_by_their_place_in_the_list() {
        let raw = b"MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"m\"\r\n\r\n\
            --m\r\nContent-Type: text/plain\r\n\r\nSee attached.\r\n\
            --m\r\nContent-Type: image/png\r\nContent-ID: <logo@x>\r\nContent-Transfer-Encoding: base64\r\n\r\niVBORw0KGgoAAAANSUhEUg==\r\n\
            --m\r\nContent-Type: text/plain; charset=iso-8859-1; name=\"caf\xe9.txt\"\r\n\
            Content-Disposition: attachment; filename=\"../notes\xe2\x80\xae.txt\"\r\n\r\nCaf\xe9\r\n\
            --m\r\nContent-Type: application/octet-stream\r\n\
            Content-Disposition: attachment; filename=\"run.exe\"\r\n\r\nMZ\r\n\
            --m--\r\n";
        let body = parse(raw);
        let shown: Vec<(Option<&str>, bool, Risk, Option<Preview>)> = body
            .attachments
            .iter()
            .map(|a| (a.name.as_deref(), a.inline, a.risk, a.preview))
            .collect();
        assert_eq!(
            shown,
            [
                (None, true, Risk::Open, Some(Preview::Image)),
                (Some("notes.txt"), false, Risk::SaveOnly, Some(Preview::Text)),
                (Some("run.exe"), false, Risk::SaveOnly, None),
            ]
        );
        let (described, file) = attachment(raw, 1).unwrap();
        assert_eq!(described, body.attachments[1]);
        assert_eq!(file.name, "notes.txt");
        assert_eq!(String::from_utf8_lossy(&file.data).trim(), "Caf\u{e9}");
        assert_eq!(attachment(raw, 0).unwrap().1.name, "attachment.png");
        assert!(attachment(raw, 3).is_none());
        let all = attachments(raw);
        assert_eq!(all.len(), 3);
        assert_eq!(all[1], (described, file));
    }

    #[test]
    fn html_only_messages_still_get_text() {
        let body = parse(b"Content-Type: text/html\r\n\r\n<html><body><p>Only <i>HTML</i></p></body></html>\r\n");
        assert!(body.has_html);
        assert!(body.text.contains("Only HTML"), "{:?}", body.text);
    }

    #[test]
    fn garbage_gives_an_empty_body() {
        assert_eq!(parse(b""), Body::default());
        let _ = parse(b"\xff\xfe\x00not mail at all");
    }

    #[test]
    fn previews_read_the_start_however_it_was_cut() {
        // Quoted-printable, with a quote below the reply.
        let qp = b"Perfect, Saturdays work =\r\nfor us.\r\n\r\nOn Sun, Bob wrote:\r\n> Plans?\r\n";
        assert_eq!(
            preview(qp, "quoted-printable", Some("utf-8"), false),
            "Perfect, Saturdays work for us. On Sun, Bob wrote:"
        );
        // Base64 cut in the middle of a group: what is whole is read.
        let full = crate::encoding::base64("Gr\u{fc}\u{df}e aus Z\u{fc}rich".as_bytes());
        let cut = &full.as_bytes()[..full.len() - 2];
        assert!(preview(cut, "base64", Some("utf-8"), false).starts_with("Gr\u{fc}\u{df}e aus Z"));
        // Latin-1, decoded.
        assert_eq!(
            preview(b"Caf\xe9 cr\xe8me", "8bit", Some("iso-8859-1"), false),
            "Caf\u{e9} cr\u{e8}me"
        );
        // HTML: the text, not the styles or tags, nor the invisible padding.
        let html = b"<html><head><style>p{color:red}</style></head><body><p>Hello\xe2\x80\x8c\xe2\x80\x8c <b>Sam</b></p></body></html>";
        assert_eq!(preview(html, "8bit", Some("utf-8"), true), "Hello Sam");
        // Long text stops at the limit.
        let long = "word ".repeat(100);
        assert_eq!(
            preview(long.as_bytes(), "7bit", None, false).chars().count(),
            PREVIEW_CHARS
        );
    }
}
