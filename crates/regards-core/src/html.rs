//! HTML mail, made safe to show (ADR-0006 §4).
//!
//! This is the first of three layers. Sanitising with `ammonia` removes
//! scripts, event handlers, forms, frames and dangerous links; inline
//! (`cid:`) images become `data:` URIs; remote images are removed unless the
//! reader allows them. The desktop then serves the result with a strict CSP
//! inside a sandboxed iframe without scripts.

use ammonia::{Builder, UrlRelative};
use mail_parser::{MessageParser, MimeHeaders};
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Inline images larger than this are left out.
const MAX_INLINE_IMAGE: usize = 2 * 1024 * 1024;
/// At most this much inline image data per message.
const MAX_INLINE_TOTAL: usize = 10 * 1024 * 1024;

/// Base styles: mail is designed for a light background, whatever the app theme.
const FRAME_STYLE: &str = "html{color-scheme:light}body{margin:0;padding:16px;\
    font:15px/1.5 system-ui,-apple-system,'Segoe UI',Roboto,sans-serif;color:#1d1d1b;\
    background:#fff;overflow-wrap:anywhere}img{max-width:100%;height:auto}";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    /// A complete document, ready to serve.
    pub html: String,
    /// Remote images (and other remote resources) that were removed.
    pub blocked_remote: usize,
}

/// The sanitised HTML version of a message, or `None` if it has none.
pub fn render(raw: &[u8], allow_remote: bool) -> Option<Rendered> {
    let message = MessageParser::default().parse(raw)?;
    let part = message.html_part(0).filter(|part| part.is_text_html())?;
    let html = part.text_contents()?;

    let mut inline = HashMap::new();
    let mut total = 0;
    for attachment in message.attachments() {
        let (Some(cid), Some(content_type)) = (attachment.content_id(), attachment.content_type()) else {
            continue;
        };
        if !content_type.ctype().eq_ignore_ascii_case("image") {
            continue;
        }
        let bytes = attachment.contents();
        if bytes.len() > MAX_INLINE_IMAGE || total + bytes.len() > MAX_INLINE_TOTAL {
            continue;
        }
        total += bytes.len();
        let mime = format!("image/{}", content_type.subtype().unwrap_or("png").to_ascii_lowercase());
        let key = cid.trim_matches(|c| c == '<' || c == '>').to_owned();
        inline.insert(key, format!("data:{mime};base64,{}", base64(bytes)));
    }

    let (body, blocked_remote) = sanitize(html, allow_remote, &inline);
    Some(Rendered {
        html: format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><style>{FRAME_STYLE}</style></head><body>{body}</body></html>"
        ),
        blocked_remote,
    })
}

