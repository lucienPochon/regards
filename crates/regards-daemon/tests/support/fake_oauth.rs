//! A fake OAuth token endpoint over HTTPS, on 127.0.0.1 with a throwaway
//! CA: it trades codes for tokens (checking PKCE) and renews them. The test
//! crate declares `fake_smtp` at its root, for the certificate.

// Each test uses a part of it.
#![allow(dead_code)]

use regards_core::encoding::base64;
use regards_net::loopback::parse_query;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// What the endpoint knows, and what it was asked.
#[derive(Default)]
pub struct Provider {
    /// Codes it will trade, each for the PKCE challenge it was issued with.
    pub codes: HashMap<String, String>,
    /// Refresh tokens it honours.
    pub refresh_tokens: HashSet<String>,
    /// The access token it gives (what the fake mail servers take).
    pub access: String,
    pub email: String,
    pub name: String,
    /// Each form posted to it.
    pub requests: Vec<HashMap<String, String>>,
    issued: u32,
}

pub struct TokenServer {
    pub port: u16,
    pub ca_pem: String,
    pub provider: Arc<Mutex<Provider>>,
}

impl TokenServer {
    pub fn url(&self) -> String {
        format!("https://127.0.0.1:{}/token", self.port)
    }

    pub fn grants(&self) -> Vec<String> {
        let provider = self.provider.lock().unwrap();
        provider
            .requests
            .iter()
            .map(|form| form.get("grant_type").cloned().unwrap_or_default())
            .collect()
    }
}

pub async fn serve(access: &str, email: &str) -> TokenServer {
    let pki = crate::fake_smtp::pki();
    let acceptor = pki.acceptor();
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let provider = Arc::new(Mutex::new(Provider {
        access: access.into(),
        email: email.into(),
        name: "Ada Lovelace".into(),
        ..Provider::default()
    }));
    let shared = Arc::clone(&provider);
    tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else { return };
            let acceptor = acceptor.clone();
            let provider = Arc::clone(&shared);
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(tcp).await else {
                    return;
                };
                let Some(form) = read_form(&mut tls).await else { return };
                let (status, body) = answer(&mut provider.lock().unwrap(), form);
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = tls.write_all(response.as_bytes()).await;
                let _ = tls.shutdown().await;
            });
        }
    });
    TokenServer {
        port,
        ca_pem: pki.ca_pem.clone(),
        provider,
    }
}

async fn read_form(stream: &mut (impl AsyncReadExt + Unpin)) -> Option<HashMap<String, String>> {
    let mut request = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        request.extend_from_slice(&chunk[..read]);
        let text = String::from_utf8_lossy(&request).into_owned();
        if let Some((head, body)) = text.split_once("\r\n\r\n") {
            let length: usize = head
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse().ok())?
                })
                .unwrap_or(0);
            if body.len() >= length {
                return Some(parse_query(body).into_iter().collect());
            }
        }
    }
}

fn answer(provider: &mut Provider, form: HashMap<String, String>) -> (&'static str, String) {
    provider.requests.push(form.clone());
    let field = |name: &str| form.get(name).map(String::as_str).unwrap_or_default();
    let refused = |error: &str| ("400 Bad Request", format!(r#"{{"error":"{error}"}}"#));
    match field("grant_type") {
        "authorization_code" => {
            let Some(challenge) = provider.codes.remove(field("code")) else {
                return refused("invalid_grant");
            };
            if challenge_of(field("code_verifier")) != challenge || field("redirect_uri").is_empty() {
                return refused("invalid_grant");
            }
            let refresh = provider.issue();
            let id_token = format!(
                "h.{}.s",
                base64_url(format!(r#"{{"email":"{}","name":"{}"}}"#, provider.email, provider.name).as_bytes())
            );
            (
                "200 OK",
                format!(
                    r#"{{"token_type":"Bearer","access_token":"{}","refresh_token":"{refresh}","expires_in":3600,"id_token":"{id_token}"}}"#,
                    provider.access
                ),
            )
        }
        "refresh_token" => {
            if !provider.refresh_tokens.contains(field("refresh_token")) {
                return refused("invalid_grant");
            }
            // A new refresh token each time, as some providers do.
            let refresh = provider.issue();
            (
                "200 OK",
                format!(
                    r#"{{"token_type":"Bearer","access_token":"{}","refresh_token":"{refresh}","expires_in":3600}}"#,
                    provider.access
                ),
            )
        }
        _ => refused("unsupported_grant_type"),
    }
}

impl Provider {
    fn issue(&mut self) -> String {
        self.issued += 1;
        let token = format!("refresh-{}", self.issued);
        self.refresh_tokens.insert(token.clone());
        token
    }
}

pub fn challenge_of(verifier: &str) -> String {
    base64_url(&Sha256::digest(verifier.as_bytes()))
}

fn base64_url(bytes: &[u8]) -> String {
    base64(bytes).trim_end_matches('=').replace('+', "-").replace('/', "_")
}
