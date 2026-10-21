//! The connection log against a fake IMAP server: commands and statuses
//! go in; passwords, tokens and user names never do. In a test binary of
//! its own, since the log is one per process.

#[path = "support/fake_imap.rs"]
mod fake_imap;

use fake_imap::{PASSWORD, pki};
use regards_core::secret::Secret;
use regards_imap::{Auth, ImapServer, Session, TlsOptions, verify_login};
use regards_net::connlog;

#[tokio::test]
async fn the_log_holds_the_conversation_but_no_secret() {
    let pki = pki(&["localhost"]);
    let (port, _) = fake_imap::fake_server(&pki, "* OK ready\r\n", PASSWORD).await;
    let server = ImapServer {
        host: "localhost".into(),
        port,
    };
    let tls = TlsOptions::platform()
        .with_extra_roots_pem(pki.ca_pem.as_bytes())
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("logs").join("connections.log");
    connlog::set(Some(path.clone())).unwrap();

    verify_login(&server, "ada@example.test", &Secret::new(PASSWORD), &tls)
        .await
        .unwrap();
    let token = Secret::new("a-bearer-token");
    let _ = Session::sign_in(&server, "ada@example.test", Auth::Bearer(&token), &tls).await;
    connlog::set(None).unwrap();

    let log = std::fs::read_to_string(&path).unwrap();
    assert!(
        log.contains("[imap#") && log.contains(" localhost] -- connected"),
        "{log}"
    );
    assert!(log.contains("] C: ") && log.contains(" CAPABILITY"), "{log}");
    assert!(log.contains(" LOGIN [redacted]"), "{log}");
    assert!(log.contains(" AUTHENTICATE XOAUTH2 [redacted]"), "{log}");
    assert!(log.contains("] S: ") && log.contains(" OK"), "{log}");
    for secret in [PASSWORD, "a-bearer-token", "ada@example.test"] {
        assert!(!log.contains(secret), "{secret:?} is in the log:\n{log}");
    }
}
