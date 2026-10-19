//! `regards://` links (ADR-0012 §5): agents give them (`regards://message/42`)
//! so the person can see in the app what an agent found or did.
//!
//! A link only opens a view, never acts: any web page or program can open
//! one too. Only `message`, `draft` and `hold` with a number are taken;
//! anything else is not a link to us.

use serde::Serialize;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Kind {
    Message,
    Draft,
    Hold,
}

/// What to open: the UI receives it as the `open` event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) struct Open {
    pub kind: Kind,
    pub id: i64,
}

/// A link that came before the window could take it (the one Regards was
/// started with), kept until the UI asks.
#[derive(Default)]
pub(crate) struct Pending(Mutex<Option<Open>>);

pub(crate) fn parse(url: &str) -> Option<Open> {
    let rest = url.strip_prefix("regards://")?.trim_end_matches('/');
    let (kind, id) = rest.split_once('/')?;
    let kind = match kind {
        "message" => Kind::Message,
        "draft" => Kind::Draft,
        "hold" => Kind::Hold,
        _ => return None,
    };
    if id.is_empty() || id.len() > 15 || !id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(Open {
        kind,
        id: id.parse().ok()?,
    })
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
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(open);
        }
        let _ = app.emit("open", open);
    }
}

/// The link not taken yet, if any (once).
#[tauri::command]
pub(crate) fn link_take(pending: tauri::State<'_, Pending>) -> Option<Open> {
    pending.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_links_to_a_message_draft_or_hold_open_anything() {
        assert_eq!(
            parse("regards://message/42"),
            Some(Open {
                kind: Kind::Message,
                id: 42
            })
        );
        assert_eq!(
            parse("regards://hold/7/"),
            Some(Open {
                kind: Kind::Hold,
                id: 7
            })
        );
        assert_eq!(parse("regards://draft/3").map(|o| o.kind), Some(Kind::Draft));
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
}
