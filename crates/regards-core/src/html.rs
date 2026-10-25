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

/// Base styles: mail is designed for a light background, whatever the app
/// theme (the desktop darkens the whole page when asked). The folded quoted
/// history ([`fold_quotes`]) shows as a small "•••" pill.
const FRAME_STYLE: &str = "html{color-scheme:light}body{margin:0;padding:16px;\
    font:15px/1.5 system-ui,-apple-system,'Segoe UI',Roboto,sans-serif;color:#1d1d1b;\
    background:#fff;overflow-wrap:anywhere}img{max-width:100%;height:auto}\
    details.regards-quote>summary{list-style:none;display:inline-block;margin:8px 0;padding:0 9px;\
    border-radius:9px;background:#e9eae8;color:#5c605d;font-size:13px;line-height:19px;cursor:pointer}\
    details.regards-quote>summary::-webkit-details-marker{display:none}\
    details.regards-quote[open]>summary{margin-bottom:4px}";

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
    render_with(raw, allow_remote, links, false)
}

/// [`render`]; with `clean_links`, links lose their tracking parameters ([`clean_link`]).
pub fn render_with(raw: &[u8], allow_remote: bool, links: Option<&str>, clean_links: bool) -> Option<Rendered> {
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

    let (body, blocked_remote) = sanitize(html, allow_remote, &inline, links, clean_links);
    let body = fold_quotes(&body);
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
/// quotes, headings, web or mail links, a colour or highlight on some text,
/// and its own images (`cid:`, sent with it), at a size. Everything else
/// (scripts, other styles, images from the web, frames, forms, event
/// handlers) is removed before sending.
pub fn sanitize_outgoing(html: &str) -> String {
    written(html, Pictures::Attached)
}

/// A signature's formatting, as [`sanitize_outgoing`] keeps a message's,
/// its pictures embedded instead: `data:` PNG, JPEG, GIF or WebP, each
/// [`SIGNATURE_PICTURE_MAX`] bytes of base64 at most. They become the
/// message's own images when the signature goes into one.
pub fn sanitize_signature(html: &str) -> String {
    written(html, Pictures::Embedded)
}

/// Base64 characters a signature's picture may have at most.
pub const SIGNATURE_PICTURE_MAX: usize = 256 * 1024;

/// Where written HTML's pictures may come from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pictures {
    /// The message's own images, by Content-ID.
    Attached,
    /// Inside the HTML, as a signature keeps them.
    Embedded,
}

/// A `data:` URI of a picture a signature may embed.
fn embedded_picture(value: &str) -> bool {
    ["image/png", "image/jpeg", "image/gif", "image/webp"]
        .iter()
        .any(|mime| {
            value
                .strip_prefix("data:")
                .and_then(|rest| rest.strip_prefix(mime))
                .and_then(|rest| rest.strip_prefix(";base64,"))
                .is_some_and(|data| {
                    data.len() <= SIGNATURE_PICTURE_MAX
                        && data.bytes().all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b))
                })
        })
}

