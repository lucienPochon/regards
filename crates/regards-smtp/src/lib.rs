//! SMTP submission (RFC 6409) for sending mail (ADR-0009, ADR-0010 §3).
//!
//! Small on purpose: one message per connection, no pipelining. It knows at
//! every moment whether the server can have accepted the message, which is
//! what at-most-once sending needs:
//!
//! - anything that fails before the final `.` of DATA is sent leaves the
//!   message **not sent** ([`SubmitError::NotSent`]); `retry` says whether
//!   trying again later can help;
//! - a reply to the final `.` settles it either way;
//! - losing the connection after the final `.` and before its reply is
//!   [`SubmitError::Uncertain`]: the server may have accepted the message, so
//!   it must never be resent without asking.
//!
//! Credentials only ever travel over TLS. Nothing here logs; errors never
//! contain the password or the message.

use regards_core::encoding::base64;
use regards_core::secret::Secret;
use regards_net::connlog::Conversation;
use regards_net::{NetError, TlsOptions};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

/// How long the server may take to answer a command.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);
/// How long the server may take to accept the message after its last line (RFC 5321 §4.5.3.2.6 suggests 10 min).
const DATA_END_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// Longer reply lines, or more of them, are not a real server's.
const MAX_REPLY_LINE: usize = 4096;
const MAX_REPLY_LINES: usize = 100;

/// How the connection is secured. Both mean TLS before any credential is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Security {
    /// TLS from the first byte (usually port 465).
    Tls,
    /// Plain connection upgraded with STARTTLS, which is required (usually port 587).
    StartTls,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmtpServer {
    pub host: String,
    pub port: u16,
    pub security: Security,
}

pub struct Credentials<'a> {
    pub username: &'a str,
    pub password: &'a Secret,
    /// `password` is an OAuth 2.0 access token, for SASL XOAUTH2 (ADR-0018).
    pub oauth: bool,
}

/// Who the message is from and who receives it (To, Cc and Bcc), as plain addresses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    pub from: String,
    pub to: Vec<String>,
}

/// The server took the message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submitted {
    /// Its reply to the end of the message, for example "2.0.0 Ok: queued as 4F2A".
    pub reply: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SubmitError {
    /// The message was not sent. `retry`: trying again later may work
    /// (offline, a timeout, a temporary refusal); otherwise something must
    /// change first (password, an address, the size).
    #[error("{reason}")]
    NotSent { reason: String, retry: bool },
    /// The server turned the sign-in down: a wrong password, or an OAuth
    /// token that expired or was withdrawn. Nothing was sent; a fresh token
    /// may work, the same password will not.
    #[error("{0}")]
    SignIn(String),
    /// The connection failed after the whole message was handed over: it may
    /// or may not have been sent. Never resend without asking.
    #[error("{0}")]
    Uncertain(String),
}

impl SubmitError {
    fn later(reason: impl Into<String>) -> Self {
        Self::NotSent {
            reason: reason.into(),
            retry: true,
        }
    }

    fn refused(reason: impl Into<String>) -> Self {
        Self::NotSent {
            reason: reason.into(),
            retry: false,
        }
    }
}

impl From<NetError> for SubmitError {
    fn from(error: NetError) -> Self {
        match error {
            // A certificate problem does not go away by waiting.
            NetError::Tls(reason) => {
                SubmitError::refused(format!("the server's security certificate was not accepted: {reason}"))
            }
            other => SubmitError::later(other.to_string()),
        }
    }
}

/// Send `message` (complete, with headers; CRLF or LF line ends) to `envelope`'s recipients.
pub async fn submit(
    server: &SmtpServer,
    credentials: &Credentials<'_>,
    envelope: &Envelope,
    message: &[u8],
    tls: &TlsOptions,
) -> Result<Submitted, SubmitError> {
    check_address(&envelope.from)?;
    if envelope.to.is_empty() {
        return Err(SubmitError::refused("there are no recipients"));
    }
    for address in &envelope.to {
        check_address(address)?;
    }
    let mut session = Session::open(server, tls).await?;
    session.authenticate(credentials).await?;
    let outcome = session.send(envelope, message).await;
    session.quit().await;
    outcome
}

/// Connect and sign in, then leave: checks an account's sending settings.
pub async fn verify_login(
    server: &SmtpServer,
    credentials: &Credentials<'_>,
    tls: &TlsOptions,
) -> Result<(), SubmitError> {
    let mut session = Session::open(server, tls).await?;
    session.authenticate(credentials).await?;
    session.quit().await;
    Ok(())
}

