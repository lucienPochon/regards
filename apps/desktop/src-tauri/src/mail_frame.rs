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

use crate::Bridge;
use regards_ipc::protocol::{MessageHtml, MessageHtmlParams, methods};
use tauri::http::{Request, Response, StatusCode};
use tauri::{Manager, UriSchemeContext, UriSchemeResponder, Wry};

pub(crate) const SCHEME: &str = "regards-mail";

/// Only the main window's reading pane may load messages.
const ALLOWED_WEBVIEW: &str = "main";

pub(crate) fn handle(ctx: UriSchemeContext<'_, Wry>, request: Request<Vec<u8>>, responder: UriSchemeResponder) {
    if ctx.webview_label() != ALLOWED_WEBVIEW {
        return responder.respond(text(StatusCode::FORBIDDEN, "not available here"));
    }
    let Some((id, allow_remote)) = parse(&request) else {
        return responder.respond(text(StatusCode::NOT_FOUND, "no such message"));
    };
    let app = ctx.app_handle().clone();
    tauri::async_runtime::spawn(async move {
        let bridge = app.state::<Bridge>();
        let result = match bridge.connect().await {
            Ok(connected) => {
                let mut client = connected.client;
                client
                    .call::<_, MessageHtml>(methods::MESSAGE_HTML, MessageHtmlParams { id, allow_remote })
                    .await
                    .map_err(crate::describe)
            }
            Err(error) => Err(error),
        };
        responder.respond(match result {
            Ok(rendered) => page(rendered.html, allow_remote),
            Err(error) => text(StatusCode::BAD_GATEWAY, &error),
        });
    });
}

/// `/message/<id>` with an optional `remote=1`; the path may arrive percent-encoded.
fn parse(request: &Request<Vec<u8>>) -> Option<(i64, bool)> {
    let raw = request.uri().path_and_query()?.as_str();
    let decoded = percent_decode(raw)?;
    let (path, query) = decoded.split_once('?').unwrap_or((&decoded, ""));
    let id = path.trim_start_matches('/').strip_prefix("message/")?.parse().ok()?;
    let allow_remote = query.split('&').any(|pair| pair == "remote=1");
    Some((id, allow_remote))
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
/// images and fonts only embedded, unless the reader allowed remote ones.
pub(crate) fn csp(allow_remote: bool) -> String {
    let remote = if allow_remote { " https: http:" } else { "" };
    format!(
        "default-src 'none'; style-src 'unsafe-inline'; img-src data:{remote}; font-src data:{remote}; \
         form-action 'none'; base-uri 'none'"
    )
}

fn page(html: String, allow_remote: bool) -> Response<Vec<u8>> {
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/html; charset=utf-8")
        .header("Content-Security-Policy", csp(allow_remote))
        .header("X-Content-Type-Options", "nosniff")
        .header("Referrer-Policy", "no-referrer")
        .header("Cache-Control", "no-store")
        .body(html.into_bytes())
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

    #[test]
    fn urls_from_every_platform_are_understood() {
        assert_eq!(
            parse(&request("regards-mail://localhost/message/42")),
            Some((42, false))
        );
        assert_eq!(
            parse(&request("http://regards-mail.localhost/message/42?remote=1")),
            Some((42, true))
        );
        assert_eq!(
            parse(&request("http://regards-mail.localhost/message%2F7%3Fremote%3D1")),
            Some((7, true))
        );
        assert_eq!(parse(&request("regards-mail://localhost/other/42")), None);
        assert_eq!(parse(&request("regards-mail://localhost/message/abc")), None);
        assert_eq!(
            parse(&request("regards-mail://localhost/message/42?remote=10")),
            Some((42, false))
        );
    }

    #[test]
    fn the_policy_never_allows_scripts_and_blocks_remote_images_by_default() {
        let strict = csp(false);
        assert!(strict.starts_with("default-src 'none'"));
        assert!(!strict.contains("script"));
        assert!(strict.contains("img-src data:;"));
        assert!(csp(true).contains("img-src data: https: http:;"));
    }
}
