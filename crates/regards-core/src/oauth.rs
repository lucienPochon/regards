//! OAuth 2.0 sign-in for mail (ADR-0018): the authorization code flow with
//! PKCE (RFC 7636), through the system browser and a loopback redirect
//! (RFC 8252), then XOAUTH2 for IMAP and SMTP.
//!
//! This builds what is sent and reads what comes back; the daemon makes the
//! requests. Tokens are [`Secret`]s: never logged, and kept only in the OS
//! keychain.
//!
//! A provider's sign-in needs the app registered with it, which gives a
//! client id ([`Client`]). Without one, that provider's accounts sign in
//! with an app password, if the provider allows it.

use crate::encoding::base64;
use crate::secret::Secret;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    /// Outlook.com, Hotmail, Microsoft 365: OAuth only since September 2024.
    Microsoft,
    /// Gmail and Google Workspace.
    Google,
}

impl Provider {
    pub fn label(self) -> &'static str {
        match self {
            Self::Microsoft => "Microsoft",
            Self::Google => "Google",
        }
    }

    pub fn authorize_endpoint(self) -> &'static str {
        match self {
            Self::Microsoft => "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
            Self::Google => "https://accounts.google.com/o/oauth2/v2/auth",
        }
    }

    pub fn token_endpoint(self) -> &'static str {
        match self {
            Self::Microsoft => "https://login.microsoftonline.com/common/oauth2/v2.0/token",
            Self::Google => "https://oauth2.googleapis.com/token",
        }
    }

    /// Mail access (IMAP and SMTP), a refresh token, and the address.
    pub fn scopes(self) -> &'static str {
        match self {
            Self::Microsoft => {
                "https://outlook.office.com/IMAP.AccessAsUser.All https://outlook.office.com/SMTP.Send \
                 offline_access openid email"
            }
            Self::Google => "https://mail.google.com/ openid email",
        }
    }

    /// The IMAP server (TLS) and the SMTP submission server (STARTTLS).
    pub fn servers(self) -> ((&'static str, u16), (&'static str, u16)) {
        match self {
            Self::Microsoft => (("outlook.office365.com", 993), ("smtp.office365.com", 587)),
            Self::Google => (("imap.gmail.com", 993), ("smtp.gmail.com", 587)),
        }
    }

    /// The loopback redirect each accepts for a desktop app.
    pub fn redirect_uri(self, port: u16) -> String {
        match self {
            // Registered as http://localhost; the port is not part of the match.
            Self::Microsoft => format!("http://localhost:{port}/"),
            Self::Google => format!("http://127.0.0.1:{port}/"),
        }
    }

    /// Whose addresses are this provider's, to offer its sign-in.
    pub fn for_address(address: &str) -> Option<Self> {
        let domain = address.rsplit_once('@')?.1.to_ascii_lowercase();
        match domain.as_str() {
            "outlook.com" | "hotmail.com" | "live.com" | "msn.com" | "outlook.fr" | "hotmail.fr" | "live.fr"
            | "hotmail.co.uk" | "outlook.de" | "hotmail.de" => Some(Self::Microsoft),
            "gmail.com" | "googlemail.com" => Some(Self::Google),
            _ => None,
        }
    }
}

/// The app as registered with a provider: its client id and, for Google's
/// installed-app clients, the secret it ships with (public by design).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Client {
    pub provider: Provider,
    pub id: String,
    pub secret: Option<String>,
    /// Where the browser signs in and where tokens come from: the
    /// provider's own, unless a test points elsewhere.
    pub authorize_endpoint: String,
    pub token_endpoint: String,
}

impl Client {
    pub fn new(provider: Provider, id: impl Into<String>, secret: Option<String>) -> Self {
        Self {
            provider,
            id: id.into(),
            secret,
            authorize_endpoint: provider.authorize_endpoint().to_owned(),
            token_endpoint: provider.token_endpoint().to_owned(),
        }
    }
}

/// A PKCE pair: the verifier stays here, its challenge goes to the browser.
#[derive(Debug, Clone)]
pub struct Pkce {
    pub verifier: Secret,
    pub challenge: String,
}

impl Default for Pkce {
    fn default() -> Self {
        Self::new()
    }
}

impl Pkce {
    pub fn new() -> Self {
        let verifier = random_token();
        let challenge = base64_url(&Sha256::digest(verifier.as_bytes()));
        Self {
            verifier: Secret::new(verifier),
            challenge,
        }
    }
}

/// 32 random bytes, base64url: for `state`, and a PKCE verifier.
pub fn random_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the operating system's random source");
    base64_url(&bytes)
}

fn base64_url(bytes: &[u8]) -> String {
    base64(bytes).trim_end_matches('=').replace('+', "-").replace('/', "_")
}

fn base64_url_decode(text: &str) -> Option<Vec<u8>> {
    crate::encoding::base64_decode(&text.replace('-', "+").replace('_', "/"))
}

