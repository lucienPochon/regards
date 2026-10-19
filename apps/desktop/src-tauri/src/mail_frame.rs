//! The `regards-mail` URL scheme: serves a message's sanitised HTML to the
//! sandboxed iframe of the reading pane (ADR-0006 §4).
//!
//! Layers, from the daemon outwards: `ammonia` sanitising and image blocking
//! in the daemon; this response's own strict CSP (no scripts at all, images
//! only from `data:` unless the reader allowed remote ones); and the iframe's
//! empty `sandbox` attribute in the WebView.
//!
//! URLs look like `regards-mail://localhost/message/123?remote=0` (on Windows
//! the WebView uses `http://regards-mail.localhost/…`).
//!
//! The frame cannot leave the app, so the daemon points web links back here:
//! `/open?t=<token>&url=<URL>`, opened in a hidden frame inside the message
//! (`regards_core::html::LINK_TARGET`), so the message stays in view. We open
//! the URL in the browser and answer 204. The token is new each launch and
//! appears only in links rewritten for us, so an image or other resource in a
//! message cannot open URLs: only a click on one of its links does.

use crate::Bridge;
use regards_ipc::protocol::{MessageHtml, MessageHtmlParams, methods};
use std::sync::LazyLock;
use tauri::http::{Request, Response, StatusCode};
use tauri::{Manager, UriSchemeContext, UriSchemeResponder, Url, Wry};

pub(crate) const SCHEME: &str = "regards-mail";

/// Only the main window's reading pane may load messages.
const ALLOWED_WEBVIEW: &str = "main";

/// Without randomness, links are left as they are (and do nothing).
static LINK_TOKEN: LazyLock<Option<String>> = LazyLock::new(|| {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).ok()?;
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
});

#[derive(Debug, PartialEq, Eq)]
enum Route {
    Message { id: i64, allow_remote: bool },
    Open(Url),
}

pub(crate) fn handle(ctx: UriSchemeContext<'_, Wry>, request: Request<Vec<u8>>, responder: UriSchemeResponder) {
    if ctx.webview_label() != ALLOWED_WEBVIEW {
        return responder.respond(text(StatusCode::FORBIDDEN, "not available here"));
    }
    let token = LINK_TOKEN.as_deref();
    match route(&request, token) {
        Some(Route::Message { id, allow_remote }) => {
            let opener = token.and(origin(&request)).map(|origin| format!("{origin}/open"));
            let link_prefix = opener
                .as_ref()
                .zip(token)
                .map(|(opener, token)| format!("{opener}?t={token}&url="));
            let app = ctx.app_handle().clone();
            tauri::async_runtime::spawn(async move {
                let bridge = app.state::<Bridge>();
                let params = MessageHtmlParams {
                    id,
                    allow_remote,
                    link_prefix,
                };
                let result = match bridge.connect().await {
                    Ok(connected) => {
                        let mut client = connected.client;
                        client
                            .call::<_, MessageHtml>(methods::MESSAGE_HTML, params)
                            .await
                            .map_err(crate::describe)
                    }
                    Err(error) => Err(error),
                };
                responder.respond(match result {
                    Ok(rendered) => page(rendered.html, &csp(allow_remote, opener.as_deref())),
                    Err(error) => text(StatusCode::BAD_GATEWAY, &error),
                });
            });
        }
        Some(Route::Open(url)) => {
            // The URL came from a message: it is not logged.
            if let Err(error) = open::that_detached(url.as_str()) {
                eprintln!("could not open a link in the browser: {}", error.kind());
            }
            responder.respond(no_content());
        }
        None => responder.respond(text(StatusCode::NOT_FOUND, "not found")),
    }
}

fn route(request: &Request<Vec<u8>>, token: Option<&str>) -> Option<Route> {
    let raw = request.uri().path_and_query()?.as_str();
    if let Some(query) = raw.strip_prefix("/open?") {
        return open_target(query, token?).map(Route::Open);
    }
    let (id, allow_remote) = parse_message(raw)?;
    Some(Route::Message { id, allow_remote })
}

/// `/message/<id>` with an optional `remote=1`; the path may arrive percent-encoded.
fn parse_message(raw: &str) -> Option<(i64, bool)> {
    let decoded = percent_decode(raw)?;
    let (path, query) = decoded.split_once('?').unwrap_or((&decoded, ""));
    let id = path.trim_start_matches('/').strip_prefix("message/")?.parse().ok()?;
    let allow_remote = query.split('&').any(|pair| pair == "remote=1");
    Some((id, allow_remote))
}

/// The web page an `/open` link names, if the link is one of ours.
fn open_target(query: &str, token: &str) -> Option<Url> {
    let mut given_token = None;
    let mut target = None;
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("t", value)) => given_token = Some(percent_decode(value)?),
            Some(("url", value)) => target = Some(percent_decode(value)?),
            _ => {}
        }
    }
    if !same(given_token?.as_bytes(), token.as_bytes()) {
        return None;
    }
    let url = Url::parse(&target?).ok()?;
    matches!(url.scheme(), "http" | "https").then_some(url)
}

/// This host as the page served for `request` sees it; it differs by platform.
fn origin(request: &Request<Vec<u8>>) -> Option<String> {
    let uri = request.uri();
    Some(format!("{}://{}", uri.scheme_str()?, uri.authority()?.as_str()))
}