fn written(html: &str, pictures: Pictures) -> String {
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
            "span",
        ])
        .add_tag_attributes("a", ["href"])
        .add_tag_attributes("img", ["src", "alt", "width", "height"])
        .add_tag_attributes("span", ["style"])
        // `type="cite"`: quoted history, which readers fold (fold_quotes).
        .add_tag_attributes("blockquote", ["type"])
        .add_url_schemes(["http", "https", "mailto", "cid", "data"])
        // An image is one of the message's own, never fetched from the web.
        .attribute_filter(move |element, attribute, value| match (element, attribute) {
            ("img", "src") if pictures == Pictures::Attached && !value.starts_with("cid:") => None,
            ("img", "src") if pictures == Pictures::Embedded && !embedded_picture(value) => None,
            ("img", "width" | "height") => pixels(value).map(Into::into),
            ("a", "href") if value.starts_with("data:") => None,
            ("span", "style") => colours(value).map(Into::into),
            ("blockquote", "type") if value != "cite" => None,
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

/// An image's width or height as whole pixels, 1 to 9999; `None` otherwise.
fn pixels(value: &str) -> Option<String> {
    let value = value.trim();
    let whole = (1..=4).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_digit());
    (whole && value.parse::<u16>().is_ok_and(|n| n > 0)).then(|| value.to_owned())
}

/// Of a span's style, only its text colour and highlight, each a six-digit
/// hex colour (as the composer offers them); `None` when nothing is left.
fn colours(style: &str) -> Option<String> {
    let kept: Vec<String> = style
        .split(';')
        .filter_map(|declaration| {
            let (name, value) = declaration.split_once(':')?;
            let (name, value) = (name.trim().to_ascii_lowercase(), value.trim());
            let hex = value.len() == 7 && value.starts_with('#') && value[1..].bytes().all(|b| b.is_ascii_hexdigit());
            (hex && (name == "color" || name == "background-color"))
                .then(|| format!("{name}: {}", value.to_ascii_lowercase()))
        })
        .collect();
    (!kept.is_empty()).then(|| kept.join("; "))
}

/// A message's HTML, to quote under a reply or a forward: as
/// [`sanitize_outgoing`] keeps formatting, without its pictures (a forward
/// carries them as files). `None` when it has no HTML version.
pub fn quotable(raw: &[u8]) -> Option<String> {
    let message = MessageParser::default().parse(raw)?;
    let part = message.html_part(0).filter(|part| part.is_text_html())?;
    let html = part.text_contents()?;
    if start_tags(html) > MAX_TAGS {
        return None;
    }
    let kept = sanitize_outgoing(html);
    let mut out = String::with_capacity(kept.len());
    let mut rest = kept.as_str();
    while let Some(at) = rest.find("<img") {
        out.push_str(&rest[..at]);
        rest = rest[at..].find('>').map_or("", |end| &rest[at + end + 1..]);
    }
    out.push_str(rest);
    Some(out)
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
    clean_links: bool,
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
            // Kept only where they mark quoted history (QUOTE_IDS), for fold_quotes.
            "id",
        ])
        .add_tag_attributes("font", ["face", "size", "color"])
        // `type="cite"` marks quoted history (Apple Mail, Thunderbird).
        .add_tag_attributes("blockquote", ["type"])
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
                        let target = if clean_links {
                            clean_link(value.trim())
                        } else {
                            value.trim().to_owned()
                        };
                        return Some(match &links {
                            Some(prefix) => Cow::Owned(format!("{prefix}{}", percent_encode(&target))),
                            None => Cow::Owned(target),
                        });
                    }
                    lower.starts_with("mailto:").then(|| value.into())
                }
                "id" => QUOTE_IDS.contains(&value).then(|| value.into()),
                "type" if element == "blockquote" => (lower == "cite").then(|| value.into()),
                _ => Some(value.into()),
            }
        })
        .clean(html)
        .to_string();
    let count = blocked.load(Ordering::Relaxed);
    (clean, count)
}

/// Classes mail clients put on the quoted history of a reply (Gmail, Yahoo,
/// Proton; Thunderbird's "… wrote:" line before its quote).
const QUOTE_CLASSES: &[&str] = &["gmail_quote", "yahoo_quoted", "protonmail_quote", "moz-cite-prefix"];
/// Where Outlook starts the quoted history: everything after it is quoted.
const QUOTE_IDS: &[&str] = &["divRplyFwdMsg", "appendonsend"];
/// How a forwarded message begins, in the languages mail clients write it
/// in: that is what was sent, not history, and stays open.
const FORWARDED: &[&str] = &[
    "forwarded message",
    "message transféré",
    "weitergeleitete nachricht",
    "mensaje reenviado",
    "messaggio inoltrato",
    // Outlook's header of a forward.
    "subject: fw:",
    "subject: fwd:",
    "objet : tr :",
    "objet : tr:",
    "betreff: wg:",
];

