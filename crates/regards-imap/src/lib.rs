//! The Regards IMAP client (ADR-0009), built on `imap-next` + `imap-codec`.
//!
//! Only this crate depends on imap-next and imap-types; it exposes Regards
//! types so the library can be replaced in one place (spike 07 §7).
//!
//! **Logging:** imap-next logs raw protocol bytes, including passwords and
//! mail, at TRACE level. Binaries must cap the `imap_next` target at INFO (the
//! `regards` CLI does).

mod conn;
mod messages;
mod shape;
mod tls;
pub mod utf7;

pub use messages::{
    Fetched, FetchedHeaders, FlagUpdate, Flags, MessageSet, Moved, Resynced, SearchTerm, Selected, Since, SystemFlag,
};

pub use conn::IdleEnd;
pub use shape::{Shape, TextPart, shape_of};
pub use tls::TlsOptions;

use conn::{Conn, Reply};
use imap_next::stream::Stream;
use imap_types::command::CommandBody;
use imap_types::core::Vec1;
use imap_types::extensions::enable::CapabilityEnable;
use imap_types::mailbox::Mailbox;
use imap_types::response::{Capability, Code, Data, GreetingKind, StatusBody, StatusKind};
use regards_core::secret::Secret;
use std::future::Future;
use std::time::Duration;

/// Errors connecting or signing in, phrased for the person setting up the account.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ImapError {
    #[error("{0}")]
    Connect(String),
    #[error("the server's security certificate was not accepted: {0}")]
    Tls(String),
    #[error("the server did not answer in time ({0})")]
    Timeout(&'static str),
    #[error("this does not look like an IMAP server: {0}")]
    NotImap(String),
    #[error("the server refused the user name or password: {0}")]
    AuthFailed(String),
    #[error("the server does not allow password sign-in on this connection")]
    LoginDisabled,
    #[error("unexpected server behaviour: {0}")]
    Protocol(String),
}

/// An IMAP server reached over TLS from the first byte (usually port 993).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImapServer {
    pub host: String,
    pub port: u16,
}

/// What a successful sign-in check learned about the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginCheck {
    /// Capabilities advertised after login, which can differ from those before.
    pub capabilities: Vec<String>,
}

/// Connect, sign in with a password, read the capabilities, and log out.
pub async fn verify_login(
    server: &ImapServer,
    username: &str,
    password: &Secret,
    tls: &TlsOptions,
) -> Result<LoginCheck, ImapError> {
    verify_sign_in(server, username, Auth::Password(password), tls).await
}

/// Connect over TLS, read the greeting and the capabilities offered before
/// sign-in, and leave: whether an IMAP server answers at `server`, with a
/// certificate valid for it (finding an address's servers).
pub async fn probe(server: &ImapServer, tls: &TlsOptions) -> Result<Vec<String>, ImapError> {
    let stream = tls::connect(&server.host, server.port, tls).await?;
    let (mut conn, greeting) = Conn::open(Stream::tls(stream.into())).await?;
    if let GreetingKind::Bye = greeting.kind {
        return Err(ImapError::NotImap(clean(greeting.text.as_ref())));
    }
    let capabilities = match greeting.code {
        Some(Code::Capability(caps)) => names(caps.as_ref()),
        _ => names_from(&conn.run(CommandBody::Capability).await?),
    };
    let _ = conn.run(CommandBody::Logout).await;
    Ok(capabilities)
}

/// [`verify_login`], with a password or an OAuth token.
pub async fn verify_sign_in(
    server: &ImapServer,
    username: &str,
    auth: Auth<'_>,
    tls: &TlsOptions,
) -> Result<LoginCheck, ImapError> {
    let session = Session::sign_in(server, username, auth, tls).await?;
    let capabilities = session.capabilities.clone();
    session.logout().await;
    Ok(LoginCheck { capabilities })
}

/// A mailbox as `LIST` reported it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedMailbox {
    /// The name exactly as the server sent it; use it in commands.
    pub remote_name: Vec<u8>,
    /// The name decoded for display (modified UTF-7).
    pub name: String,
    /// Hierarchy separator, `None` for a flat namespace.
    pub delimiter: Option<char>,
    /// Attributes such as `\Noselect` or `\Sent`.
    pub attributes: Vec<String>,
}

/// A signed-in IMAP connection.
pub struct Session {
    conn: Conn,
    capabilities: Vec<String>,
    qresync: bool,
}

/// `ENABLE QRESYNC`. A refusal is not an error: the session goes on without it.
async fn enable_qresync(conn: &mut Conn) -> Result<bool, ImapError> {
    let Ok(name) = CapabilityEnable::try_from("QRESYNC") else {
        return Ok(false);
    };
    let reply = conn
        .run(CommandBody::Enable {
            capabilities: Vec1::from(name),
        })
        .await?;
    Ok(reply.done.kind == StatusKind::Ok
        && reply.data.iter().any(|data| match data {
            Data::Enabled { capabilities } => capabilities
                .iter()
                .any(|enabled| enabled.to_string().eq_ignore_ascii_case("QRESYNC")),
            _ => false,
        }))
}