/// Connect (with STARTTLS when that is the security), greet, and leave:
/// whether a submission server answers at `server`, with a certificate
/// valid for it (finding an address's servers).
pub async fn probe(server: &SmtpServer, tls: &TlsOptions) -> Result<(), SubmitError> {
    let session = Session::open(server, tls).await?;
    session.quit().await;
    Ok(())
}

/// An address for MAIL FROM or RCPT TO. Anything that could end the command
/// line or the angle brackets is refused, so no command can be smuggled in.
fn check_address(address: &str) -> Result<(), SubmitError> {
    let valid = !address.is_empty()
        && address.len() <= 320
        && address.contains('@')
        && !address
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '<' | '>'));
    if valid {
        Ok(())
    } else {
        Err(SubmitError::refused(format!(
            "{address:?} is not a valid email address"
        )))
    }
}

/// A server reply: its code and text lines.
#[derive(Debug)]
struct Reply {
    code: u16,
    lines: Vec<String>,
}

impl Reply {
    fn text(&self) -> String {
        self.lines.join(" ")
    }

    fn positive(&self) -> bool {
        (200..400).contains(&self.code)
    }

    /// A refusal, as an error: 4xx may pass, 5xx will not.
    fn refusal(&self, what: &str) -> SubmitError {
        let reason = format!("{what}: the server said {} {}", self.code, self.text());
        if (400..500).contains(&self.code) {
            SubmitError::later(reason)
        } else {
            SubmitError::refused(reason)
        }
    }
}

trait Wire: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Wire for T {}

struct Session {
    stream: BufReader<Box<dyn Wire>>,
    extensions: Vec<String>,
    /// The connection log: command names and replies only (see [`heard`]).
    log: Arc<Conversation>,
}

impl Session {
    async fn open(server: &SmtpServer, tls: &TlsOptions) -> Result<Self, SubmitError> {
        let log = Arc::new(Conversation::new("smtp", &server.host));
        let mut session = match server.security {
            Security::Tls => {
                let stream = regards_net::connect_tls(&server.host, server.port, tls).await?;
                Self::new(Box::new(stream), log)
            }
            Security::StartTls => {
                let tcp = regards_net::connect(&server.host, server.port).await?;
                let mut plain = Plain::new(tcp, Arc::clone(&log));
                plain.greeting().await?;
                let extensions = plain.ehlo().await?;
                if !has(&extensions, "STARTTLS") {
                    return Err(SubmitError::refused(
                        "the server does not offer STARTTLS, so the password cannot be sent safely",
                    ));
                }
                let tcp = plain.start_tls().await?;
                let stream = regards_net::start_tls(tcp, &server.host, server.port, tls).await?;
                log.note("TLS started");
                let mut session = Self::new(Box::new(stream), log);
                session.extensions = session.ehlo().await?;
                return Ok(session);
            }
        };
        session.greeting().await?;
        session.extensions = session.ehlo().await?;
        Ok(session)
    }

    fn new(stream: Box<dyn Wire>, log: Arc<Conversation>) -> Self {
        Self {
            stream: BufReader::new(stream),
            extensions: Vec::new(),
            log,
        }
    }

    async fn greeting(&mut self) -> Result<(), SubmitError> {
        let reply = timed(COMMAND_TIMEOUT, "the greeting", read_reply(&mut self.stream)).await?;
        heard(&self.log, &reply, false);
        if reply.code != 220 {
            return Err(reply.refusal("the server is not accepting mail"));
        }
        Ok(())
    }

    async fn ehlo(&mut self) -> Result<Vec<String>, SubmitError> {
        let reply = self.command("EHLO [127.0.0.1]", "EHLO").await?;
        if reply.code != 250 {
            return Err(reply.refusal("the server refused EHLO"));
        }
        Ok(reply
            .lines
            .into_iter()
            .skip(1)
            .map(|line| line.to_ascii_uppercase())
            .collect())
    }

    /// `what` names the command for errors and the connection log, which
    /// never gets `line` (it can hold a password or an address).
    async fn command(&mut self, line: &str, what: &'static str) -> Result<Reply, SubmitError> {
        let signing_in = what == "AUTH";
        self.log.client(if signing_in { "AUTH [redacted]" } else { what });
        let reply = timed(COMMAND_TIMEOUT, what, async {
            write_line(&mut self.stream, line).await?;
            read_reply(&mut self.stream).await
        })
        .await;
        match &reply {
            Ok(reply) => heard(&self.log, reply, signing_in),
            Err(error) => self.log.note(&error.to_string()),
        }
        reply
    }