/// The quoted history at the end of a reply, folded behind "•••" (a
/// `<details>`, so it opens without a script). Only when something was
/// written above it and nothing below (not a reply written between quotes),
/// and not a forward. `html` is the sanitiser's output: tags well formed,
/// attributes in double quotes.
fn fold_quotes(html: &str) -> String {
    let Some((start, end)) = quoted_history(html) else {
        return html.to_owned();
    };
    let quote = &html[start..end];
    let opening = visible_text(quote)
        .split_whitespace()
        .take(80)
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    if !has_text(&html[..start]) || has_text(&html[end..]) || FORWARDED.iter().any(|f| opening.contains(f)) {
        return html.to_owned();
    }
    format!(
        "{}<details class=\"regards-quote\"><summary title=\"Show the quoted text\" aria-label=\"Show the quoted text\">\
         •••</summary>{quote}</details>{}",
        &html[..start],
        &html[end..]
    )
}

/// Where the first quoted history starts and ends.
fn quoted_history(html: &str) -> Option<(usize, usize)> {
    let mut at = 0;
    while let Some(found) = html[at..].find('<') {
        let start = at + found;
        let tag = start_tag(html, start)?;
        at = start + tag.len;
        let Some(name) = tag.name else { continue };
        let class = attribute(tag.text, "class");
        let classed = class.is_some_and(|class| class.split_whitespace().any(|c| QUOTE_CLASSES.contains(&c)));
        let cited = name == "blockquote" && attribute(tag.text, "type") == Some("cite");
        if attribute(tag.text, "id").is_some_and(|id| QUOTE_IDS.contains(&id)) {
            // Outlook: from here to the end.
            return Some((start, html.len()));
        }
        if !(classed || cited) {
            continue;
        }
        let mut end = element_end(html, start + tag.len, name)?;
        // Thunderbird: the "… wrote:" line, then the quote itself.
        if class.is_some_and(|class| class.split_whitespace().any(|c| c == "moz-cite-prefix")) {
            let next = end + (html.len() - end - html[end..].trim_start().len());
            if html[next..].starts_with("<blockquote") {
                let quote = start_tag(html, next)?;
                end = element_end(html, next + quote.len, "blockquote")?;
            }
        }
        return Some((start, end));
    }
    None
}

struct StartTag<'a> {
    /// `None` for end tags, comments and the like.
    name: Option<&'a str>,
    /// The whole tag, `<` to `>`.
    text: &'a str,
    len: usize,
}

/// The tag at `start` (a `<`), read up to its `>` outside quotes.
fn start_tag(html: &str, start: usize) -> Option<StartTag<'_>> {
    let rest = &html[start..];
    let mut quoted = false;
    let close = rest.char_indices().skip(1).find_map(|(i, c)| match c {
        '"' => {
            quoted = !quoted;
            None
        }
        '>' if !quoted => Some(i),
        _ => None,
    })?;
    let text = &rest[..=close];
    let name_end = text[1..]
        .find(|c: char| !c.is_ascii_alphanumeric())
        .map_or(text.len(), |i| i + 1);
    let name = &text[1..name_end];
    Some(StartTag {
        name: (!name.is_empty() && name.as_bytes()[0].is_ascii_alphabetic()).then_some(name),
        text,
        len: text.len(),
    })
}

/// An attribute's value in a start tag, as the sanitiser writes them.
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let marker = format!(" {name}=\"");
    let from = tag.find(&marker)? + marker.len();
    let to = tag[from..].find('"')? + from;
    Some(&tag[from..to])
}

/// Just after the end tag that closes the `name` element whose content starts at `from`.
fn element_end(html: &str, from: usize, name: &str) -> Option<usize> {
    let (open, close) = (format!("<{name}"), format!("</{name}>"));
    let mut depth = 1;
    let mut at = from;
    loop {
        let next_close = html[at..].find(&close)? + at;
        // Other elements of the same name opened before it.
        let mut scan = at;
        while let Some(found) = html[scan..next_close].find(&open) {
            let begin = scan + found;
            let after = html.as_bytes().get(begin + open.len()).copied();
            if matches!(after, Some(b' ' | b'>' | b'/')) {
                depth += 1;
            }
            scan = begin + open.len();
        }
        depth -= 1;
        at = next_close + close.len();
        if depth == 0 {
            return Some(at);
        }
    }
}

