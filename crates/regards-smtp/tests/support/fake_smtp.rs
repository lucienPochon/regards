//! A fake SMTP submission server with a throwaway CA, for tests. It records
//! what each connection did and can misbehave on purpose.
// Shared by the SMTP and daemon tests; each uses only part of it.
#![allow(dead_code)]

use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, Issuer, KeyPair, KeyUsagePurpose};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::rustls::crypto::ring;
use tokio_rustls::rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};

pub const PASSWORD: &str = "correct horse";

pub struct Pki {
    pub ca_pem: String,
    server: Arc<ServerConfig>,
}

impl Pki {
    /// TLS as this certificate's server, for other fakes.
    pub fn acceptor(&self) -> TlsAcceptor {
        TlsAcceptor::from(Arc::clone(&self.server))
    }
}

/// A fresh CA and a certificate for 127.0.0.1.
pub fn pki() -> Pki {
    let ca_key = KeyPair::generate().unwrap();
    let mut ca = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::DigitalSignature];
    ca.distinguished_name.push(DnType::CommonName, "Regards Test CA");
    let ca_cert = ca.self_signed(&ca_key).unwrap();
    let issuer = Issuer::from_params(&ca, &ca_key);
    let leaf_key = KeyPair::generate().unwrap();
    let leaf = CertificateParams::new(vec!["127.0.0.1".to_owned()])
        .unwrap()
        .signed_by(&leaf_key, &issuer)
        .unwrap();
    let server = ServerConfig::builder_with_provider(Arc::new(ring::default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![leaf.der().clone()],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf_key.serialize_der())),
        )
        .unwrap();
    Pki {
        ca_pem: ca_cert.pem(),
        server: Arc::new(server),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// TLS from the first byte.
    Tls,
    /// Plain, then STARTTLS (if offered).
    StartTls { offer: bool, inject: bool },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    None,
    /// Read the whole message, then hang up without a reply.
    DropAfterData,
    /// Answer the end of the message with this line.
    ReplyAfterData(&'static str),
    /// Hang up when the first recipient is given.
    DropAtRecipient,
}

#[derive(Clone, Copy)]
pub struct Script {
    pub mode: Mode,
    /// AUTH mechanisms to offer, for example "PLAIN LOGIN".
    pub auth: &'static str,
    pub size: Option<u64>,
    /// Recipients refused with 550.
    pub reject: &'static [&'static str],
    pub fault: Fault,
    /// The only password sign-in accepts.
    pub password: &'static str,
}

impl Script {
    pub const fn new(mode: Mode) -> Self {
        Self {
            mode,
            password: PASSWORD,
            auth: "PLAIN LOGIN",
            size: None,
            reject: &[],
            fault: Fault::None,
        }
    }
}

/// What one connection did.
#[derive(Debug, Default, Clone)]
pub struct Session {
    /// Every command line, with credentials replaced by "<secret>".
    pub commands: Vec<String>,
    /// The user name and password of a sign-in, if one was tried.
    pub login: Option<(String, String)>,
    pub from: Option<String>,
    pub to: Vec<String>,
    /// The message as the client meant it (dot-stuffing removed).
    pub data: Option<Vec<u8>>,
    /// Whether the line the password came on was encrypted.
    pub tls_at_login: bool,
}

pub struct Sessions(mpsc::UnboundedReceiver<Session>);

impl Sessions {
    pub async fn next(&mut self) -> Session {
        self.0.recv().await.expect("the fake server is gone")
    }

    /// A finished session, if there is one already.
    pub fn try_next(&mut self) -> Option<Session> {
        self.0.try_recv().ok()
    }
}

pub async fn serve(pki: &Pki, script: Script) -> (u16, Sessions) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let acceptor = TlsAcceptor::from(Arc::clone(&pki.server));
    let (sent, received) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let (acceptor, sent) = (acceptor.clone(), sent.clone());
            tokio::spawn(async move {
                let mut session = Session::default();
                connection(tcp, acceptor, script, &mut session).await;
                let _ = sent.send(session);
            });
        }
    });
    (port, Sessions(received))
}

