//! A fake TLS IMAP server with a throwaway CA, shared by the `regards-imap`
//! and `regards-daemon` tests (included with `#[path]`).
#![allow(dead_code)]

use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair, KeyUsagePurpose,
};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::rustls::crypto::ring;
use tokio_rustls::rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};

/// The password the fake server accepts unless a test says otherwise.
pub const PASSWORD: &str = "correct horse";

/// A CA and a server certificate it issued.
pub struct Pki {
    pub ca_pem: String,
    server: Arc<ServerConfig>,
}

/// A fresh CA and a server certificate for `names`.
pub fn pki(names: &[&str]) -> Pki {
    let ca_key = KeyPair::generate().unwrap();
    let mut ca = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    ca.distinguished_name.push(DnType::CommonName, "Regards Test CA");
    let ca_cert = ca.self_signed(&ca_key).unwrap();
    let issuer = Issuer::from_params(&ca, &ca_key);

    let leaf_key = KeyPair::generate().unwrap();
    let mut leaf = CertificateParams::new(names.iter().map(|n| n.to_string()).collect::<Vec<_>>()).unwrap();
    leaf.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let leaf_cert = leaf.signed_by(&leaf_key, &issuer).unwrap();

    let server = ServerConfig::builder_with_provider(Arc::new(ring::default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![leaf_cert.der().clone()],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf_key.serialize_der())),
        )
        .unwrap();
    Pki {
        ca_pem: ca_cert.pem(),
        server: Arc::new(server),
    }
}

/// A Dovecot-like folder list: SPECIAL-USE on some folders, a non-ASCII name in
/// modified UTF-7, and an unselectable parent.
pub const DEFAULT_LIST: &str = concat!(
    r#"* LIST (\HasNoChildren) "/" INBOX"#,
    "\r\n",
    r#"* LIST (\HasNoChildren \Sent) "/" Sent"#,
    "\r\n",
    r#"* LIST (\HasNoChildren \Drafts) "/" Drafts"#,
    "\r\n",
    r#"* LIST (\HasNoChildren \Trash) "/" Trash"#,
    "\r\n",
    r#"* LIST (\HasNoChildren) "/" Spam"#,
    "\r\n",
    r#"* LIST (\Noselect \HasChildren) "/" Projects"#,
    "\r\n",
    r#"* LIST (\HasNoChildren) "/" "Projects/R&AOk-sum&AOk-""#,
    "\r\n",
);

/// How the fake server behaves.
#[derive(Clone, Copy)]
pub struct Script {
    pub greeting: &'static str,
    /// The only password LOGIN accepts.
    pub password: &'static str,
    /// Untagged lines sent in reply to LIST.
    pub list: &'static str,
}

impl Script {
    pub const fn new(password: &'static str) -> Self {
        Self {
            greeting: "* OK ready\r\n",
            password,
            list: DEFAULT_LIST,
        }
    }
}

/// What each finished connection sent as its password (`None`: no LOGIN, for
/// example after a TLS failure), in order.
pub struct Logins(mpsc::UnboundedReceiver<Option<String>>);

impl Logins {
    pub async fn next(&mut self) -> Option<String> {
        self.0.recv().await.expect("the fake server is gone")
    }
}

/// [`serve`] with the default folders.
pub async fn fake_server(pki: &Pki, greeting: &'static str, password: &'static str) -> (u16, Logins) {
    serve(
        pki,
        Script {
            greeting,
            ..Script::new(password)
        },
    )
    .await
}

/// Serve connections on 127.0.0.1 following `script`, until the test ends.
pub async fn serve(pki: &Pki, script: Script) -> (u16, Logins) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let acceptor = TlsAcceptor::from(Arc::clone(&pki.server));
    let (sent, received) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            let sent = sent.clone();
            tokio::spawn(async move {
                let given = match acceptor.accept(tcp).await {
                    Ok(tls) => session(tls, script).await,
                    Err(_) => None,
                };
                let _ = sent.send(given);
            });
        }
    });
    (port, Logins(received))
}