/// The text of an HTML fragment: no tags, no style sheets, no entities.
fn visible_text(html: &str) -> String {
    let mut text = String::new();
    let mut rest = html;
    while !rest.is_empty() {
        if rest.starts_with("<style") {
            rest = rest.find("</style>").map_or("", |end| &rest[end + 8..]);
        } else if rest.starts_with('<') {
            rest = rest.find('>').map_or("", |end| &rest[end + 1..]);
        } else if rest.starts_with('&') {
            text.push(' ');
            rest = rest
                .find(';')
                .filter(|&end| end < 10)
                .map_or(&rest[1..], |end| &rest[end + 1..]);
        } else {
            let c = rest.chars().next().unwrap_or(' ');
            text.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    text
}

/// Whether a fragment says anything: letters or digits outside its tags.
fn has_text(html: &str) -> bool {
    visible_text(html).chars().any(char::is_alphanumeric)
}

/// Query parameters that only say who clicked and from which campaign
/// (ADR-0015 §4); `utm_` parameters too, whatever follows.
const TRACKING: &[&str] = &[
    "fbclid",
    "gclid",
    "dclid",
    "gbraid",
    "wbraid",
    "msclkid",
    "yclid",
    "ttclid",
    "twclid",
    "li_fat_id",
    "mc_cid",
    "mc_eid",
    "_hsenc",
    "_hsmi",
    "mkt_tok",
    "igshid",
    "oly_anon_id",
    "oly_enc_id",
    "vero_id",
    "vero_conv",
    "_kx",
];

/// A web link without its tracking parameters; the rest of it, the order
/// of what stays, and its `#fragment` are kept. It does not follow
/// redirects: a link through a click-counting address stays one.
pub fn clean_link(url: &str) -> String {
    let (before_fragment, fragment) = match url.split_once('#') {
        Some((rest, fragment)) => (rest, Some(fragment)),
        None => (url, None),
    };
    let Some((address, query)) = before_fragment.split_once('?') else {
        return url.to_owned();
    };
    let kept: Vec<&str> = query
        .split('&')
        .filter(|pair| {
            let name = pair.split('=').next().unwrap_or("").to_ascii_lowercase();
            !pair.is_empty() && !name.starts_with("utm_") && !TRACKING.contains(&name.as_str())
        })
        .collect();
    let mut cleaned = address.to_owned();
    if !kept.is_empty() {
        cleaned.push('?');
        cleaned.push_str(&kept.join("&"));
    }
    if let Some(fragment) = fragment {
        cleaned.push('#');
        cleaned.push_str(fragment);
    }
    cleaned
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
    fn links_lose_their_tracking_but_nothing_else() {
        assert_eq!(
            clean_link("https://shop.example/item?id=7&utm_source=news&UTM_Medium=mail&fbclid=abc#reviews"),
            "https://shop.example/item?id=7#reviews"
        );
        assert_eq!(
            clean_link("https://example.test/?utm_campaign=x"),
            "https://example.test/"
        );
        assert_eq!(
            clean_link("https://example.test/a?b=1&c=d"),
            "https://example.test/a?b=1&c=d"
        );
        assert_eq!(clean_link("https://example.test/plain"), "https://example.test/plain");
        assert_eq!(
            clean_link("https://example.test/?mc_eid=1&q=utm_source"),
            "https://example.test/?q=utm_source"
        );
        let source = "<a href=\"https://example.test/a?utm_source=x&amp;b=1\">a</a>";
        let (kept, _) = sanitize(source, false, &HashMap::new(), None, false);
        assert!(kept.contains("utm_source"), "only when asked: {kept}");
        let (cleaned, _) = sanitize(source, false, &HashMap::new(), None, true);
        assert!(cleaned.contains("href=\"https://example.test/a?b=1\""), "{cleaned}");
    }

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
        sanitize(html, allow_remote, &HashMap::new(), None, false)
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
            false,
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
            false,
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
    fn a_signature_keeps_its_embedded_pictures_and_formatting_only() {
        let signature = "<p><strong>Ada</strong> <span style=\"color: #1565c0\">Engines Ltd</span></p>\
            <p><img src=\"data:image/png;base64,iVBORw0KGgo=\" alt=\"Logo\" width=\"120\"></p>\
            <img src=\"https://tracker.test/p.gif\"><img src=\"cid:x@regards\">\
            <img src=\"data:text/html;base64,PHNjcmlwdD4=\"><img src=\"data:image/svg+xml;base64,PHN2Zz4=\">\
            <a href=\"data:text/html,hi\">bad link</a><script>steal()</script>";
        let clean = sanitize_signature(signature);
        assert!(
            clean.contains("<img src=\"data:image/png;base64,iVBORw0KGgo=\" alt=\"Logo\" width=\"120\">"),
            "{clean}"
        );
        assert!(
            clean.contains("<strong>Ada</strong>") && clean.contains("color: #1565c0"),
            "{clean}"
        );
        for gone in ["tracker", "cid:", "text/html", "svg", "href", "script", "steal"] {
            assert!(!clean.contains(gone), "{gone} survived: {clean}");
        }
        assert!(clean.contains("bad link"), "the words stay: {clean}");
        // A message's own HTML still takes no embedded pictures.
        assert!(
            !sanitize_outgoing(signature).contains("data:"),
            "{}",
            sanitize_outgoing(signature)
        );
    }

    #[test]
    fn written_colours_and_picture_sizes_go_out_and_nothing_more() {
        let written = "<p><span style=\"color: #C62828; background-color: #fff59d\">urgent</span> \
            <span style=\"color: #1565c0; font-size: 40px; position: fixed\">blue</span> \
            <span style=\"color: red\">named</span> <span style=\"background: url(https://t.test/x)\">sneaky</span> \
            <img src=\"cid:lake@regards\" alt=\"\" width=\"320\" height=\"0\"> \
            <img src=\"cid:hill@regards\" alt=\"\" width=\"100%\" height=\"180\"></p>";
        let clean = sanitize_outgoing(written);
        assert!(
            clean.contains("<span style=\"color: #c62828; background-color: #fff59d\">urgent</span>"),
            "{clean}"
        );
        assert!(clean.contains("<span style=\"color: #1565c0\">blue</span>"), "{clean}");
        for gone in ["font-size", "position", "red", "url(", "100%", "height=\"0\""] {
            assert!(!clean.contains(gone), "{gone} survived: {clean}");
        }
        assert!(
            clean.contains("named") && clean.contains("sneaky"),
            "the words stay: {clean}"
        );
        assert!(
            clean.contains("width=\"320\"") && clean.contains("height=\"180\""),
            "{clean}"
        );
    }

    /// Sanitised, then folded, as messages are shown.
    fn folded(html: &str) -> String {
        fold_quotes(&clean(html, false).0)
    }

    const FOLD: &str = "<details class=\"regards-quote\">";

    #[test]
    fn the_quoted_history_under_a_reply_is_folded_as_each_client_marks_it() {
        // Gmail: its container, the "… wrote:" line and the quote, nested divs and all.
        let gmail = folded(
            "<div dir=\"ltr\">Sounds good</div><br><div class=\"gmail_quote gmail_quote_container\">\
             <div dir=\"ltr\" class=\"gmail_attr\">On Mon, Ada wrote:<br></div>\
             <blockquote class=\"gmail_quote\"><div><div>Lunch?</div></div></blockquote></div>",
        );
        let at = gmail.find(FOLD).expect(&gmail);
        assert!(gmail[..at].contains("Sounds good"), "{gmail}");
        assert!(
            gmail[at..].contains("wrote:") && gmail[at..].contains("Lunch?"),
            "{gmail}"
        );
        assert!(gmail.ends_with("</blockquote></div></details>"), "{gmail}");

        // Apple Mail and Thunderbird: blockquote type="cite"; Thunderbird's line before it too.
        let apple = folded(
            "<div>Yes</div><div><br><blockquote type=\"cite\"><div>On 1 Oct, Ada wrote:</div><div>Lunch?</div></blockquote></div>",
        );
        assert!(apple.contains("<details class=\"regards-quote\"><summary"), "{apple}");
        assert!(apple.contains("<blockquote type=\"cite\">"), "{apple}");
        let thunderbird = folded(
            "<p>Yes</p><div class=\"moz-cite-prefix\">On 1 Oct, Ada wrote:<br></div>\n<blockquote type=\"cite\">Lunch?</blockquote>",
        );
        let at = thunderbird.find(FOLD).expect(&thunderbird);
        assert!(
            thunderbird[at..].contains("wrote:") && thunderbird.ends_with("</blockquote></details>"),
            "{thunderbird}"
        );

        // Outlook: everything from its marker to the end.
        let outlook = folded(
            "<div>Thanks</div><div id=\"appendonsend\"></div><hr><div id=\"divRplyFwdMsg\"><b>From:</b> Ada</div><div>Lunch?</div>",
        );
        let at = outlook.find(FOLD).expect(&outlook);
        assert!(
            outlook[at..].contains("From:") && outlook.ends_with("<div>Lunch?</div></details>"),
            "{outlook}"
        );
    }

    #[test]
    fn quotes_stay_open_unless_they_are_history_under_a_reply() {
        // Answered between the quotes: all of it is read together.
        let between = folded("<p>See below</p><blockquote type=\"cite\">Lunch?</blockquote><p>Yes, at noon.</p>");
        assert!(!between.contains(FOLD), "{between}");
        // Nothing written above (a style sheet and spaces are not writing).
        let only =
            folded("<style>p{color:red}</style>&nbsp;<div class=\"gmail_quote\">On Mon, Ada wrote: Lunch?</div>");
        assert!(!only.contains(FOLD), "{only}");
        // A forward is what was sent.
        let forward = folded(
            "<div>FYI</div><div class=\"gmail_quote\"><div class=\"gmail_attr\">---------- Forwarded message ---------<br>From: Ada</div>The plan</div>",
        );
        assert!(!forward.contains(FOLD), "{forward}");
        let outlook_forward = folded(
            "<p>FYI</p><div id=\"divRplyFwdMsg\"><b>From:</b> Ada<br><b>Subject:</b>&nbsp;FW: Plans</div><div>The plan</div>",
        );
        assert!(!outlook_forward.contains(FOLD), "{outlook_forward}");
        // Plain quoting with no client's mark is left alone.
        assert!(!folded("<p>Yes</p><blockquote>Lunch?</blockquote>").contains(FOLD));
    }

    #[test]
    fn only_quote_marks_survive_of_ids_and_types() {
        let (html, _) = clean(
            "<div id=\"divRplyFwdMsg\">a</div><div id=\"main\">b</div><blockquote type=\"cite\">c</blockquote>\
             <blockquote type=\"other\">d</blockquote><ol type=\"a\"><li>e</li></ol>",
            false,
        );
        assert!(html.contains("<div id=\"divRplyFwdMsg\">"), "{html}");
        assert!(!html.contains("id=\"main\""), "{html}");
        assert!(
            html.contains("<blockquote type=\"cite\">c") && html.contains("<blockquote>d"),
            "{html}"
        );
    }

    #[test]
    fn a_quoted_message_keeps_its_formatting_but_not_its_pictures_or_scripts() {
        let raw = b"Content-Type: text/html

<p>Hi <b>Bob</b>, <a href=\"https://example.test\">see</a></p>\n            <img src=\"cid:logo\"><img src=\"https://t.test/p.gif\"><script>steal()</script><ul><li>one</li></ul>
";
        let quoted = quotable(raw).unwrap();
        assert!(
            quoted.contains("<b>Bob</b>") && quoted.contains("<li>one</li>"),
            "{quoted}"
        );
        assert!(quoted.contains("href=\"https://example.test\""), "{quoted}");
        assert!(!quoted.contains("<img") && !quoted.contains("script"), "{quoted}");
        assert!(
            quotable(
                b"Content-Type: text/plain

Hi
"
            )
            .is_none()
        );
        // Quoted history keeps its mark when sent, for readers to fold.
        assert!(
            sanitize_outgoing("<blockquote type=\"cite\">a</blockquote><blockquote type=\"x\">b</blockquote>")
                .contains("<blockquote type=\"cite\">a</blockquote><blockquote>b")
        );
    }

    #[test]
    fn percent_encoding_keeps_only_unreserved_characters() {
        assert_eq!(percent_encode("aZ09-._~"), "aZ09-._~");
        assert_eq!(percent_encode("a b&c=d/é"), "a%20b%26c%3Dd%2F%C3%A9");
    }
}