async fn connection(tcp: TcpStream, acceptor: TlsAcceptor, script: Script, session: &mut Session) {
    match script.mode {
        Mode::Tls => {
            let Ok(tls) = acceptor.accept(tcp).await else { return };
            let mut stream = BufReader::new(tls);
            if stream.write_all(b"220 fake ESMTP\r\n").await.is_ok() {
                commands(&mut stream, script, session, true).await;
            }
        }
        Mode::StartTls { offer, inject } => {
            let mut plain = BufReader::new(tcp);
            if plain.write_all(b"220 fake ESMTP\r\n").await.is_err() {
                return;
            }
            loop {
                let Some(line) = read_line(&mut plain).await else {
                    return;
                };
                session.commands.push(redact(&line));
                let verb = line.split_whitespace().next().unwrap_or("").to_ascii_uppercase();
                let reply: &[u8] = match verb.as_str() {
                    "EHLO" if offer => b"250-fake\r\n250-STARTTLS\r\n250 AUTH PLAIN LOGIN\r\n",
                    "EHLO" => b"250-fake\r\n250 AUTH PLAIN LOGIN\r\n",
                    "STARTTLS" if offer => {
                        let reply: &[u8] = if inject {
                            b"220 go ahead\r\n250 injected\r\n"
                        } else {
                            b"220 go ahead\r\n"
                        };
                        if plain.write_all(reply).await.is_err() {
                            return;
                        }
                        break;
                    }
                    // Accepting a password in the clear would be the bug under test.
                    "AUTH" => {
                        session.login = Some(("<clear>".into(), "<clear>".into()));
                        b"535 not over plain text\r\n"
                    }
                    "QUIT" => {
                        let _ = plain.write_all(b"221 bye\r\n").await;
                        return;
                    }
                    _ => b"530 STARTTLS first\r\n",
                };
                if plain.write_all(reply).await.is_err() {
                    return;
                }
            }
            let Ok(tls) = acceptor.accept(plain.into_inner()).await else {
                return;
            };
            let mut stream = BufReader::new(tls);
            commands(&mut stream, script, session, true).await;
        }
    }
}

