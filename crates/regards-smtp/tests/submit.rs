//! Sending against a fake submission server: what reaches it, and how every
//! failure is classed (not sent, may retry, or uncertain).

#[path = "support/fake_smtp.rs"]
mod fake_smtp;

use fake_smtp::{Fault, Mode, PASSWORD, Pki, Script, pki, serve};
use regards_core::secret::Secret;
use regards_net::TlsOptions;
use regards_smtp::{Credentials, Envelope, Security, SmtpServer, SubmitError, submit, verify_login};

const MESSAGE: &[u8] =
    b"From: Ada <ada@example.test>\r\nTo: bob@example.test\r\nSubject: Hi\r\n\r\nHello\r\n.\r\n..two\r\n.hidden\r\nend";

fn trusting(pki: &Pki) -> TlsOptions {
    TlsOptions::platform()
        .with_extra_roots_pem(pki.ca_pem.as_bytes())
        .unwrap()
}

fn server(port: u16, security: Security) -> SmtpServer {
    SmtpServer {
        host: "127.0.0.1".into(),
        port,
        security,
    }
}

fn envelope(to: &[&str]) -> Envelope {
    Envelope {
        from: "ada@example.test".into(),
        to: to.iter().map(|s| s.to_string()).collect(),
    }
}

async fn send(
    pki: &Pki,
    script: Script,
    security: Security,
    password: &str,
    to: &[&str],
) -> (Result<regards_smtp::Submitted, SubmitError>, fake_smtp::Session) {
    let (port, mut sessions) = serve(pki, script).await;
    let password = Secret::new(password);
    let credentials = Credentials {
        username: "ada",
        password: &password,
    };
    let outcome = submit(
        &server(port, security),
        &credentials,
        &envelope(to),
        MESSAGE,
        &trusting(pki),
    )
    .await;
    (outcome, sessions.next().await)
}

#[tokio::test]
async fn a_message_goes_out_over_starttls_exactly_as_written() {
    let pki = pki();
    let script = Script::new(Mode::StartTls {
        offer: true,
        inject: false,
    });
    let (outcome, session) = send(
        &pki,
        script,
        Security::StartTls,
        PASSWORD,
        &["bob@example.test", "carol@example.test"],
    )
    .await;
    assert_eq!(outcome.unwrap().reply, "2.0.0 Ok: queued as FAKE1");
    assert_eq!(session.login, Some(("ada".into(), PASSWORD.into())));
    assert!(session.tls_at_login, "the password went over TLS");
    assert_eq!(session.from.as_deref(), Some("ada@example.test"));
    assert_eq!(session.to, ["bob@example.test", "carol@example.test"]);
    // Dot-stuffing undone by the server gives back the message, with a final line end.
    let mut expected = MESSAGE.to_vec();
    expected.extend_from_slice(b"\r\n");
    assert_eq!(session.data.unwrap(), expected);
    assert!(
        session.commands.iter().position(|c| c == "STARTTLS")
            < session.commands.iter().position(|c| c.starts_with("AUTH"))
    );
}

#[tokio::test]
async fn implicit_tls_with_auth_login() {
    let pki = pki();
    let script = Script {
        auth: "LOGIN",
        ..Script::new(Mode::Tls)
    };
    let (outcome, session) = send(&pki, script, Security::Tls, PASSWORD, &["bob@example.test"]).await;
    outcome.unwrap();
    assert_eq!(session.login, Some(("ada".into(), PASSWORD.into())));
}

#[tokio::test]
async fn a_wrong_password_is_not_retried() {
    let pki = pki();
    let (outcome, session) = send(
        &pki,
        Script::new(Mode::Tls),
        Security::Tls,
        "wrong",
        &["bob@example.test"],
    )
    .await;
    match outcome.unwrap_err() {
        SubmitError::NotSent { reason, retry } => {
            assert!(!retry);
            assert!(reason.contains("user name or password"), "{reason}");
            assert!(!reason.contains("wrong"), "the password is never in an error");
        }
        other @ SubmitError::Uncertain(_) => panic!("{other:?}"),
    }
    assert!(session.data.is_none());
}

#[tokio::test]
async fn a_refused_recipient_stops_the_message_before_data() {
    let pki = pki();
    let script = Script {
        reject: &["nobody@example.test"],
        ..Script::new(Mode::Tls)
    };
    let (outcome, session) = send(
        &pki,
        script,
        Security::Tls,
        PASSWORD,
        &["bob@example.test", "nobody@example.test"],
    )
    .await;
    match outcome.unwrap_err() {
        SubmitError::NotSent { reason, retry } => {
            assert!(!retry);
            assert!(reason.contains("nobody@example.test"), "{reason}");
        }
        other @ SubmitError::Uncertain(_) => panic!("{other:?}"),
    }
    assert!(session.data.is_none(), "nothing was sent to anyone");
}

