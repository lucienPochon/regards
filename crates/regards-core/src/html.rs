//! HTML mail, made safe to show (ADR-0006 §4).
//!
//! This is the first of three layers. Sanitising with `ammonia` removes
//! scripts, event handlers, forms, frames and dangerous links; inline
//! (`cid:`) images become `data:` URIs; remote images are removed unless the
//! reader allows them. The desktop then serves the result with a strict CSP
//! inside a sandboxed iframe without scripts.
//!
//! The frame cannot leave the app, so web links can be pointed back at its
//! host instead (`links`): `<prefix><percent-encoded URL>`, opened in a hidden
//! frame inside the document ([`LINK_TARGET`]) so the message stays in view
//! whatever the host answers. The host then opens the URL in the browser.

use crate::encoding::base64;
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

/// The hidden frame that rewritten links open in (see the module docs).
pub const LINK_TARGET: &str = "regards-links";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    /// A complete document, ready to serve.
    pub html: String,
    /// Remote images (and other remote resources) that were removed.
    pub blocked_remote: usize,
}

/// The sanitised HTML version of a message, or `None` if it has none. With
/// `links`, web links become `<links><percent-encoded URL>`.
pub fn render(raw: &[u8], allow_remote: bool, links: Option<&str>) -> Option<Rendered> {
    let message = MessageParser::default().parse(raw)?;
    let part = message.html_part(0).filter(|part| part.is_text_html())?;
    let html = part.text_contents()?;
    if start_tags(html) > MAX_TAGS {
        let text = message.body_text(0).unwrap_or_default();
        return Some(Rendered {
            html: document(
                "",
                &format!(
                    "<p><em>This message's formatting is too complex to show safely, so here is its text.</em></p>\
                     <pre style=\"white-space:pre-wrap\">{}</pre>",
                    escape(&text)
                ),
            ),
            blocked_remote: 0,
        });
    }

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

    let (body, blocked_remote) = sanitize(html, allow_remote, &inline, links);
    // The message's own <base> and frames were removed; these are ours.
    let html = if links.is_some() {
        document(
            &format!("<base target=\"{LINK_TARGET}\">"),
            &format!("{body}<iframe name=\"{LINK_TARGET}\" hidden></iframe>"),
        )
    } else {
        document("", &body)
    };
    Some(Rendered { html, blocked_remote })
}

/// Formatting a message written in Regards may carry: text styles, lists,
/// quotes, headings, web or mail links, and its own images (`cid:`, sent
/// with it). Everything else (scripts, styles, images from the web, frames,
/// forms, event handlers) is removed before sending.
pub fn sanitize_outgoing(html: &str) -> String {
    let clean = Builder::empty()
        .add_tags([
            "p",
            "br",
            "strong",
            "b",
            "em",
            "i",
            "u",
            "s",
            "a",
            "ul",
            "ol",
            "li",
            "blockquote",
            "code",
            "pre",
            "h1",
            "h2",
            "h3",
            "hr",
            "img",
        ])
        .add_tag_attributes("a", ["href"])
        .add_tag_attributes("img", ["src", "alt"])
        .add_url_schemes(["http", "https", "mailto", "cid"])
        // An image is one of the message's own, never fetched from the web.
        .attribute_filter(|element, attribute, value| match (element, attribute) {
            ("img", "src") if !value.starts_with("cid:") => None,
            ("a", "href") if value.starts_with("cid:") => None,
            _ => Some(value.into()),
        })
        .url_relative(UrlRelative::Deny)
        .link_rel(None)
        .strip_comments(true)
        .clean(html)
        .to_string();
    without_empty_images(&clean)
}

/// Images left without a source (it was from the web) are taken out. The
/// sanitiser's output is regular: attributes quoted, tags well formed.
fn without_empty_images(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(at) = rest.find("<img") {
        out.push_str(&rest[..at]);
        let tag_end = rest[at..].find('>').map_or(rest.len(), |end| at + end + 1);
        let tag = &rest[at..tag_end];
        if tag.contains(" src=\"") {
            out.push_str(tag);
        }
        rest = &rest[tag_end..];
    }
    out.push_str(rest);
    out
}