    async fn authenticate(&mut self, credentials: &Credentials<'_>) -> Result<(), SubmitError> {
        let mechanisms: Vec<String> = self
            .extensions
            .iter()
            .filter_map(|line| line.strip_prefix("AUTH ").or_else(|| line.strip_prefix("AUTH=")))
            .flat_map(|list| list.split_whitespace().map(str::to_owned).collect::<Vec<_>>())
            .collect();
        let password = credentials.password.expose();
        let reply = if credentials.oauth {
            if !mechanisms.iter().any(|m| m == "XOAUTH2") {
                return Err(SubmitError::refused(
                    "this server does not take OAuth sign-in (AUTH XOAUTH2)",
                ));
            }
            let sasl = base64(format!("user={}\u{1}auth=Bearer {password}\u{1}\u{1}", credentials.username).as_bytes());
            let reply = self.command(&format!("AUTH XOAUTH2 {sasl}"), "AUTH").await?;
            // A refusal first comes as a challenge carrying the error, ended with an empty line.
            if reply.code == 334 {
                self.command("", "AUTH").await?
            } else {
                reply
            }
        } else if mechanisms.iter().any(|m| m == "PLAIN") {
            let token = base64(format!("\0{}\0{password}", credentials.username).as_bytes());
            self.command(&format!("AUTH PLAIN {token}"), "AUTH").await?
        } else if mechanisms.iter().any(|m| m == "LOGIN") {
            let challenge = self.command("AUTH LOGIN", "AUTH").await?;
            if challenge.code != 334 {
                return Err(challenge.refusal("the server refused sign-in"));
            }
            let challenge = self.command(&base64(credentials.username.as_bytes()), "AUTH").await?;
            if challenge.code != 334 {
                return Err(challenge.refusal("the server refused the user name"));
            }
            self.command(&base64(password.as_bytes()), "AUTH").await?
        } else {
            return Err(SubmitError::refused(
                "the server offers no password sign-in (AUTH PLAIN or LOGIN)",
            ));
        };
        match reply.code {
            235 => Ok(()),
            code if (400..500).contains(&code) => Err(reply.refusal("sign-in failed for now")),
            _ if credentials.oauth => Err(SubmitError::SignIn(format!(
                "the server refused the sign-in: {}",
                reply.text()
            ))),
            _ => Err(SubmitError::SignIn(format!(
                "the server refused the user name or password: {}",
                reply.text()
            ))),
        }
    }

    async fn send(&mut self, envelope: &Envelope, message: &[u8]) -> Result<Submitted, SubmitError> {
        let data = dot_stuff(message);
        if let Some(limit) = self.size_limit()
            && data.len() as u64 > limit
        {
            return Err(SubmitError::refused(format!(
                "the message is larger than the server accepts ({} KB, at most {} KB)",
                data.len() / 1024,
                limit / 1024
            )));
        }
        let mut from = format!("MAIL FROM:<{}>", envelope.from);
        if self.size_limit().is_some() {
            from.push_str(&format!(" SIZE={}", data.len()));
        }
        let non_ascii = !envelope.from.is_ascii() || envelope.to.iter().any(|a| !a.is_ascii());
        if non_ascii {
            if !has(&self.extensions, "SMTPUTF8") {
                return Err(SubmitError::refused(
                    "an address has non-ASCII characters, which this server cannot take",
                ));
            }
            from.push_str(" SMTPUTF8");
        }
        let reply = self.command(&from, "MAIL FROM").await?;
        if !reply.positive() {
            return Err(reply.refusal("the server refused the sender"));
        }
        for address in &envelope.to {
            let reply = self.command(&format!("RCPT TO:<{address}>"), "RCPT TO").await?;
            if !reply.positive() {
                let _ = self.command("RSET", "RSET").await;
                return Err(reply.refusal(&format!("the server refused the recipient {address}")));
            }
        }
        let reply = self.command("DATA", "DATA").await?;
        if reply.code != 354 {
            return Err(reply.refusal("the server refused the message"));
        }
        self.log.client(&format!("[the message, {} bytes]", data.len()));
        // Until the final line is written, the server cannot have accepted anything.
        timed(COMMAND_TIMEOUT, "sending the message", async {
            self.stream.write_all(&data).await.map_err(lost)?;
            Ok(())
        })
        .await?;
        // From here on, a lost connection leaves the outcome unknown.
        let end = async {
            self.stream.write_all(b".\r\n").await?;
            self.stream.flush().await?;
            read_reply(&mut self.stream)
                .await
                .map_err(|error| std::io::Error::other(error.to_string()))
        };
        let reply = match tokio::time::timeout(DATA_END_TIMEOUT, end).await {
            Ok(Ok(reply)) => reply,
            Ok(Err(error)) => {
                return Err(SubmitError::Uncertain(format!(
                    "the connection was lost while the server was accepting the message ({error}); it may or may not have been sent"
                )));
            }
            Err(_) => {
                return Err(SubmitError::Uncertain(
                    "the server did not confirm the message in time; it may or may not have been sent".into(),
                ));
            }
        };
        heard(&self.log, &reply, false);
        if reply.code == 250 {
            Ok(Submitted { reply: reply.text() })
        } else {
            Err(reply.refusal("the server did not accept the message"))
        }
    }