/// Sanitise an HTML fragment; returns it and how many remote resources were removed.
pub fn sanitize(html: &str, allow_remote: bool, inline: &HashMap<String, String>) -> (String, usize) {
    let blocked = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&blocked);
    let inline = Arc::new(inline.clone());
    let clean = Builder::default()
        .add_tags(["style", "font"])
        // Mail layout leans on presentational attributes; none of them can run code.
        .add_generic_attributes([
            "style",
            "class",
            "align",
            "valign",
            "width",
            "height",
            "bgcolor",
            "color",
            "border",
            "cellpadding",
            "cellspacing",
            "dir",
            "lang",
            "title",
            "background",
        ])
        .add_tag_attributes("font", ["face", "size", "color"])
        .rm_clean_content_tags(["style"])
        .add_url_schemes(["cid", "data"])
        .url_relative(UrlRelative::Deny)
        .link_rel(Some("noopener noreferrer"))
        .strip_comments(true)
        .attribute_filter(move |element, attribute, value| {
            let lower = value.trim_start().to_ascii_lowercase();
            match attribute {
                "src" | "background" => {
                    if let Some(cid) = lower.strip_prefix("cid:") {
                        // Content-IDs compare case-sensitively in the map; try both forms.
                        let original = &value.trim_start()[4..];
                        return inline
                            .get(original)
                            .or_else(|| inline.get(cid))
                            .map(|uri| Cow::Owned(uri.clone()));
                    }
                    if lower.starts_with("data:") {
                        // Only images may be embedded.
                        return (element == "img" && lower.starts_with("data:image/")).then(|| value.into());
                    }
                    if lower.starts_with("http:") || lower.starts_with("https:") {
                        if allow_remote {
                            return Some(value.into());
                        }
                        counter.fetch_add(1, Ordering::Relaxed);
                        return None;
                    }
                    None
                }
                // Only ordinary links: no data: or cid: targets.
                "href" => (lower.starts_with("http:") || lower.starts_with("https:") || lower.starts_with("mailto:"))
                    .then(|| value.into()),
                _ => Some(value.into()),
            }
        })
        .clean(html)
        .to_string();
    let count = blocked.load(Ordering::Relaxed);
    (clean, count)
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(html: &str, allow_remote: bool) -> (String, usize) {
        sanitize(html, allow_remote, &HashMap::new())
    }

    #[test]
    fn scripts_handlers_forms_and_frames_are_removed() {
        let (html, _) = clean(
            "<p onclick=\"steal()\">Hi</p><script>steal()</script>\
             <iframe src=\"https://evil.test\"></iframe><form action=\"https://evil.test\"><input name=p></form>\
             <object data=\"x\"></object><a href=\"javascript:steal()\">x</a><a href=\"data:text/html,hi\">y</a>",
            true,
        );
        for forbidden in [
            "onclick",
            "<script",
            "steal()",
            "<iframe",
            "<form",
            "<input",
            "<object",
            "javascript:",
            "data:text",
        ] {
            assert!(!html.contains(forbidden), "{forbidden} survived in {html}");
        }
        assert!(html.contains("<p>Hi</p>"));
    }

    #[test]
    fn remote_images_are_blocked_and_counted_unless_allowed() {
        let source = "<img src=\"https://tracker.test/pixel.gif\"><table background=\"http://cdn.test/bg.png\"><tr><td>x</td></tr></table>";
        let (blocked, count) = clean(source, false);
        assert_eq!(count, 2);
        assert!(!blocked.contains("tracker.test") && !blocked.contains("cdn.test"));
        let (allowed, count) = clean(source, true);
        assert_eq!(count, 0);
        assert!(allowed.contains("https://tracker.test/pixel.gif"));
    }

    #[test]
    fn styles_and_layout_survive_and_links_are_safe() {
        let (html, _) = clean(
            "<style>p { color: red }</style><table width=\"600\" bgcolor=\"#eee\"><tr><td style=\"padding:4px\">\
             <font face=\"Arial\">Hello</font> <a href=\"https://example.test\">site</a></td></tr></table>",
            false,
        );
        assert!(html.contains("<style>p { color: red }</style>"), "{html}");
        assert!(html.contains("width=\"600\"") && html.contains("style=\"padding:4px\""));
        assert!(html.contains("<font face=\"Arial\">"));
        assert!(html.contains("rel=\"noopener noreferrer\""));
    }

    #[test]
    fn inline_images_become_data_uris_and_other_data_is_refused() {
        let mut inline = HashMap::new();
        inline.insert("logo@example.test".to_owned(), "data:image/png;base64,AAAA".to_owned());
        let (html, _) = sanitize(
            "<img src=\"cid:logo@example.test\"><img src=\"cid:missing\"><img src=\"data:text/html;base64,PHNjcmlwdD4=\">",
            false,
            &inline,
        );
        assert!(html.contains("src=\"data:image/png;base64,AAAA\""), "{html}");
        assert!(!html.contains("cid:"));
        assert!(!html.contains("data:text"));
    }

    #[test]
    fn whole_messages_render_with_their_inline_images() {
        let raw = b"MIME-Version: 1.0\r\nContent-Type: multipart/related; boundary=\"r\"\r\n\r\n\
            --r\r\nContent-Type: text/html; charset=utf-8\r\n\r\n\
            <p>Hi <img src=\"cid:pic@x\"> <img src=\"https://t.test/p.gif\"></p>\r\n\
            --r\r\nContent-Type: image/png\r\nContent-ID: <pic@x>\r\nContent-Transfer-Encoding: base64\r\n\r\n\
            iVBORw0KGgo=\r\n--r--\r\n";
        let rendered = render(raw, false).unwrap();
        assert_eq!(rendered.blocked_remote, 1);
        assert!(rendered.html.starts_with("<!doctype html>"));
        assert!(
            rendered.html.contains("data:image/png;base64,iVBORw0KGgo="),
            "{}",
            rendered.html
        );
        assert!(render(b"Content-Type: text/plain\r\n\r\nno html\r\n", false).is_none());
    }

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
}
