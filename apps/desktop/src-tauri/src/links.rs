//! Links opened from outside the app.
//!
//! - **`regards://`** (ADR-0012 §5): agents give them (`regards://message/42`)
//!   so the person can see in the app what an agent found or did. Only
//!   `message`, `draft` and `hold` with a number are taken.
//! - **`mailto:`** (RFC 6068), once Regards is the person's mail app: a new
//!   message to write, with its recipients, subject and text.
//!
//! A link only opens a view or a draft, never acts (nothing is sent): any
//! web page or program can open one too. A `mailto:` link's other fields
//! (attachments among them) are ignored, and its sizes are capped.

use serde::Serialize;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

/// The event the UI receives a link as.
pub(crate) const EVENT: &str = "link_open";

/// What to open: the UI receives it as [`EVENT`], `{ "kind": …, … }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Open {
    Message {
        id: i64,
    },
    Draft {
        id: i64,
    },
    Hold {
        id: i64,
    },
    /// A `mailto:` link: a draft to start, never sent unasked.
    Compose(Compose),
}

/// A message a `mailto:` link asks to write.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub(crate) struct Compose {
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body: String,
}

/// The most recipients a link may name, all fields together.
const MAX_RECIPIENTS: usize = 100;
const MAX_SUBJECT: usize = 998;
const MAX_BODY: usize = 64 * 1024;

/// A link that came before the window could take it (the one Regards was
/// started with), kept until the UI asks.
#[derive(Default)]
pub(crate) struct Pending(Mutex<Option<Open>>);

pub(crate) fn parse(url: &str) -> Option<Open> {
    if url.len() > 6 && url[..7].eq_ignore_ascii_case("mailto:") {
        return mailto(&url[7..]).map(Open::Compose);
    }
    let rest = url.strip_prefix("regards://")?.trim_end_matches('/');
    let (kind, id) = rest.split_once('/')?;
    if id.is_empty() || id.len() > 15 || !id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let id = id.parse().ok()?;
    Some(match kind {
        "message" => Open::Message { id },
        "draft" => Open::Draft { id },
        "hold" => Open::Hold { id },
        _ => return None,
    })
}

/// What follows `mailto:`: addresses, then `?to=…&cc=…&bcc=…&subject=…&body=…`.
fn mailto(rest: &str) -> Option<Compose> {
    let (addresses, fields) = rest.split_once('?').unwrap_or((rest, ""));
    let mut compose = Compose {
        to: addresses_in(&decode(addresses)),
        ..Compose::default()
    };
    for field in fields.split('&').filter(|field| !field.is_empty()) {
        let (name, value) = field.split_once('=').unwrap_or((field, ""));
        let value = decode(value);
        match name.to_ascii_lowercase().as_str() {
            "to" => compose.to.extend(addresses_in(&value)),
            "cc" => compose.cc.extend(addresses_in(&value)),
            "bcc" => compose.bcc.extend(addresses_in(&value)),
            "subject" => compose.subject = clip(value.replace(['\r', '\n'], " "), MAX_SUBJECT),
            "body" => compose.body = clip(value.replace("\r\n", "\n"), MAX_BODY),
            // Attachments, other headers: never.
            _ => {}
        }
    }
    let mut room = MAX_RECIPIENTS;
    for list in [&mut compose.to, &mut compose.cc, &mut compose.bcc] {
        list.truncate(room);
        room -= list.len();
    }
    Some(compose)
}

/// The plain addresses in a comma-separated list; anything else is dropped.
fn addresses_in(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|address| {
            address.contains('@')
                && address.len() <= 320
                && !address
                    .chars()
                    .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '<' | '>' | '"'))
        })
        .map(str::to_owned)
        .collect()
}

