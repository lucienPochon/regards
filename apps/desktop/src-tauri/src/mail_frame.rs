//! The `regards-mail` URL scheme: serves a message's sanitised HTML to the
//! sandboxed iframe of the reading pane (ADR-0006 §4).
//!
//! Layers, from the daemon outwards: `ammonia` sanitising and image blocking
//! in the daemon; this response's own strict CSP (no scripts but the one we
//! add, which says how tall the message is; images only from `data:` unless
//! the reader allowed remote ones); and the iframe's `sandbox` in the
//! WebView, which allows scripts but no same-origin access, forms, popups or
//! navigation.
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
    Message { id: i64, allow_remote: bool, dark: bool },
    Open(Url),
}

pub(crate) fn handle(ctx: UriSchemeContext<'_, Wry>, request: Request<Vec<u8>>, responder: UriSchemeResponder) {
    if ctx.webview_label() != ALLOWED_WEBVIEW {
        return responder.respond(text(StatusCode::FORBIDDEN, "not available here"));
    }
    let token = LINK_TOKEN.as_deref();
    match route(&request, token) {
        Some(Route::Message { id, allow_remote, dark }) => {
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
                let nonce = nonce();
                responder.respond(match result {
                    Ok(rendered) => page(
                        darkened(measured(rendered.html, nonce.as_deref()), dark),
                        &csp(allow_remote, opener.as_deref(), nonce.as_deref()),
                    ),
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
    let (id, allow_remote, dark) = parse_message(raw)?;
    Some(Route::Message { id, allow_remote, dark })
}

/// `/message/<id>` with an optional `remote=1` and `dark=1`; the path may
/// arrive percent-encoded.
fn parse_message(raw: &str) -> Option<(i64, bool, bool)> {
    let decoded = percent_decode(raw)?;
    let (path, query) = decoded.split_once('?').unwrap_or((&decoded, ""));
    let id = path.trim_start_matches('/').strip_prefix("message/")?.parse().ok()?;
    let allow_remote = query.split('&').any(|pair| pair == "remote=1");
    let dark = query.split('&').any(|pair| pair == "dark=1");
    Some((id, allow_remote, dark))
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

/// The one script a message's page runs, ours, in two parts. This one tells
/// the reading pane how tall the message is, as it loads and whenever that
/// changes, so the frame is as tall as the message (a horizontal scroll bar
/// included). Neither can do more than tell the reading pane: the frame's
/// origin is its own, not the app's, and it may fetch nothing
/// (`default-src 'none'`).
const MEASURE: &str = "(()=>{let last=0;const tell=()=>{const root=document.documentElement;\
const body=document.body;const bar=innerHeight-root.clientHeight;\
const height=Math.ceil(Math.max(root.getBoundingClientRect().height,body?body.scrollHeight:0)+bar);\
if(height!==last){last=height;parent.postMessage({regardsMailHeight:height},'*');}};\
new ResizeObserver(tell).observe(document.documentElement);\
if(document.body){new ResizeObserver(tell).observe(document.body);}\
addEventListener('load',tell);tell();})();";

/// The second part: what happens in the message that the app should know.
/// - **Keys**, once the person has clicked in the message, so shortcuts
///   still work there. Not those that stay with the message: copy, select
///   all, and the keys that scroll it or move between its links.
/// - **The link under the pointer**: where it really goes (the URL our
///   `/open` link wraps, never its token), or `null` off a link.
const RELAY: &str = "(()=>{const send=m=>parent.postMessage(m,'*');\
addEventListener('keydown',e=>{\
if(e.isComposing||/^(Arrow|Page)/.test(e.key)||/^(Home|End|Tab|Enter| )$/.test(e.key))return;\
if((e.ctrlKey||e.metaKey)&&!e.altKey&&/^[ac]$/i.test(e.key))return;\
send({regardsMailKey:{key:e.key,code:e.code,ctrlKey:e.ctrlKey,metaKey:e.metaKey,shiftKey:e.shiftKey,\
altKey:e.altKey,altGraph:e.getModifierState('AltGraph')}});});\
const goes=a=>{try{const u=new URL(a.href);\
if(u.pathname==='/open'&&u.searchParams.has('url'))return u.searchParams.get('url');\
return u.protocol==='mailto:'?a.href:null;}catch(_){return null;}};\
let shown=null;const show=u=>{if(u!==shown){shown=u;send({regardsMailLink:u});}};\
addEventListener('mouseover',e=>{const a=e.target instanceof Element?e.target.closest('a[href]'):null;\
show(a?goes(a):null);});\
document.documentElement.addEventListener('mouseleave',()=>show(null));})();";

/// The third part: find in the message, as the reading pane asks (only it:
/// `{regardsMailFind: {query, step}}`). Every match is marked, ignoring
/// case, the current one more strongly (the Custom Highlight API; selected
/// where it is missing), and the pane is told how many there are, which one
/// is current and where it is (`regardsMailFound`). An empty query clears.
const FIND: &str = "(()=>{let hits=[],at=-1,last='',css=null;const hl=window.CSS&&CSS.highlights&&window.Highlight;\
const look=q=>{hits=[];const w=document.body?document.createTreeWalker(document.body,NodeFilter.SHOW_TEXT):null;\
for(let n=w&&w.nextNode();n&&hits.length<1000;n=w.nextNode()){\
if(/^(SCRIPT|STYLE|NOSCRIPT)$/.test(n.parentNode.nodeName))continue;const t=n.data.toLowerCase();\
for(let i=t.indexOf(q);i!==-1&&hits.length<1000;i=t.indexOf(q,i+q.length)){const r=document.createRange();\
r.setStart(n,i);r.setEnd(n,i+q.length);hits.push(r);}}};\
const mark=()=>{if(hl){if(!css){css=document.createElement('style');\
css.textContent='::highlight(regards-find){background:#ffe066;color:inherit}::highlight(regards-find-now){background:#ff9632;color:#000}';\
(document.head||document.documentElement).append(css);}\
CSS.highlights.set('regards-find',new Highlight(...hits));\
CSS.highlights.set('regards-find-now',hits[at]?new Highlight(hits[at]):new Highlight());}\
else{const s=getSelection();s.removeAllRanges();if(hits[at])s.addRange(hits[at]);}};\
addEventListener('message',e=>{if(e.source!==parent)return;const f=e.data&&e.data.regardsMailFind;\
if(!f||typeof f.query!=='string')return;const q=f.query.toLowerCase();\
if(q!==last){last=q;if(q)look(q);else hits=[];at=hits.length?0:-1;}\
else if(hits.length)at=((at+(Number(f.step)||0))%hits.length+hits.length)%hits.length;\
mark();const r=hits[at];let top=0,bottom=0;\
if(r){const el=r.startContainer.parentElement;if(el)el.scrollIntoView({block:'nearest'});\
const b=r.getBoundingClientRect();top=b.top;bottom=b.bottom;}\
if(q)parent.postMessage({regardsMailFound:{count:hits.length,at,top,bottom}},'*');});})();";

/// A value used once, for this page's script alone; `None` without
/// randomness, and then no script runs (the frame keeps a fixed height).
fn nonce() -> Option<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).ok()?;
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// The message's HTML with [`MEASURE`], [`RELAY`] and [`FIND`] at the end of its body.
fn measured(html: String, nonce: Option<&str>) -> String {
    let Some(nonce) = nonce else {
        return html;
    };
    let script = format!("<script nonce=\"{nonce}\">{MEASURE}{RELAY}{FIND}</script>");
    match html.to_ascii_lowercase().rfind("</body>") {
        Some(at) => format!("{}{script}{}", &html[..at], &html[at..]),
        None => html + &script,
    }
}

/// The page for the app's dark theme: the message inverted, its hues turned
/// back so colours stay recognisable, and pictures inverted again so they
/// look as sent. Its white page becomes the dark reading card (`--bg-elev`).
const DARK: &str = "<style>html{color-scheme:dark;background:#1f2221}\
body{filter:invert(1) hue-rotate(180deg);background:#e0ddde}\
img,video,picture,canvas,[style*=\"background-image\"],[background]{filter:invert(1) hue-rotate(180deg)}</style>";

/// The message's page with [`DARK`] last in its head, when `dark`.
fn darkened(html: String, dark: bool) -> String {
    if !dark {
        return html;
    }
    match html.to_ascii_lowercase().find("</head>") {
        Some(at) => format!("{}{DARK}{}", &html[..at], &html[at..]),
        None => format!("{DARK}{html}"),
    }
}

/// No scripts but ours (`nonce`), no plugins, no forms, no base changes;
/// inline styles allowed; images and fonts only embedded, unless the reader
/// allowed remote ones. The only frame is the hidden link frame, and it may
/// load only `opener`.
fn csp(allow_remote: bool, opener: Option<&str>, nonce: Option<&str>) -> String {
    let remote = if allow_remote { " https: http:" } else { "" };
    let frames = opener.map_or("'none'", |opener| opener);
    let scripts = nonce.map_or_else(|| "'none'".to_owned(), |nonce| format!("'nonce-{nonce}'"));
    format!(
        "default-src 'none'; script-src {scripts}; style-src 'unsafe-inline'; img-src data:{remote}; \
         font-src data:{remote}; frame-src {frames}; form-action 'none'; base-uri 'none'"
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
            Some(Route::Message { id, allow_remote, .. }) => Some((id, allow_remote)),
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
    fn the_policy_allows_only_our_script_and_blocks_remote_images_by_default() {
        let strict = csp(false, None, Some("abc123"));
        assert!(strict.starts_with("default-src 'none'"));
        let scripts = strict.split(';').find(|rule| rule.trim().starts_with("script-src"));
        assert_eq!(
            scripts.map(str::trim),
            Some("script-src 'nonce-abc123'"),
            "ours alone: {strict}"
        );
        assert!(csp(false, None, None).contains("script-src 'none';"));
        assert!(strict.contains("img-src data:;"));
        assert!(strict.contains("frame-src 'none';"));
        assert!(csp(true, None, None).contains("img-src data: https: http:;"));
        // The link frame may load our opener and nothing else.
        let framed = csp(false, Some("http://regards-mail.localhost/open"), None);
        assert!(
            framed.contains("frame-src http://regards-mail.localhost/open;"),
            "{framed}"
        );
    }

    #[test]
    fn the_dark_theme_gets_a_darkened_page_when_asked() {
        let dark = |uri: &str| matches!(route(&request(uri), Some("k")), Some(Route::Message { dark: true, .. }));
        assert!(dark("http://regards-mail.localhost/message/42?remote=0&dark=1"));
        assert!(dark("regards-mail://localhost/message%2F42%3Fdark%3D1"));
        assert!(!dark("regards-mail://localhost/message/42?remote=1"));
        let page = darkened(
            "<html><head><style>a{}</style></head><body>Hi</body></html>".into(),
            true,
        );
        assert!(
            page.contains("<style>a{}</style><style>html{color-scheme:dark"),
            "{page}"
        );
        assert!(page.ends_with("</style></head><body>Hi</body></html>"), "{page}");
        assert_eq!(darkened("<p>Hi</p>".into(), false), "<p>Hi</p>");
    }

    #[test]
    fn the_measuring_script_goes_at_the_end_of_the_body_with_its_nonce() {
        let page = measured("<html><body><p>Hi</p></BODY></html>".into(), Some("n1"));
        assert!(page.starts_with("<html><body><p>Hi</p><script nonce=\"n1\">"), "{page}");
        assert!(page.ends_with("</script></BODY></html>"), "{page}");
        assert!(
            [
                "regardsMailHeight",
                "regardsMailKey",
                "regardsMailLink",
                "regardsMailFind"
            ]
            .iter()
            .all(|part| page.contains(part)),
            "one script: the height, keys, where links go, and find"
        );
        assert!(!RELAY.contains("searchParams.get('t')"), "never the link token");
        assert!(measured("<p>Hi</p>".into(), Some("n1")).ends_with("</script>"));
        assert_eq!(measured("<p>Hi</p>".into(), None), "<p>Hi</p>", "no nonce, no script");
    }
}