    fn size_limit(&self) -> Option<u64> {
        self.extensions
            .iter()
            .find_map(|line| line.strip_prefix("SIZE"))
            .and_then(|rest| rest.trim().parse::<u64>().ok())
            .filter(|limit| *limit > 0)
    }

    async fn quit(mut self) {
        self.log.client("QUIT");
        let _ = tokio::time::timeout(Duration::from_secs(5), async {
            let _ = write_line(&mut self.stream, "QUIT").await;
            if let Ok(reply) = read_reply(&mut self.stream).await {
                heard(&self.log, &reply, false);
            }
        })
        .await;
    }
}

/// The plain part of a STARTTLS connection.
struct Plain {
    stream: BufReader<TcpStream>,
    log: Arc<Conversation>,
}

impl Plain {
    fn new(tcp: TcpStream, log: Arc<Conversation>) -> Self {
        Self {
            stream: BufReader::new(tcp),
            log,
        }
    }

    async fn greeting(&mut self) -> Result<(), SubmitError> {
        let reply = timed(COMMAND_TIMEOUT, "the greeting", read_reply(&mut self.stream)).await?;
        heard(&self.log, &reply, false);
        if reply.code != 220 {
            return Err(reply.refusal("the server is not accepting mail"));
        }
        Ok(())
    }

    async fn ehlo(&mut self) -> Result<Vec<String>, SubmitError> {
        self.log.client("EHLO");
        let reply = timed(COMMAND_TIMEOUT, "EHLO", async {
            write_line(&mut self.stream, "EHLO [127.0.0.1]").await?;
            read_reply(&mut self.stream).await
        })
        .await?;
        heard(&self.log, &reply, false);
        if reply.code != 250 {
            return Err(reply.refusal("the server refused EHLO"));
        }
        Ok(reply
            .lines
            .into_iter()
            .skip(1)
            .map(|line| line.to_ascii_uppercase())
            .collect())
    }

    async fn start_tls(mut self) -> Result<TcpStream, SubmitError> {
        self.log.client("STARTTLS");
        let reply = timed(COMMAND_TIMEOUT, "STARTTLS", async {
            write_line(&mut self.stream, "STARTTLS").await?;
            read_reply(&mut self.stream).await
        })
        .await?;
        heard(&self.log, &reply, false);
        if reply.code != 220 {
            return Err(reply.refusal("the server refused STARTTLS"));
        }
        // Anything already sent after the reply came in the clear and could be
        // an attacker's (CVE-2011-0411): refuse rather than read it as encrypted.
        if !self.stream.buffer().is_empty() {
            return Err(SubmitError::refused(
                "the server sent unexpected data when starting encryption",
            ));
        }
        Ok(self.stream.into_inner())
    }
}

/// A reply, to the connection log: each line with its code. A sign-in's
/// challenges stay out (they carry what the sign-in sends back).
fn heard(log: &Conversation, reply: &Reply, signing_in: bool) {
    if signing_in && reply.code == 334 {
        log.server("334 [redacted]");
        return;
    }
    for line in &reply.lines {
        log.server(&format!("{} {line}", reply.code));
    }
}

async fn timed<T>(
    limit: Duration,
    what: &'static str,
    work: impl Future<Output = Result<T, SubmitError>>,
) -> Result<T, SubmitError> {
    tokio::time::timeout(limit, work)
        .await
        .map_err(|_| SubmitError::later(format!("the server did not answer in time ({what})")))?
}

fn lost(error: std::io::Error) -> SubmitError {
    SubmitError::later(format!("the connection was lost: {error}"))
}