/// The plain-text version of written HTML, for the `text/plain` part.
pub fn to_text(html: &str) -> String {
    mail_parser::decoders::html::html_to_text(html)
}

/// A complete frame document around sanitised `body`.
fn document(head: &str, body: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">{head}<style>{FRAME_STYLE}</style></head>\
         <body>{body}</body></html>"
    )
}

/// More start tags than this and the HTML is not shown. Parsing and
/// sanitising cost grows with the square of the nesting depth, which a hostile
/// message can make as large as its tag count: 20,000 nested elements took
/// 0.7 s, 200,000 (a 1 MB message) would take over a minute. Real mail stays
/// far below; at this limit the worst case is about 0.4 s.
const MAX_TAGS: usize = 15_000;

fn start_tags(html: &str) -> usize {
    html.as_bytes()
        .windows(2)
        .filter(|pair| pair[0] == b'<' && pair[1].is_ascii_alphabetic())
        .count()
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// Sanitise an HTML fragment; returns it and how many remote resources were removed.
pub fn sanitize(
    html: &str,
    allow_remote: bool,
    inline: &HashMap<String, String>,
    links: Option<&str>,
) -> (String, usize) {
    let blocked = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&blocked);
    let inline = Arc::new(inline.clone());
    let links = links.map(str::to_owned);
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
        // `noopener` would stop links reaching the named link frame. Rewritten
        // links only lead back to the host, and the frame can open no windows.
        .link_rel(if links.is_some() {
            None
        } else {
            Some("noopener noreferrer")
        })
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
                "href" => {
                    if lower.starts_with("http:") || lower.starts_with("https:") {
                        return Some(match &links {
                            Some(prefix) => Cow::Owned(format!("{prefix}{}", percent_encode(value.trim()))),
                            None => value.into(),
                        });
                    }
                    lower.starts_with("mailto:").then(|| value.into())
                }
                _ => Some(value.into()),
            }
        })
        .clean(html)
        .to_string();
    let count = blocked.load(Ordering::Relaxed);
    (clean, count)
}