/// How a session signs in.
#[derive(Debug, Clone, Copy)]
pub enum Auth<'a> {
    Password(&'a Secret),
    /// An OAuth 2.0 access token, by SASL XOAUTH2 (ADR-0018).
    Bearer(&'a Secret),
}

impl Session {
    /// Connect over TLS and sign in with a password.
    pub async fn login(
        server: &ImapServer,
        username: &str,
        password: &Secret,
        tls: &TlsOptions,
    ) -> Result<Self, ImapError> {
        Self::sign_in(server, username, Auth::Password(password), tls).await
    }

    /// Connect over TLS and sign in, with a password or an OAuth token.
    pub async fn sign_in(
        server: &ImapServer,
        username: &str,
        auth: Auth<'_>,
        tls: &TlsOptions,
    ) -> Result<Self, ImapError> {
        let stream = tls::connect(&server.host, server.port, tls).await?;
        let (mut conn, greeting) = Conn::open(Stream::tls(stream.into())).await?;
        match greeting.kind {
            GreetingKind::Ok => {}
            GreetingKind::PreAuth => {
                return Err(ImapError::Protocol("the server signed us in without a password".into()));
            }
            GreetingKind::Bye => return Err(ImapError::NotImap(clean(greeting.text.as_ref()))),
        }

        let mut capabilities = match greeting.code {
            Some(Code::Capability(caps)) => names(caps.as_ref()),
            _ => names_from(&conn.run(CommandBody::Capability).await?),
        };
        let done = match auth {
            Auth::Password(password) => {
                if has(&capabilities, "LOGINDISABLED") {
                    return Err(ImapError::LoginDisabled);
                }
                let login = CommandBody::login(username.to_owned(), password.expose().to_owned()).map_err(|_| {
                    ImapError::AuthFailed("the user name or password contains characters IMAP cannot send".into())
                })?;
                conn.run(login).await?.done
            }
            Auth::Bearer(token) => {
                if !has(&capabilities, "AUTH=XOAUTH2") {
                    return Err(ImapError::AuthFailed("this server does not take OAuth sign-in".into()));
                }
                let sasl = format!("user={username}\u{1}auth=Bearer {}\u{1}\u{1}", token.expose());
                conn.authenticate_xoauth2(sasl.into_bytes(), has(&capabilities, "SASL-IR"))
                    .await?
            }
        };
        if done.kind != StatusKind::Ok {
            return Err(ImapError::AuthFailed(clean(done.text.as_ref())));
        }

        // Servers often include the new capabilities in the sign-in reply; otherwise ask.
        capabilities = match code_capabilities(&done) {
            Some(caps) => caps,
            None => names_from(&conn.run(CommandBody::Capability).await?),
        };

        let qresync = has(&capabilities, "QRESYNC") && enable_qresync(&mut conn).await?;
        Ok(Self {
            conn,
            capabilities,
            qresync,
        })
    }

    /// Whether QRESYNC (RFC 7162) is on for this session: a SELECT can then
    /// report what changed and vanished since a known modseq.
    pub fn qresync(&self) -> bool {
        self.qresync
    }

    /// Capabilities advertised after login.
    pub fn capabilities(&self) -> &[String] {
        &self.capabilities
    }

    pub fn has_capability(&self, name: &str) -> bool {
        has(&self.capabilities, name)
    }

    /// Every mailbox (`LIST "" "*"`), with SPECIAL-USE attributes where the server has them.
    pub async fn list_mailboxes(&mut self) -> Result<Vec<ListedMailbox>, ImapError> {
        let list = CommandBody::list("", "*").map_err(|_| ImapError::Protocol("cannot build LIST".into()))?;
        let reply = self.conn.run(list).await?;
        if reply.done.kind != StatusKind::Ok {
            return Err(ImapError::Protocol(format!(
                "LIST failed: {}",
                clean(reply.done.text.as_ref())
            )));
        }
        Ok(reply
            .data
            .into_iter()
            .filter_map(|data| match data {
                Data::List {
                    items,
                    delimiter,
                    mailbox,
                } => {
                    let remote_name = match &mailbox {
                        Mailbox::Inbox => b"INBOX".to_vec(),
                        Mailbox::Other(other) => other.as_ref().to_vec(),
                    };
                    Some(ListedMailbox {
                        name: utf7::decode(&remote_name),
                        remote_name,
                        delimiter: delimiter.map(|d| d.inner()),
                        attributes: items.iter().map(ToString::to_string).collect(),
                    })
                }
                _ => None,
            })
            .collect())
    }

    /// Wait in IDLE on the selected mailbox until something changes, `max`
    /// passes (re-IDLE about every 25 minutes, RFC 2177) or `stop` completes.
    pub async fn idle(&mut self, max: Duration, stop: impl Future<Output = ()>) -> Result<IdleEnd, ImapError> {
        self.conn.idle(max, stop).await
    }

    /// Sign out. Failures are ignored: the connection closes either way.
    pub async fn logout(mut self) {
        if let Err(error) = self.conn.run(CommandBody::Logout).await {
            tracing::debug!(%error, "LOGOUT failed");
        }
    }
}

/// Capability names, upper-cased: IMAP compares them case-insensitively.
fn names(caps: &[Capability<'_>]) -> Vec<String> {
    caps.iter().map(|cap| cap.to_string().to_ascii_uppercase()).collect()
}

fn names_from(reply: &Reply) -> Vec<String> {
    reply
        .data
        .iter()
        .find_map(|data| match data {
            Data::Capability(caps) => Some(names(caps.as_ref())),
            _ => None,
        })
        .unwrap_or_default()
}

fn code_capabilities(status: &StatusBody<'_>) -> Option<Vec<String>> {
    match &status.code {
        Some(Code::Capability(caps)) => Some(names(caps.as_ref())),
        _ => None,
    }
}

fn has(capabilities: &[String], name: &str) -> bool {
    capabilities.iter().any(|cap| cap.eq_ignore_ascii_case(name))
}

/// Server text shown to the user: printable, single line, bounded.
pub(crate) fn clean(text: &str) -> String {
    let text: String = text.chars().filter(|c| !c.is_control()).take(200).collect();
    if text.is_empty() {
        "no reason given".into()
    } else {
        text
    }
}