async fn commands<S>(stream: &mut BufReader<S>, script: Script, session: &mut Session, tls: bool)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    while let Some(line) = read_line(stream).await {
        session.commands.push(redact(&line));
        let mut words = line.splitn(3, ' ');
        let verb = words.next().unwrap_or("").to_ascii_uppercase();
        let reply = match verb.as_str() {
            "EHLO" => {
                let size = script.size.map(|s| format!("250-SIZE {s}\r\n")).unwrap_or_default();
                format!("250-fake\r\n{size}250 AUTH {}\r\n", script.auth)
            }
            "AUTH" => match words.next().map(str::to_ascii_uppercase).as_deref() {
                Some("PLAIN") => {
                    let token = decode(words.next().unwrap_or(""));
                    let parts: Vec<&[u8]> = token.split(|b| *b == 0).collect();
                    let (user, pass) = match parts.as_slice() {
                        [_, user, pass] => (lossy(user), lossy(pass)),
                        _ => (String::new(), String::new()),
                    };
                    login(session, user, pass, tls, script.password)
                }
                Some("LOGIN") => {
                    if stream.write_all(b"334 VXNlcm5hbWU6\r\n").await.is_err() {
                        return;
                    }
                    let Some(user) = read_line(stream).await else { return };
                    if stream.write_all(b"334 UGFzc3dvcmQ6\r\n").await.is_err() {
                        return;
                    }
                    let Some(pass) = read_line(stream).await else { return };
                    login(
                        session,
                        lossy(&decode(&user)),
                        lossy(&decode(&pass)),
                        tls,
                        script.password,
                    )
                }
                // OAuth: the token is the script's password.
                Some("XOAUTH2") => {
                    let sasl = lossy(&decode(words.next().unwrap_or("")));
                    let user = sasl
                        .split('\u{1}')
                        .find_map(|part| part.strip_prefix("user="))
                        .unwrap_or_default()
                        .to_owned();
                    let token = sasl
                        .split('\u{1}')
                        .find_map(|part| part.strip_prefix("auth=Bearer "))
                        .unwrap_or_default()
                        .to_owned();
                    if token == script.password {
                        login(session, user, token, tls, script.password)
                    } else {
                        // The error as a challenge; an empty line ends it (RFC 7628 §3.2.3).
                        let error = regards_core::encoding::base64(br#"{"status":"401","schemes":"bearer"}"#);
                        if stream.write_all(format!("334 {error}\r\n").as_bytes()).await.is_err() {
                            return;
                        }
                        let Some(_) = read_line(stream).await else { return };
                        session.login = Some((user, token));
                        session.tls_at_login = tls;
                        "535 5.7.3 Authentication unsuccessful\r\n".into()
                    }
                }
                _ => "504 unknown mechanism\r\n".into(),
            },
            "MAIL" => {
                session.from = address(&line);
                "250 ok\r\n".into()
            }
            "RCPT" => {
                if script.fault == Fault::DropAtRecipient {
                    return;
                }
                let to = address(&line).unwrap_or_default();
                if script.reject.contains(&to.as_str()) {
                    format!("550 5.1.1 <{to}> no such user\r\n")
                } else {
                    session.to.push(to);
                    "250 ok\r\n".into()
                }
            }
            "DATA" => {
                if stream.write_all(b"354 go ahead\r\n").await.is_err() {
                    return;
                }
                let mut data = Vec::new();
                loop {
                    let mut raw = Vec::new();
                    if stream.read_until(b'\n', &mut raw).await.unwrap_or(0) == 0 {
                        return;
                    }
                    if raw == b".\r\n" {
                        break;
                    }
                    let line = raw.strip_prefix(b".").unwrap_or(&raw);
                    data.extend_from_slice(line);
                }
                session.data = Some(data);
                match script.fault {
                    Fault::DropAfterData => return,
                    Fault::ReplyAfterData(reply) => format!("{reply}\r\n"),
                    _ => "250 2.0.0 Ok: queued as FAKE1\r\n".into(),
                }
            }
            "RSET" => "250 ok\r\n".into(),
            "QUIT" => {
                let _ = stream.write_all(b"221 bye\r\n").await;
                return;
            }
            _ => "502 unknown\r\n".into(),
        };
        if stream.write_all(reply.as_bytes()).await.is_err() {
            return;
        }
    }
}

fn login(session: &mut Session, user: String, pass: String, tls: bool, password: &str) -> String {
    let ok = pass == password;
    session.login = Some((user, pass));
    session.tls_at_login = tls;
    if ok {
        "235 2.7.0 accepted\r\n".into()
    } else {
        "535 5.7.8 bad credentials\r\n".into()
    }
}

async fn read_line<R: tokio::io::AsyncBufRead + Unpin>(stream: &mut R) -> Option<String> {
    let mut line = Vec::new();
    if stream.read_until(b'\n', &mut line).await.ok()? == 0 {
        return None;
    }
    Some(String::from_utf8_lossy(&line).trim_end_matches(['\r', '\n']).to_owned())
}

fn redact(line: &str) -> String {
    if line.to_ascii_uppercase().starts_with("AUTH PLAIN ") {
        "AUTH PLAIN <secret>".into()
    } else if line.to_ascii_uppercase().starts_with("AUTH XOAUTH2 ") {
        "AUTH XOAUTH2 <secret>".into()
    } else {
        line.to_owned()
    }
}

fn address(line: &str) -> Option<String> {
    let start = line.find('<')? + 1;
    let end = line[start..].find('>')? + start;
    Some(line[start..end].to_owned())
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn decode(text: &str) -> Vec<u8> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut bits = 0u32;
    let mut count = 0;
    let mut out = Vec::new();
    for byte in text.bytes() {
        let Some(value) = ALPHABET.iter().position(|&a| a == byte) else {
            continue;
        };
        bits = (bits << 6) | value as u32;
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
        }
    }
    out
}
