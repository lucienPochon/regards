//! Where the browser comes back after an OAuth sign-in (RFC 8252 §7.3): a
//! listener on this computer's loopback address, on a port the system
//! picks, that takes one redirect and answers with a page saying to go back
//! to Regards. Only the redirect carrying the sign-in's `state` counts:
//! anything else on this computer can reach the port too.

use crate::NetError;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const PAGE: &str = "<!doctype html><html><head><meta charset=\"utf-8\"><title>Regards</title></head>\
    <body style=\"font-family: system-ui, sans-serif; margin: 15vh auto; max-width: 28em; text-align: center\">\
    <h1 style=\"font-weight: 600\">Back to Regards</h1><p>You can close this window and go back to Regards.</p>\
    </body></html>";

/// A loopback listener for one sign-in.
pub struct Redirect {
    listener: TcpListener,
    port: u16,
}

impl Redirect {
    /// Listen on 127.0.0.1, on a port of the system's choosing.
    pub async fn listen() -> Result<Self, NetError> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(|error| NetError::Connect(format!("cannot listen for the sign-in: {error}")))?;
        let port = listener
            .local_addr()
            .map_err(|error| NetError::Connect(error.to_string()))?
            .port();
        Ok(Self { listener, port })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// The query parameters of the first request for `/` with this
    /// `state`, within `wait`. Others (a browser asking for its icon, a
    /// stale or forged redirect) are turned away.
    pub async fn wait(self, state: &str, wait: Duration) -> Result<Vec<(String, String)>, NetError> {
        tokio::time::timeout(wait, async {
            loop {
                let (mut stream, _) = self
                    .listener
                    .accept()
                    .await
                    .map_err(|error| NetError::Connect(error.to_string()))?;
                let mut request = Vec::new();
                let mut chunk = [0u8; 2048];
                // The request line and headers; a GET has no body.
                while !request.windows(4).any(|window| window == b"\r\n\r\n") && request.len() < 16 * 1024 {
                    match tokio::time::timeout(Duration::from_secs(5), stream.read(&mut chunk)).await {
                        Ok(Ok(0)) | Ok(Err(_)) | Err(_) => break,
                        Ok(Ok(read)) => request.extend_from_slice(&chunk[..read]),
                    }
                }
                let line = String::from_utf8_lossy(&request);
                let target = line
                    .lines()
                    .next()
                    .and_then(|first| first.strip_prefix("GET "))
                    .and_then(|rest| rest.split_whitespace().next())
                    .unwrap_or_default()
                    .to_owned();
                let (path, query) = target.split_once('?').unwrap_or((target.as_str(), ""));
                if path != "/" {
                    let _ = stream
                        .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                        .await;
                    continue;
                }
                let query = parse_query(query);
                if !query.iter().any(|(name, value)| name == "state" && value == state) {
                    let _ = stream
                        .write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                        .await;
                    continue;
                }
                let answer = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
                     Cache-Control: no-store\r\nConnection: close\r\n\r\n{PAGE}",
                    PAGE.len()
                );
                let _ = stream.write_all(answer.as_bytes()).await;
                let _ = stream.shutdown().await;
                return Ok(query);
            }
        })
        .await
        .map_err(|_| NetError::Timeout("the sign-in in the browser"))?
    }
}

/// `code=a%20b&state=x` as pairs, percent-decoded.
pub fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            (decode(name), decode(value))
        })
        .collect()
}

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
            (b'+', ..) => {
                out.push(b' ');
                i += 1;
            }
            (byte, ..) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_strings_are_read() {
        assert_eq!(
            parse_query("code=M.R3_b%2Fx&state=abc+def&empty="),
            [
                ("code".to_owned(), "M.R3_b/x".to_owned()),
                ("state".to_owned(), "abc def".to_owned()),
                ("empty".to_owned(), String::new())
            ]
        );
        assert_eq!(
            parse_query("error=access_denied&error_description=No%21"),
            [
                ("error".to_owned(), "access_denied".to_owned()),
                ("error_description".to_owned(), "No!".to_owned())
            ]
        );
        assert_eq!(
            parse_query("a=%zz&b=%4"),
            [("a".to_owned(), "%zz".to_owned()), ("b".to_owned(), "%4".to_owned())]
        );
    }

    #[tokio::test]
    async fn the_browser_comes_back_with_the_code() {
        let redirect = Redirect::listen().await.unwrap();
        let port = redirect.port();
        let browser = tokio::spawn(async move {
            let ask = |path: &'static str| async move {
                let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
                stream
                    .write_all(format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes())
                    .await
                    .unwrap();
                let mut answer = String::new();
                stream.read_to_string(&mut answer).await.unwrap();
                answer
            };
            let icon = ask("/favicon.ico").await;
            let forged = ask("/?code=evil&state=guess").await;
            let page = ask("/?code=abc&state=xyz").await;
            (icon, forged, page)
        });
        let query = redirect.wait("xyz", Duration::from_secs(5)).await.unwrap();
        assert_eq!(
            query,
            [
                ("code".to_owned(), "abc".to_owned()),
                ("state".to_owned(), "xyz".to_owned())
            ]
        );
        let (icon, forged, page) = browser.await.unwrap();
        assert!(icon.starts_with("HTTP/1.1 404"));
        assert!(
            forged.starts_with("HTTP/1.1 400"),
            "another state does not end the wait"
        );
        assert!(page.starts_with("HTTP/1.1 200") && page.contains("go back to Regards"));
    }

    #[tokio::test]
    async fn nobody_coming_back_ends_in_time() {
        let redirect = Redirect::listen().await.unwrap();
        assert!(matches!(
            redirect.wait("s", Duration::from_millis(50)).await,
            Err(NetError::Timeout(_))
        ));
    }
}