/// Percent-decoded (UTF-8); `+` stays `+`, as in RFC 6068.
fn decode(text: &str) -> String {
    let hex = |byte: u8| (byte as char).to_digit(16).map(|digit| digit as u8);
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match (
            bytes[i],
            bytes.get(i + 1).copied().and_then(hex),
            bytes.get(i + 2).copied().and_then(hex),
        ) {
            (b'%', Some(high), Some(low)) => {
                out.push(high << 4 | low);
                i += 3;
            }
            (byte, ..) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn clip(mut text: String, most: usize) -> String {
    if text.len() > most {
        let mut end = most;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    text
}

/// The `mailto:` links among a program's arguments. Regards registers that
/// scheme only when the person asks ([`make_mail_app`]), so the deep-link
/// plugin, which takes only the configured schemes, leaves these to us.
pub(crate) fn mailto_args(args: &[String]) -> Vec<String> {
    args.iter()
        .filter(|arg| arg.len() > 6 && arg[..7].eq_ignore_ascii_case("mailto:"))
        .cloned()
        .collect()
}

/// Show the window and open what the links name. Those Regards was started
/// with are kept too, for the UI to take once it is there.
pub(crate) fn open(app: &AppHandle, urls: impl IntoIterator<Item = String>, at_start: bool) {
    for url in urls {
        let Some(open) = parse(&url) else {
            continue;
        };
        crate::background::show(app);
        if at_start {
            *app.state::<Pending>()
                .0
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(open.clone());
        }
        let _ = app.emit(EVENT, open);
    }
}

/// The link not taken yet, if any (once).
#[tauri::command]
pub(crate) fn link_take(pending: tauri::State<'_, Pending>) -> Option<Open> {
    pending.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take()
}

/// Whether Regards opens `mailto:` links here (as far as it can tell).
#[derive(Debug, Clone, Serialize)]
pub(crate) struct MailAppStatus {
    registered: bool,
    /// It can register itself; on macOS the person chooses in Mail instead.
    can_register: bool,
}

#[tauri::command]
pub(crate) fn mail_app_status(app: AppHandle) -> MailAppStatus {
    use tauri_plugin_deep_link::DeepLinkExt;
    MailAppStatus {
        registered: app.deep_link().is_registered("mailto").unwrap_or(false),
        can_register: !cfg!(target_os = "macos"),
    }
}

/// The person asked: open `mailto:` links in Regards. On Windows the
/// system's Default apps then opens, to confirm it there.
#[tauri::command]
pub(crate) fn make_mail_app(app: AppHandle) -> Result<(), String> {
    if cfg!(target_os = "macos") {
        return Err("on macOS, choose Regards as the default mail app in Mail, under Settings › General".into());
    }
    use tauri_plugin_deep_link::DeepLinkExt;
    app.deep_link()
        .register("mailto")
        .map_err(|error| format!("could not make Regards your mail app: {error}"))?;
    if cfg!(windows) {
        let _ = open::that_detached("ms-settings:defaultapps");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_links_to_a_message_draft_or_hold_open_anything() {
        assert_eq!(parse("regards://message/42"), Some(Open::Message { id: 42 }));
        assert_eq!(parse("regards://hold/7/"), Some(Open::Hold { id: 7 }));
        assert_eq!(parse("regards://draft/3"), Some(Open::Draft { id: 3 }));
        assert_eq!(
            serde_json::to_value(Open::Message { id: 42 }).unwrap(),
            serde_json::json!({ "kind": "message", "id": 42 }),
            "as the UI has them"
        );
        for other in [
            "regards://send/3",
            "regards://message/",
            "regards://message/-1",
            "regards://message/4?then=send",
            "regards://message/4/delete",
            "regards://message/99999999999999999999",
            "https://message/4",
            "regards:message/4",
        ] {
            assert_eq!(parse(other), None, "{other}");
        }
    }

    #[test]
    fn a_mailto_link_starts_a_message_and_nothing_more() {
        let Some(Open::Compose(compose)) = parse(
            "MAILTO:ada@example.test,bob%40example.test?cc=cy@example.test&subject=Plans%20for%20Friday\
             &body=Hi%2C%0D%0Asee%20you+soon&attach=C:%5Csecret.pdf&X-Evil=1",
        ) else {
            panic!("not a draft");
        };
        assert_eq!(compose.to, ["ada@example.test", "bob@example.test"]);
        assert_eq!(compose.cc, ["cy@example.test"]);
        assert_eq!(compose.subject, "Plans for Friday");
        assert_eq!(compose.body, "Hi,\nsee you+soon", "+ is a plus in mailto");
        let json = serde_json::to_value(Open::Compose(compose)).unwrap();
        assert_eq!(json["kind"], "compose");
        assert!(json.get("attach").is_none());

        // Nothing addressed is fine (a blank message); junk addresses are dropped.
        let Some(Open::Compose(blank)) = parse("mailto:?subject=Hi%0Athere") else {
            panic!()
        };
        assert!(blank.to.is_empty());
        assert_eq!(blank.subject, "Hi there", "one line");
        let Some(Open::Compose(junk)) = parse("mailto:not-an-address,<x@y.test>,a@b.test") else {
            panic!()
        };
        assert_eq!(junk.to, ["a@b.test"]);

        let many: String = (0..150).map(|n| format!("p{n}@example.test,")).collect();
        let Some(Open::Compose(capped)) = parse(&format!("mailto:{many}?body={}", "a".repeat(100_000))) else {
            panic!()
        };
        assert_eq!(capped.to.len(), MAX_RECIPIENTS);
        assert_eq!(capped.body.len(), MAX_BODY);
        assert_eq!(
            mailto_args(&["regards.exe".into(), "mailto:a@b.test".into(), "--background".into()]),
            ["mailto:a@b.test"]
        );
    }
}
