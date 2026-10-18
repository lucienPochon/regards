//! Sign-in checks against a fake TLS IMAP server with a throwaway CA.

#[path = "support/fake_imap.rs"]
mod fake_imap;

use fake_imap::{PASSWORD, Pki, fake_server, pki};
use regards_core::secret::Secret;
use regards_imap::{ImapError, ImapServer, TlsOptions, verify_login};
use tokio::net::TcpListener;

fn trusting(pki: &Pki) -> TlsOptions {
    TlsOptions::platform()
        .with_extra_roots_pem(pki.ca_pem.as_bytes())
        .unwrap()
}

fn localhost(port: u16) -> ImapServer {
    ImapServer {
        host: "localhost".into(),
        port,
    }
}

#[tokio::test]
async fn signs_in_and_reports_the_post_login_capabilities() {
    let pki = pki(&["localhost"]);
    let (port, password) = fake_server(&pki, "* OK ready\r\n", PASSWORD).await;
    let check = verify_login(&localhost(port), "ada", &Secret::new(PASSWORD), &trusting(&pki))
        .await
        .unwrap();
    assert_eq!(check.capabilities, ["IMAP4REV1", "IDLE", "MOVE", "UIDPLUS"]);
    assert_eq!(password.await.unwrap().as_deref(), Some(PASSWORD));
}

#[tokio::test]
async fn a_wrong_password_is_reported_with_the_server_text() {
    let pki = pki(&["localhost"]);
    let (port, _) = fake_server(&pki, "* OK [CAPABILITY IMAP4rev1 AUTH=PLAIN] ready\r\n", PASSWORD).await;
    let error = verify_login(&localhost(port), "ada", &Secret::new("wrong"), &trusting(&pki))
        .await
        .unwrap_err();
    assert_eq!(error, ImapError::AuthFailed("Authentication failed.".into()));
}

#[tokio::test]
async fn passwords_with_quotes_backslashes_and_accents_arrive_intact() {
    let pki = pki(&["localhost"]);
    let tricky = "p\"a\\ss wörd 🙂";
    let (port, received) = fake_server(&pki, "* OK ready\r\n", tricky).await;
    verify_login(&localhost(port), "ada", &Secret::new(tricky), &trusting(&pki))
        .await
        .unwrap();
    assert_eq!(received.await.unwrap().as_deref(), Some(tricky));
}

#[tokio::test]
async fn a_certificate_from_an_unknown_authority_is_refused() {
    let pki = pki(&["localhost"]);
    let (port, password) = fake_server(&pki, "* OK ready\r\n", PASSWORD).await;
    let error = verify_login(&localhost(port), "ada", &Secret::new(PASSWORD), &TlsOptions::platform())
        .await
        .unwrap_err();
    assert!(matches!(error, ImapError::Tls(_)), "got {error:?}");
    assert_eq!(password.await.unwrap(), None, "no password may be sent");
}

#[tokio::test]
async fn a_certificate_for_another_host_is_refused() {
    let pki = pki(&["mail.other.test"]);
    let (port, password) = fake_server(&pki, "* OK ready\r\n", PASSWORD).await;
    let error = verify_login(&localhost(port), "ada", &Secret::new(PASSWORD), &trusting(&pki))
        .await
        .unwrap_err();
    assert!(matches!(error, ImapError::Tls(_)), "got {error:?}");
    assert_eq!(password.await.unwrap(), None);
}

#[tokio::test]
async fn login_disabled_is_reported_without_sending_the_password() {
    let pki = pki(&["localhost"]);
    let (port, password) = fake_server(&pki, "* OK [CAPABILITY IMAP4rev1 LOGINDISABLED] ready\r\n", PASSWORD).await;
    let error = verify_login(&localhost(port), "ada", &Secret::new(PASSWORD), &trusting(&pki))
        .await
        .unwrap_err();
    assert_eq!(error, ImapError::LoginDisabled);
    drop(password);
}

#[tokio::test]
async fn a_server_that_is_not_imap_or_says_bye_is_reported() {
    let pki = pki(&["localhost"]);
    for greeting in ["HTTP/1.1 400 Bad Request\r\n\r\n", "* BYE Too many connections\r\n"] {
        let (port, _) = fake_server(&pki, greeting, PASSWORD).await;
        let error = verify_login(&localhost(port), "ada", &Secret::new(PASSWORD), &trusting(&pki))
            .await
            .unwrap_err();
        assert!(matches!(error, ImapError::NotImap(_)), "{greeting:?} gave {error:?}");
    }
}

#[tokio::test]
async fn nothing_listening_is_a_connection_error() {
    let port = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let error = verify_login(&localhost(port), "ada", &Secret::new(PASSWORD), &TlsOptions::platform())
        .await
        .unwrap_err();
    assert!(matches!(error, ImapError::Connect(_)), "got {error:?}");
}

/// Against a real server, for example the Dovecot container from the spike:
/// `REGARDS_TEST_IMAP=localhost:31993 REGARDS_TEST_IMAP_CA=ca.pem cargo test -p regards-imap -- --ignored`
#[tokio::test]
#[ignore = "needs a real IMAP server (REGARDS_TEST_IMAP, REGARDS_TEST_IMAP_CA)"]
async fn live_server_sign_in() {
    let address = std::env::var("REGARDS_TEST_IMAP").unwrap();
    let (host, port) = address.rsplit_once(':').unwrap();
    let ca = std::fs::read(std::env::var("REGARDS_TEST_IMAP_CA").unwrap()).unwrap();
    let tls = TlsOptions::platform().with_extra_roots_pem(&ca).unwrap();
    let server = ImapServer {
        host: host.into(),
        port: port.parse().unwrap(),
    };
    let user = std::env::var("REGARDS_TEST_IMAP_USER").unwrap_or_else(|_| "regards-test".into());
    let password = Secret::new(std::env::var("REGARDS_TEST_IMAP_PASSWORD").unwrap_or_else(|_| "pass".into()));
    let check = verify_login(&server, &user, &password, &tls).await.unwrap();
    assert!(check.capabilities.iter().any(|c| c == "IMAP4REV1"));
    let error = verify_login(&server, &user, &Secret::new("definitely wrong"), &tls)
        .await
        .unwrap_err();
    assert!(matches!(error, ImapError::AuthFailed(_)), "got {error:?}");
}
