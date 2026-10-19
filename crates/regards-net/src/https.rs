//! A small HTTPS client, for OAuth token requests (ADR-0018), the update
//! check and the assistant: POST a form or JSON, or GET, and read the
//! answer. HTTP/1.1, one request per connection, over the same TLS as mail
//! (the platform's verifier, never an invalid certificate). Plain HTTP only
//! to this computer (a model server run locally). Not a general web client.

use crate::{NetError, TlsOptions, connect_tls};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;

/// How long the whole exchange may take.
const TIMEOUT: Duration = Duration::from_secs(30);
/// The most of an answer read; token answers are a few kilobytes.
const MAX_ANSWER: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpsResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

/// POST `form` (application/x-www-form-urlencoded) to an `https://` URL.
pub async fn post_form(url: &str, form: &[(&str, &str)], options: &TlsOptions) -> Result<HttpsResponse, NetError> {
    let body = form_encode(form);
    let head = format!(
        "Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n",
        body.len()
    );
    exchange("POST", url, &head, &body, options, TIMEOUT).await
}

/// GET an `https://` URL.
pub async fn get(url: &str, options: &TlsOptions) -> Result<HttpsResponse, NetError> {
    exchange("GET", url, "", "", options, TIMEOUT).await
}

/// POST a JSON `body` with these extra `headers`, waiting up to `wait` (an
/// AI model can take a while). `https://` anywhere; `http://` only to this
/// computer (`localhost`, `127.0.0.1`, `[::1]`).
pub async fn post_json(
    url: &str,
    headers: &[(&str, &str)],
    body: &str,
    wait: Duration,
    options: &TlsOptions,
) -> Result<HttpsResponse, NetError> {
    let mut head = String::new();
    for (name, value) in headers {
        // Nothing may end the header line: no header or request can be smuggled in.
        let clean = |text: &str| !text.is_empty() && !text.chars().any(|c| c.is_control());
        if !clean(name) || !clean(value) || name.contains(':') {
            return Err(NetError::Connect(format!("a header that cannot be sent: {name}")));
        }
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str(&format!(
        "Content-Type: application/json\r\nContent-Length: {}\r\n",
        body.len()
    ));
    exchange("POST", url, &head, body, options, wait).await
}

async fn exchange(
    method: &str,
    url: &str,
    head: &str,
    body: &str,
    options: &TlsOptions,
    wait: Duration,
) -> Result<HttpsResponse, NetError> {
    let (secure, host, port, path) = split_url(url)?;
    let default_port = if secure { 443 } else { 80 };
    let host_header = if port == default_port {
        host.clone()
    } else {
        format!("{host}:{port}")
    };
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host_header}\r\nUser-Agent: Regards\r\nAccept: application/json\r\n\
         {head}Connection: close\r\n\r\n{body}"
    );
    let exchange = async {
        if secure {
            talk(connect_tls(&host, port, options).await?, &request, &host).await
        } else {
            let address = if host == "localhost" {
                "127.0.0.1"
            } else {
                host.trim_matches(['[', ']'])
            };
            let stream = TcpStream::connect((address, port))
                .await
                .map_err(|error| NetError::Connect(format!("could not reach {host}: {error}")))?;
            talk(stream, &request, &host).await
        }
    };
    tokio::time::timeout(wait, exchange)
        .await
        .map_err(|_| NetError::Timeout("HTTPS request"))?
}

/// Send the request and read the whole answer.
async fn talk<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    request: &str,
    host: &str,
) -> Result<HttpsResponse, NetError> {
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|error| NetError::Connect(format!("could not send to {host}: {error}")))?;
    stream
        .flush()
        .await
        .map_err(|error| NetError::Connect(format!("could not send to {host}: {error}")))?;
    let mut answer = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let read = match stream.read(&mut chunk).await {
            Ok(read) => read,
            // Servers often end a Connection: close answer without TLS close_notify.
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => 0,
            Err(error) => return Err(NetError::Connect(format!("could not read from {host}: {error}"))),
        };
        if read == 0 {
            break;
        }
        answer.extend_from_slice(&chunk[..read]);
        if answer.len() > MAX_ANSWER {
            return Err(NetError::Connect(format!("{host} answered with too much")));
        }
    }
    parse_response(&answer).ok_or_else(|| NetError::Connect(format!("{host} gave an answer that is not HTTP")))
}