async fn write_line<W: AsyncWrite + Unpin>(stream: &mut W, line: &str) -> Result<(), SubmitError> {
    stream.write_all(line.as_bytes()).await.map_err(lost)?;
    stream.write_all(b"\r\n").await.map_err(lost)?;
    stream.flush().await.map_err(lost)
}

/// Read one reply: `ddd-text` lines, then `ddd text`.
async fn read_reply<R: AsyncBufRead + Unpin>(stream: &mut R) -> Result<Reply, SubmitError> {
    let mut lines = Vec::new();
    loop {
        let mut line = Vec::new();
        let read = (&mut *stream)
            .take(MAX_REPLY_LINE as u64)
            .read_until(b'\n', &mut line)
            .await
            .map_err(lost)?;
        if read == 0 {
            return Err(SubmitError::later("the server closed the connection"));
        }
        if !line.ends_with(b"\n") {
            return Err(SubmitError::later("the server sent an overlong reply line"));
        }
        let text = String::from_utf8_lossy(&line);
        let text = text.trim_end_matches(['\r', '\n']);
        let code = text
            .get(..3)
            .and_then(|digits| digits.parse::<u16>().ok())
            .filter(|code| (200..600).contains(code))
            .ok_or_else(|| SubmitError::later("the server sent something that is not an SMTP reply"))?;
        let last = text.as_bytes().get(3) != Some(&b'-');
        lines.push(text.get(4..).unwrap_or("").to_owned());
        if last {
            return Ok(Reply { code, lines });
        }
        if lines.len() >= MAX_REPLY_LINES {
            return Err(SubmitError::later("the server sent an overlong reply"));
        }
    }
}

fn has(extensions: &[String], name: &str) -> bool {
    extensions
        .iter()
        .any(|line| line.split_whitespace().next() == Some(name))
}

/// The message as DATA sends it: every line ends in CRLF, a line starting
/// with `.` gets another one (RFC 5321 §4.5.2), and the last line ends too.
fn dot_stuff(message: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(message.len() + message.len() / 50 + 2);
    let mut at_line_start = true;
    let mut i = 0;
    while i < message.len() {
        let byte = message[i];
        if at_line_start && byte == b'.' {
            out.push(b'.');
        }
        match byte {
            b'\r' if message.get(i + 1) == Some(&b'\n') => {
                out.extend_from_slice(b"\r\n");
                i += 1;
                at_line_start = true;
            }
            b'\r' | b'\n' => {
                out.extend_from_slice(b"\r\n");
                at_line_start = true;
            }
            _ => {
                out.push(byte);
                at_line_start = false;
            }
        }
        i += 1;
    }
    if !at_line_start {
        out.extend_from_slice(b"\r\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_stuffing_and_line_ends() {
        assert_eq!(dot_stuff(b"a\r\n.b\r\n..c\r\n.\r\n"), b"a\r\n..b\r\n...c\r\n..\r\n");
        assert_eq!(dot_stuff(b".start\nmiddle\rend"), b"..start\r\nmiddle\r\nend\r\n");
        assert_eq!(dot_stuff(b""), b"");
    }

    #[test]
    fn addresses_that_could_inject_commands_are_refused() {
        assert!(check_address("ada@example.test").is_ok());
        assert!(check_address("élodie@exemple.test").is_ok());
        for bad in [
            "",
            "no-at-sign",
            "a@b>\r\nRCPT TO:<x@y",
            "a@b\nDATA",
            "a b@c",
            "<a@b>",
            "a@b\u{0}",
        ] {
            assert!(check_address(bad).is_err(), "{bad:?}");
        }
    }

    #[tokio::test]
    async fn replies_are_read_across_lines_and_bounded() {
        let mut ok = BufReader::new(&b"250-mail.example.test\r\n250-SIZE 1000\r\n250 AUTH PLAIN LOGIN\r\n"[..]);
        let reply = read_reply(&mut ok).await.unwrap();
        assert_eq!(reply.code, 250);
        assert_eq!(reply.lines, ["mail.example.test", "SIZE 1000", "AUTH PLAIN LOGIN"]);

        for bad in [&b"hello\r\n"[..], b"999 no\r\n", b"250-a\r\n", b""] {
            assert!(read_reply(&mut BufReader::new(bad)).await.is_err());
        }
        let long = [b"250 ".to_vec(), vec![b'a'; MAX_REPLY_LINE + 10], b"\r\n".to_vec()].concat();
        assert!(read_reply(&mut BufReader::new(&long[..])).await.is_err());
    }
}