/// Everything but unreserved characters (RFC 3986 §2.3) is encoded, so the
/// result is safe as one query parameter.
pub fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_carries_its_own_images_never_ones_from_the_web() {
        let written = r#"<p>Hi <img src="cid:lake@regards" alt="Lake"><img src="https://tracker.example/p.gif"><img src="data:image/png;base64,AAAA"> <a href="cid:x">x</a></p>"#;
        let clean = sanitize_outgoing(written);
        assert!(clean.contains(r#"<img src="cid:lake@regards" alt="Lake">"#), "{clean}");
        assert!(
            !clean.contains("tracker.example") && !clean.contains("data:"),
            "{clean}"
        );
        assert!(!clean.contains("href=\"cid:"), "{clean}");
    }

    fn clean(html: &str, allow_remote: bool) -> (String, usize) {
        sanitize(html, allow_remote, &HashMap::new(), None)
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
            None,
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
        let rendered = render(raw, false, None).unwrap();
        assert_eq!(rendered.blocked_remote, 1);
        assert!(rendered.html.starts_with("<!doctype html>"));
        assert!(
            rendered.html.contains("data:image/png;base64,iVBORw0KGgo="),
            "{}",
            rendered.html
        );
        assert!(render(b"Content-Type: text/plain\r\n\r\nno html\r\n", false, None).is_none());
    }

    #[test]
    fn web_links_can_point_back_at_the_frame_host() {
        let source = "<a href=\"https://example.test/a?b=1&amp;c=d e\">site</a> <a href=\" HTTP://x.test\">x</a> \
             <a href=\"mailto:ada@example.test\">mail</a> <a href=\"javascript:steal()\">bad</a> \
             <a href=\"regards-mail://localhost/open?t=forged&amp;url=x\">forged</a>";
        let (html, _) = sanitize(
            source,
            false,
            &HashMap::new(),
            Some("regards-mail://localhost/open?t=k&url="),
        );
        // The attribute value was entity-decoded: `&amp;` is `&`, encoded here as %26.
        assert!(
            html.contains(
                "href=\"regards-mail://localhost/open?t=k&amp;url=https%3A%2F%2Fexample.test%2Fa%3Fb%3D1%26c%3Dd%20e\""
            ),
            "{html}"
        );
        assert!(html.contains("url=HTTP%3A%2F%2Fx.test\""), "{html}");
        assert!(html.contains("href=\"mailto:ada@example.test\""));
        assert!(!html.contains("javascript:") && !html.contains("forged&"), "{html}");
        assert!(!html.contains("noopener"), "{html}");

        let (plain, _) = clean("<a href=\"https://example.test/\">site</a>", false);
        assert!(plain.contains("href=\"https://example.test/\""));
        assert!(plain.contains("rel=\"noopener noreferrer\""));
    }

    #[test]
    fn rewritten_links_open_in_the_hidden_link_frame() {
        let raw = b"Content-Type: text/html\r\n\r\n<base target=\"_top\" href=\"https://evil.test/\">\
            <a href=\"https://example.test/\" target=\"_top\">site</a>\r\n";
        let framed = render(raw, false, Some("regards-mail://localhost/open?t=k&url="))
            .unwrap()
            .html;
        assert!(
            framed.contains("<head><meta charset=\"utf-8\"><base target=\"regards-links\"><style>"),
            "{framed}"
        );
        assert!(framed.ends_with("<iframe name=\"regards-links\" hidden></iframe></body></html>"));
        assert_eq!(
            framed.matches("<base").count(),
            1,
            "the message's own base is gone: {framed}"
        );
        assert!(!framed.contains("_top") && !framed.contains("evil.test"), "{framed}");

        let plain = render(raw, false, None).unwrap().html;
        assert!(!plain.contains("<base") && !plain.contains("<iframe"), "{plain}");
    }

    #[test]
    fn hostile_nesting_shows_the_text_instead_and_quickly() {
        let raw = format!(
            "Content-Type: multipart/alternative; boundary=\"a\"\r\n\r\n\
             --a\r\nContent-Type: text/plain\r\n\r\nThe <real> text & more\r\n\
             --a\r\nContent-Type: text/html\r\n\r\n{}x\r\n--a--\r\n",
            "<div>".repeat(MAX_TAGS + 1)
        );
        let started = std::time::Instant::now();
        let rendered = render(raw.as_bytes(), false, None).unwrap();
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
        assert!(rendered.html.contains("too complex to show safely"));
        assert!(
            rendered.html.contains("The &lt;real&gt; text &amp; more"),
            "{}",
            rendered.html
        );
        assert!(!rendered.html.contains("<div>"));

        // Ordinary mail is far below the limit.
        assert_eq!(
            start_tags("<table><tr><td><p>Hi <b>there</b></p></td></tr></table> a < b"),
            5
        );
    }

    #[test]
    fn written_html_keeps_formatting_and_loses_everything_else() {
        let written = "<p>Hello <strong>Bob</strong>, <em>see</em> <a href=\"https://example.test\" onclick=\"x()\">this</a> \
            and <a href=\"javascript:alert(1)\">that</a>.</p><ul><li>one</li></ul><blockquote><p>quoted</p></blockquote>\
            <script>steal()</script><img src=\"https://t.test/p.gif\"><p style=\"color:red\">styled</p>";
        let clean = sanitize_outgoing(written);
        assert!(
            clean.contains("<strong>Bob</strong>") && clean.contains("<em>see</em>"),
            "{clean}"
        );
        assert!(clean.contains("<a href=\"https://example.test\">this</a>"), "{clean}");
        assert!(
            clean.contains("<ul><li>one</li></ul>") && clean.contains("<blockquote>"),
            "{clean}"
        );
        for gone in ["onclick", "javascript", "<script", "steal", "<img", "style="] {
            assert!(!clean.contains(gone), "{gone} survived: {clean}");
        }
        let text = to_text(&clean);
        assert!(
            text.contains("Hello Bob") && text.contains("one") && text.contains("quoted"),
            "{text}"
        );
    }

    #[test]
    fn percent_encoding_keeps_only_unreserved_characters() {
        assert_eq!(percent_encode("aZ09-._~"), "aZ09-._~");
        assert_eq!(percent_encode("a b&c=d/é"), "a%20b%26c%3Dd%2F%C3%A9");
    }
}