async fn session<S>(tls: S, script: Script) -> Option<String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (read, mut write) = tokio::io::split(tls);
    let mut read = BufReader::new(read);
    let mut given = None;
    if write.write_all(script.greeting.as_bytes()).await.is_err() {
        return given;
    }
    while let Some(command) = read_command(&mut read, &mut write).await {
        let args = parse_words(&command);
        let tag = args[0].clone();
        let reply = match args.get(1).map(|v| v.to_ascii_uppercase()).as_deref() {
            Some("CAPABILITY") => format!("* CAPABILITY IMAP4rev1 AUTH=PLAIN\r\n{tag} OK done\r\n"),
            Some("LOGIN") => {
                let attempt = args.get(3).cloned().unwrap_or_default();
                let ok = attempt == script.password;
                given = Some(attempt);
                if ok {
                    format!("{tag} OK [CAPABILITY IMAP4rev1 IDLE MOVE UIDPLUS SPECIAL-USE] Logged in\r\n")
                } else {
                    format!("{tag} NO [AUTHENTICATIONFAILED] Authentication failed.\r\n")
                }
            }
            Some("LIST") => format!("{}{tag} OK List completed\r\n", script.list),
            Some("LOGOUT") => {
                let _ = write
                    .write_all(format!("* BYE Logging out\r\n{tag} OK Logout completed\r\n").as_bytes())
                    .await;
                break;
            }
            _ => format!("{tag} BAD unexpected\r\n"),
        };
        if write.write_all(reply.as_bytes()).await.is_err() {
            break;
        }
    }
    given
}

/// Read one command line, following `{n}` literals (sending a continuation for synchronising ones).
async fn read_command<R, W>(read: &mut BufReader<R>, write: &mut W) -> Option<Vec<u8>>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut command = Vec::new();
    loop {
        let mut line = Vec::new();
        if read.read_until(b'\n', &mut line).await.ok()? == 0 {
            return None;
        }
        command.extend_from_slice(&line);
        let trimmed = line.strip_suffix(b"\r\n").unwrap_or(&line);
        let Some(open) = trimmed
            .ends_with(b"}")
            .then(|| trimmed.iter().rposition(|&b| b == b'{'))
            .flatten()
        else {
            return Some(command);
        };
        let spec = std::str::from_utf8(&trimmed[open + 1..trimmed.len() - 1]).ok()?;
        let (count, sync) = match spec.strip_suffix('+') {
            Some(count) => (count, false),
            None => (spec, true),
        };
        if sync {
            write.write_all(b"+ go ahead\r\n").await.ok()?;
        }
        let mut literal = vec![0; count.parse().ok()?];
        read.read_exact(&mut literal).await.ok()?;
        command.extend_from_slice(&literal);
    }
}

/// Split an IMAP command into atoms, quoted strings and literals, unescaped.
fn parse_words(command: &[u8]) -> Vec<String> {
    let mut words = Vec::new();
    let mut i = 0;
    while i < command.len() {
        match command[i] {
            b' ' | b'\r' | b'\n' => i += 1,
            b'"' => {
                let mut word = Vec::new();
                i += 1;
                while i < command.len() && command[i] != b'"' {
                    if command[i] == b'\\' {
                        i += 1;
                    }
                    word.push(command[i]);
                    i += 1;
                }
                i += 1;
                words.push(String::from_utf8_lossy(&word).into_owned());
            }
            b'{' => {
                let close = i + command[i..].iter().position(|&b| b == b'}').unwrap();
                let count: usize = std::str::from_utf8(&command[i + 1..close])
                    .unwrap()
                    .trim_end_matches('+')
                    .parse()
                    .unwrap();
                let start = close + 3; // "}\r\n"
                words.push(String::from_utf8_lossy(&command[start..start + count]).into_owned());
                i = start + count;
            }
            _ => {
                let end = i + command[i..]
                    .iter()
                    .position(|&b| b == b' ' || b == b'\r')
                    .unwrap_or(command.len() - i);
                words.push(String::from_utf8_lossy(&command[i..end]).into_owned());
                i = end;
            }
        }
    }
    words
}