/// Compare in constant time.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |diff, (x, y)| diff | (x ^ y)) == 0
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// No scripts, no plugins, no forms, no base changes; inline styles allowed;
/// images and fonts only embedded, unless the reader allowed remote ones. The
/// only frame is the hidden link frame, and it may load only `opener`.
fn csp(allow_remote: bool, opener: Option<&str>) -> String {
    let remote = if allow_remote { " https: http:" } else { "" };
    let frames = opener.map_or("'none'", |opener| opener);
    format!(
        "default-src 'none'; style-src 'unsafe-inline'; img-src data:{remote}; font-src data:{remote}; \
         frame-src {frames}; form-action 'none'; base-uri 'none'"
    )
}

fn page(html: String, csp: &str) -> Response<Vec<u8>> {
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/html; charset=utf-8")
        .header("Content-Security-Policy", csp)
        .header("X-Content-Type-Options", "nosniff")
        .header("Referrer-Policy", "no-referrer")
        .header("Cache-Control", "no-store")
        .body(html.into_bytes())
        .expect("static headers are valid")
}

/// Shown, if at all, in the hidden link frame.
fn no_content() -> Response<Vec<u8>> {
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header("Content-Type", "text/plain; charset=utf-8")
        .header("Content-Security-Policy", "default-src 'none'")
        .header("Cache-Control", "no-store")
        .body(Vec::new())
        .expect("static headers are valid")
}

fn text(status: StatusCode, message: &str) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "text/plain; charset=utf-8")
        .header("Content-Security-Policy", "default-src 'none'")
        .body(message.as_bytes().to_vec())
        .expect("static headers are valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(uri: &str) -> Request<Vec<u8>> {
        Request::builder().uri(uri).body(Vec::new()).unwrap()
    }

    fn message(uri: &str) -> Option<(i64, bool)> {
        match route(&request(uri), Some("k")) {
            Some(Route::Message { id, allow_remote }) => Some((id, allow_remote)),
            _ => None,
        }
    }

    fn opened(uri: &str) -> Option<String> {
        match route(&request(uri), Some("k")) {
            Some(Route::Open(url)) => Some(url.to_string()),
            _ => None,
        }
    }

    #[test]
    fn urls_from_every_platform_are_understood() {
        assert_eq!(message("regards-mail://localhost/message/42"), Some((42, false)));
        assert_eq!(
            message("http://regards-mail.localhost/message/42?remote=1"),
            Some((42, true))
        );
        assert_eq!(
            message("http://regards-mail.localhost/message%2F7%3Fremote%3D1"),
            Some((7, true))
        );
        assert_eq!(message("regards-mail://localhost/other/42"), None);
        assert_eq!(message("regards-mail://localhost/message/abc"), None);
        assert_eq!(
            message("regards-mail://localhost/message/42?remote=10"),
            Some((42, false))
        );
    }

    #[test]
    fn links_point_back_at_the_host_they_were_served_from() {
        assert_eq!(
            origin(&request("regards-mail://localhost/message/42")).as_deref(),
            Some("regards-mail://localhost")
        );
        assert_eq!(
            origin(&request("http://regards-mail.localhost/message/42")).as_deref(),
            Some("http://regards-mail.localhost")
        );
    }

    #[test]
    fn only_our_own_links_to_web_pages_are_opened() {
        let link = "regards-mail://localhost/open?t=k&url=https%3A%2F%2Fexample.test%2Fa%3Fb%3D1%26c%3Dd";
        assert_eq!(opened(link).as_deref(), Some("https://example.test/a?b=1&c=d"));
        assert_eq!(
            opened("http://regards-mail.localhost/open?t=k&url=http%3A%2F%2Fx.test%2F").as_deref(),
            Some("http://x.test/")
        );
        // Without the token, or with another one: a message made this up.
        assert_eq!(opened("regards-mail://localhost/open?url=https%3A%2F%2Fx.test"), None);
        assert_eq!(
            opened("regards-mail://localhost/open?t=j&url=https%3A%2F%2Fx.test"),
            None
        );
        assert_eq!(
            opened("regards-mail://localhost/open?t=kk&url=https%3A%2F%2Fx.test"),
            None
        );
        // Only web pages.
        for target in [
            "file%3A%2F%2F%2Fetc%2Fpasswd",
            "javascript%3Aalert(1)",
            "mailto%3Aa%40b.test",
            "x",
        ] {
            assert_eq!(opened(&format!("regards-mail://localhost/open?t=k&url={target}")), None);
        }
        // Without randomness there are no links to open.
        assert_eq!(
            route(
                &request("regards-mail://localhost/open?t=k&url=https%3A%2F%2Fx.test"),
                None
            ),
            None
        );
    }

    #[test]
    fn the_policy_never_allows_scripts_and_blocks_remote_images_by_default() {
        let strict = csp(false, None);
        assert!(strict.starts_with("default-src 'none'"));
        assert!(!strict.contains("script"));
        assert!(strict.contains("img-src data:;"));
        assert!(strict.contains("frame-src 'none';"));
        assert!(csp(true, None).contains("img-src data: https: http:;"));
        // The link frame may load our opener and nothing else.
        let framed = csp(false, Some("http://regards-mail.localhost/open"));
        assert!(
            framed.contains("frame-src http://regards-mail.localhost/open;"),
            "{framed}"
        );
    }
}