/// Everything but the unreserved characters, percent-encoded.
fn query_value(text: &str) -> String {
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

/// Where the browser goes to sign in.
pub fn authorize_url(
    client: &Client,
    redirect_uri: &str,
    state: &str,
    pkce: &Pkce,
    login_hint: Option<&str>,
) -> String {
    let mut params = vec![
        ("client_id", client.id.as_str()),
        ("response_type", "code"),
        ("redirect_uri", redirect_uri),
        ("scope", client.provider.scopes()),
        ("state", state),
        ("code_challenge", pkce.challenge.as_str()),
        ("code_challenge_method", "S256"),
    ];
    match client.provider {
        // A refresh token, and asked for again if one was given before.
        Provider::Google => params.extend([("access_type", "offline"), ("prompt", "consent")]),
        Provider::Microsoft => params.push(("prompt", "select_account")),
    }
    if let Some(hint) = login_hint {
        params.push(("login_hint", hint));
    }
    let query: Vec<String> = params
        .iter()
        .map(|(name, value)| format!("{name}={}", query_value(value)))
        .collect();
    format!("{}?{}", client.authorize_endpoint, query.join("&"))
}

/// The token request for the code the browser brought back.
pub fn code_form(client: &Client, code: &str, pkce: &Pkce, redirect_uri: &str) -> Vec<(&'static str, String)> {
    let mut form = vec![
        ("grant_type", "authorization_code".to_owned()),
        ("client_id", client.id.clone()),
        ("code", code.to_owned()),
        ("code_verifier", pkce.verifier.expose().to_owned()),
        ("redirect_uri", redirect_uri.to_owned()),
    ];
    if let Some(secret) = &client.secret {
        form.push(("client_secret", secret.clone()));
    }
    form
}

/// The token request for a new access token.
pub fn refresh_form(client: &Client, refresh: &Secret) -> Vec<(&'static str, String)> {
    let mut form = vec![
        ("grant_type", "refresh_token".to_owned()),
        ("client_id", client.id.clone()),
        ("refresh_token", refresh.expose().to_owned()),
    ];
    if client.provider == Provider::Microsoft {
        form.push(("scope", client.provider.scopes().to_owned()));
    }
    if let Some(secret) = &client.secret {
        form.push(("client_secret", secret.clone()));
    }
    form
}

/// What a token request gave.
#[derive(Debug, Clone)]
pub struct Tokens {
    pub access: Secret,
    /// Given at sign-in, and sometimes anew on refresh (keep the new one).
    pub refresh: Option<Secret>,
    /// Seconds the access token lasts.
    pub expires_in: u64,
    /// The account's address, from the ID token (sign-in only).
    pub email: Option<String>,
    /// The person's name, from the ID token, when the provider gives it.
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OauthError {
    /// The refresh token no longer works (revoked, expired, password changed).
    #[error("the sign-in has expired; sign in again")]
    SignInAgain,
    #[error("the sign-in was refused: {0}")]
    Refused(String),
    #[error("the sign-in service gave an answer Regards cannot read")]
    Unreadable,
}

/// The token endpoint's answer, with its HTTP status.
pub fn read_tokens(status: u16, body: &[u8]) -> Result<Tokens, OauthError> {
    #[derive(Deserialize)]
    struct Answer {
        access_token: Option<String>,
        refresh_token: Option<String>,
        expires_in: Option<u64>,
        id_token: Option<String>,
        error: Option<String>,
        error_description: Option<String>,
    }
    let answer: Answer = serde_json::from_slice(body).map_err(|_| OauthError::Unreadable)?;
    if let Some(error) = answer.error {
        return Err(if error == "invalid_grant" {
            OauthError::SignInAgain
        } else {
            // The description is the provider's, for the person; never a token.
            OauthError::Refused(
                answer
                    .error_description
                    .unwrap_or(error)
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_owned(),
            )
        });
    }
    if !(200..300).contains(&status) {
        return Err(OauthError::Refused(format!("HTTP {status}")));
    }
    let access = answer.access_token.ok_or(OauthError::Unreadable)?;
    let claims = answer.id_token.as_deref().and_then(claims_of);
    Ok(Tokens {
        access: Secret::new(access),
        refresh: answer.refresh_token.map(Secret::new),
        expires_in: answer.expires_in.unwrap_or(3600),
        email: claims.as_ref().and_then(email_in),
        name: claims
            .as_ref()
            .and_then(|claims| claims.get("name")?.as_str())
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned),
    })
}

/// An ID token's claims. It came straight from the token endpoint over TLS
/// (OpenID Connect §3.1.3.7), so its signature is not checked here.
fn claims_of(id_token: &str) -> Option<serde_json::Value> {
    let payload = id_token.split('.').nth(1)?;
    serde_json::from_slice(&base64_url_decode(payload)?).ok()
}

fn email_in(claims: &serde_json::Value) -> Option<String> {
    ["email", "preferred_username", "upn"]
        .iter()
        .find_map(|claim| claims.get(claim)?.as_str())
        .filter(|address| address.contains('@'))
        .map(str::to_owned)
}

/// The address in an ID token's claims.
pub fn email_of(id_token: &str) -> Option<String> {
    email_in(&claims_of(id_token)?)
}

/// The XOAUTH2 SASL string, base64: for IMAP AUTHENTICATE and SMTP AUTH.
pub fn xoauth2(user: &str, access: &Secret) -> Secret {
    Secret::new(base64(
        format!("user={user}\u{1}auth=Bearer {}\u{1}\u{1}", access.expose()).as_bytes(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn microsoft() -> Client {
        Client::new(Provider::Microsoft, "00000000-test", None)
    }

    #[test]
    fn pkce_challenges_are_the_verifiers_digest() {
        let pkce = Pkce::new();
        assert_eq!(pkce.verifier.expose().len(), 43, "32 bytes, base64url");
        assert!(!pkce.challenge.contains(['=', '+', '/']));
        // RFC 7636 appendix B.
        let known = base64_url(&Sha256::digest(b"dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"));
        assert_eq!(known, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        assert_ne!(Pkce::new().verifier.expose(), pkce.verifier.expose());
    }

    #[test]
    fn the_browser_is_sent_where_the_provider_signs_in() {
        let pkce = Pkce::new();
        let url = authorize_url(
            &microsoft(),
            &Provider::Microsoft.redirect_uri(4321),
            "st",
            &pkce,
            Some("ada@outlook.com"),
        );
        assert!(
            url.starts_with("https://login.microsoftonline.com/common/oauth2/v2.0/authorize?client_id=00000000-test&")
        );
        assert!(url.contains("&redirect_uri=http%3A%2F%2Flocalhost%3A4321%2F&"));
        assert!(url.contains("&scope=https%3A%2F%2Foutlook.office.com%2FIMAP.AccessAsUser.All%20"));
        assert!(url.contains(&format!(
            "&code_challenge={}&code_challenge_method=S256",
            pkce.challenge
        )));
        assert!(url.contains("&state=st&") && url.ends_with("&login_hint=ada%40outlook.com"));
        let google = Client::new(Provider::Google, "g", Some("public".into()));
        let url = authorize_url(&google, &Provider::Google.redirect_uri(9), "s", &pkce, None);
        assert!(url.contains("access_type=offline") && url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A9%2F"));
        let form = code_form(&google, "c0de", &pkce, "http://127.0.0.1:9/");
        assert!(form.contains(&("client_secret", "public".to_owned())));
        assert!(form.contains(&("code_verifier", pkce.verifier.expose().to_owned())));
        let refresh = refresh_form(&microsoft(), &Secret::new("r1"));
        assert!(refresh.contains(&("grant_type", "refresh_token".to_owned())));
        assert!(
            refresh.iter().any(|(name, _)| *name == "scope"),
            "Microsoft wants the scopes again"
        );
    }

    #[test]
    fn token_answers_are_read_with_the_address() {
        let claims = base64_url(br#"{"aud":"x","preferred_username":"ada@outlook.com","name":"Ada"}"#);
        let body = format!(
            r#"{{"token_type":"Bearer","access_token":"at","refresh_token":"rt","expires_in":3599,"id_token":"h.{claims}.s"}}"#
        );
        let tokens = read_tokens(200, body.as_bytes()).unwrap();
        assert_eq!(tokens.access.expose(), "at");
        assert_eq!(tokens.refresh.as_ref().map(Secret::expose), Some("rt"));
        assert_eq!(
            (tokens.expires_in, tokens.email.as_deref(), tokens.name.as_deref()),
            (3599, Some("ada@outlook.com"), Some("Ada"))
        );

        let expired = br#"{"error":"invalid_grant","error_description":"AADSTS70008: expired"}"#;
        assert_eq!(read_tokens(400, expired).unwrap_err(), OauthError::SignInAgain);
        let refused =
            br#"{"error":"invalid_client","error_description":"AADSTS7000215: Invalid client.\r\nTrace ID: x"}"#;
        assert_eq!(
            read_tokens(401, refused).unwrap_err(),
            OauthError::Refused("AADSTS7000215: Invalid client.".into())
        );
        assert_eq!(read_tokens(200, b"<html>").unwrap_err(), OauthError::Unreadable);
        assert_eq!(
            read_tokens(500, b"{}").unwrap_err(),
            OauthError::Refused("HTTP 500".into())
        );
    }

    #[test]
    fn xoauth2_is_the_sasl_string() {
        let sasl = xoauth2("ada@outlook.com", &Secret::new("tok"));
        let decoded = crate::encoding::base64_decode(sasl.expose()).unwrap();
        assert_eq!(decoded, b"user=ada@outlook.com\x01auth=Bearer tok\x01\x01");
        assert_eq!(Provider::for_address("Ada@Hotmail.com"), Some(Provider::Microsoft));
        assert_eq!(Provider::for_address("ada@gmail.com"), Some(Provider::Google));
        assert_eq!(Provider::for_address("ada@infomaniak.ch"), None);
    }
}
