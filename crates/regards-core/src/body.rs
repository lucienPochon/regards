//! Message bodies: the readable text, whether there is an HTML version, and
//! the attachments. Parsed with `mail-parser`, which never panics on
//! malformed input.

use mail_parser::{MessageParser, MimeHeaders};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    pub name: Option<String>,
    /// For example `application/pdf`.
    pub mime: String,
    /// Decoded size in bytes.
    pub size: usize,
    /// Shown inside the message (for example an image the HTML refers to) rather than attached.
    pub inline: bool,
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
    let attachments = message
        .attachments()
        .map(|part| {
            let mime = part.content_type().map_or_else(
                || "application/octet-stream".to_owned(),
                |ct| match ct.subtype() {
                    Some(subtype) => format!("{}/{}", ct.ctype(), subtype),
                    None => ct.ctype().to_owned(),
                },
            );
            let inline = part.content_disposition().is_some_and(|cd| cd.is_inline())
                || (part.content_id().is_some() && part.content_disposition().is_none_or(|cd| !cd.is_attachment()));
            Attachment {
                name: part.attachment_name().map(str::to_owned),
                mime: mime.to_ascii_lowercase(),
                size: part.len(),
                inline,
            }
        })
        .collect();
    Body {
        text,
        has_html,
        attachments,
    }
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
            }]
        );
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
}