/// `https://host[:port]/path?query` (or `http://` to this computer) as
/// whether it is secure, its host, port and path.
fn split_url(url: &str) -> Result<(bool, String, u16, String), NetError> {
    let (secure, rest) = match (url.strip_prefix("https://"), url.strip_prefix("http://")) {
        (Some(rest), _) => (true, rest),
        (None, Some(rest)) => (false, rest),
        (None, None) => return Err(NetError::Connect(format!("not an https:// address: {url}"))),
    };
    let (authority, path) = match rest.find('/') {
        Some(at) => (&rest[..at], &rest[at..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && !port.contains(']') => (
            host,
            port.parse()
                .map_err(|_| NetError::Connect(format!("not a port: {port}")))?,
        ),
        _ => (authority, if secure { 443 } else { 80 }),
    };
    if host.is_empty() {
        return Err(NetError::Connect(format!("no host in {url}")));
    }
    // In the clear only on this computer: nothing on the way can read it.
    if !secure && !matches!(host, "localhost" | "127.0.0.1" | "[::1]") {
        return Err(NetError::Connect(format!(
            "not an https:// address: {url} (plain http:// is for this computer only)"
        )));
    }
    Ok((secure, host.to_owned(), port, path.to_owned()))
}

/// `a=1&b=two%20words`.
pub fn form_encode(form: &[(&str, &str)]) -> String {
    form.iter()
        .map(|(name, value)| format!("{}={}", percent(name), percent(value)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Everything but the unreserved characters (RFC 3986), percent-encoded.
fn percent(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// An HTTP/1.1 answer: its status and body (with chunked transfer decoded).
fn parse_response(answer: &[u8]) -> Option<HttpsResponse> {
    let split = answer.windows(4).position(|window| window == b"\r\n\r\n")?;
    let head = std::str::from_utf8(&answer[..split]).ok()?;
    let rest = &answer[split + 4..];
    let mut lines = head.split("\r\n");
    let status_line = lines.next()?;
    let status: u16 = status_line.split_whitespace().nth(1)?.parse().ok()?;
    if !status_line.starts_with("HTTP/1.") {
        return None;
    }
    let mut chunked = false;
    let mut length: Option<usize> = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let (name, value) = (name.trim().to_ascii_lowercase(), value.trim());
        if name == "transfer-encoding" && value.to_ascii_lowercase().contains("chunked") {
            chunked = true;
        } else if name == "content-length" {
            length = value.parse().ok();
        }
    }
    let body = if chunked {
        dechunk(rest)?
    } else {
        match length {
            Some(length) => rest.get(..length)?.to_vec(),
            None => rest.to_vec(),
        }
    };
    Some(HttpsResponse { status, body })
}

fn dechunk(mut rest: &[u8]) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let line_end = rest.windows(2).position(|window| window == b"\r\n")?;
        let size_text = std::str::from_utf8(&rest[..line_end]).ok()?;
        let size = usize::from_str_radix(size_text.split(';').next()?.trim(), 16).ok()?;
        rest = &rest[line_end + 2..];
        if size == 0 {
            return Some(body);
        }
        body.extend_from_slice(rest.get(..size)?);
        rest = rest.get(size + 2..)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_split_into_host_port_and_path() {
        assert_eq!(
            split_url("https://login.example.test/common/oauth2/v2.0/token").unwrap(),
            (
                true,
                "login.example.test".into(),
                443,
                "/common/oauth2/v2.0/token".into()
            )
        );
        assert_eq!(
            split_url("https://localhost:8443/token?x=1").unwrap(),
            (true, "localhost".into(), 8443, "/token?x=1".into())
        );
        assert_eq!(split_url("https://example.test").unwrap().3, "/");
        assert!(split_url("http://example.test/token").is_err(), "never in the clear");
        assert!(split_url("https:///token").is_err());
        // A model server on this computer may be plain HTTP.
        assert_eq!(
            split_url("http://localhost:11434/v1/chat/completions").unwrap(),
            (false, "localhost".into(), 11434, "/v1/chat/completions".into())
        );
        assert_eq!(split_url("http://127.0.0.1/v1").unwrap().2, 80);
        assert!(split_url("http://192.168.1.5:8080/v1").is_err(), "not this computer");
        assert!(split_url("http://localhost.evil.test/v1").is_err());
    }

    #[test]
    fn forms_are_encoded_and_answers_read() {
        assert_eq!(
            form_encode(&[("grant_type", "refresh_token"), ("scope", "a b&c=d/é")]),
            "grant_type=refresh_token&scope=a%20b%26c%3Dd%2F%C3%A9"
        );
        let plain = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}extra";
        assert_eq!(
            parse_response(plain),
            Some(HttpsResponse {
                status: 200,
                body: b"{}".to_vec()
            })
        );
        let chunked =
            b"HTTP/1.1 400 Bad Request\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n{\"a\"\r\n3;x=y\r\n:1}\r\n0\r\n\r\n";
        assert_eq!(
            parse_response(chunked),
            Some(HttpsResponse {
                status: 400,
                body: b"{\"a\":1}".to_vec()
            })
        );
        assert_eq!(parse_response(b"SSH-2.0-OpenSSH\r\n\r\n"), None);
        assert_eq!(
            parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nshort"),
            None
        );
    }
}