#[tokio::test]
async fn a_temporary_refusal_after_the_message_may_be_retried() {
    let pki = pki();
    let script = Script {
        fault: Fault::ReplyAfterData("451 4.3.0 try again later"),
        ..Script::new(Mode::Tls)
    };
    let (outcome, _) = send(&pki, script, Security::Tls, PASSWORD, &["bob@example.test"]).await;
    assert!(matches!(outcome.unwrap_err(), SubmitError::NotSent { retry: true, .. }));

    let script = Script {
        fault: Fault::ReplyAfterData("554 5.7.1 spam"),
        ..Script::new(Mode::Tls)
    };
    let (outcome, _) = send(&pki, script, Security::Tls, PASSWORD, &["bob@example.test"]).await;
    assert!(matches!(
        outcome.unwrap_err(),
        SubmitError::NotSent { retry: false, .. }
    ));
}

#[tokio::test]
async fn losing_the_connection_after_the_message_is_uncertain() {
    let pki = pki();
    let script = Script {
        fault: Fault::DropAfterData,
        ..Script::new(Mode::Tls)
    };
    let (outcome, session) = send(&pki, script, Security::Tls, PASSWORD, &["bob@example.test"]).await;
    assert!(matches!(outcome.unwrap_err(), SubmitError::Uncertain(_)));
    assert!(session.data.is_some(), "the server had the whole message");
}

#[tokio::test]
async fn losing_the_connection_before_the_message_is_not_sent() {
    let pki = pki();
    let script = Script {
        fault: Fault::DropAtRecipient,
        ..Script::new(Mode::Tls)
    };
    let (outcome, _) = send(&pki, script, Security::Tls, PASSWORD, &["bob@example.test"]).await;
    assert!(matches!(outcome.unwrap_err(), SubmitError::NotSent { retry: true, .. }));
}

#[tokio::test]
async fn without_starttls_no_password_is_sent() {
    let pki = pki();
    let script = Script::new(Mode::StartTls {
        offer: false,
        inject: false,
    });
    let (outcome, session) = send(&pki, script, Security::StartTls, PASSWORD, &["bob@example.test"]).await;
    assert!(matches!(
        outcome.unwrap_err(),
        SubmitError::NotSent { retry: false, .. }
    ));
    assert!(session.login.is_none());
    assert!(!session.commands.iter().any(|c| c.starts_with("AUTH")));
}

#[tokio::test]
async fn data_injected_before_the_tls_handshake_is_refused() {
    let pki = pki();
    let script = Script::new(Mode::StartTls {
        offer: true,
        inject: true,
    });
    let (outcome, session) = send(&pki, script, Security::StartTls, PASSWORD, &["bob@example.test"]).await;
    assert!(matches!(
        outcome.unwrap_err(),
        SubmitError::NotSent { retry: false, .. }
    ));
    assert!(session.login.is_none(), "no password after a tampered STARTTLS");
}

#[tokio::test]
async fn a_message_over_the_size_limit_is_not_sent() {
    let pki = pki();
    let script = Script {
        size: Some(20),
        ..Script::new(Mode::Tls)
    };
    let (outcome, session) = send(&pki, script, Security::Tls, PASSWORD, &["bob@example.test"]).await;
    assert!(matches!(
        outcome.unwrap_err(),
        SubmitError::NotSent { retry: false, .. }
    ));
    assert!(session.from.is_none(), "refused before MAIL FROM");
}

#[tokio::test]
async fn addresses_that_could_inject_commands_never_reach_the_server() {
    let pki = pki();
    let (port, _) = serve(&pki, Script::new(Mode::Tls)).await;
    let password = Secret::new(PASSWORD);
    let outcome = submit(
        &server(port, Security::Tls),
        &Credentials {
            username: "ada",
            password: &password,
        },
        &envelope(&["bob@example.test>\r\nRCPT TO:<eve@example.test"]),
        MESSAGE,
        &trusting(&pki),
    )
    .await;
    assert!(matches!(
        outcome.unwrap_err(),
        SubmitError::NotSent { retry: false, .. }
    ));
}

#[tokio::test]
async fn nothing_listening_or_an_untrusted_certificate() {
    let port = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let password = Secret::new(PASSWORD);
    let credentials = Credentials {
        username: "ada",
        password: &password,
    };
    let offline = verify_login(&server(port, Security::Tls), &credentials, &TlsOptions::platform()).await;
    assert!(matches!(offline.unwrap_err(), SubmitError::NotSent { retry: true, .. }));

    let pki = pki();
    let (port, _) = serve(&pki, Script::new(Mode::Tls)).await;
    let untrusted = verify_login(&server(port, Security::Tls), &credentials, &TlsOptions::platform()).await;
    assert!(matches!(
        untrusted.unwrap_err(),
        SubmitError::NotSent { retry: false, .. }
    ));

    verify_login(&server(port, Security::Tls), &credentials, &trusting(&pki))
        .await
        .unwrap();
}
