//! IDLE only reports what changes after it starts. Changes the server reports
//! in reply to other commands (mail that came in during a sync) must not wait
//! for the next change to be noticed.

#[path = "support/fake_imap.rs"]
mod fake_imap;

use fake_imap::{PASSWORD, Pki, Script, pki, serve};
use regards_core::secret::Secret;
use regards_imap::{IdleEnd, ImapServer, Session, TlsOptions};
use std::time::Duration;

fn trusting(pki: &Pki) -> TlsOptions {
    TlsOptions::platform()
        .with_extra_roots_pem(pki.ca_pem.as_bytes())
        .unwrap()
}

async fn signed_in(pki: &Pki, script: Script) -> Session {
    let (port, _) = serve(pki, script).await;
    let server = ImapServer {
        host: "127.0.0.1".into(),
        port,
    };
    Session::login(&server, "ada", &Secret::new(PASSWORD), &trusting(pki))
        .await
        .unwrap()
}

/// IDLE until the server reports a change, or give up after 200 ms.
async fn idle_briefly(session: &mut Session) -> IdleEnd {
    tokio::time::timeout(
        Duration::from_secs(5),
        session.idle(Duration::from_secs(60), tokio::time::sleep(Duration::from_millis(200))),
    )
    .await
    .expect("IDLE came back")
    .unwrap()
}

#[tokio::test]
async fn idle_waits_when_nothing_changed() {
    let pki = pki(&["127.0.0.1"]);
    let mut session = signed_in(&pki, Script::new(PASSWORD)).await;
    session.select(b"INBOX").await.unwrap();
    session.uid_search_all().await.unwrap();
    assert_eq!(idle_briefly(&mut session).await, IdleEnd::Stopped);
}

#[tokio::test]
async fn mail_reported_during_another_command_ends_the_next_idle_at_once() {
    let pki = pki(&["127.0.0.1"]);
    let script = Script {
        meanwhile: "* 1 EXISTS\r\n",
        ..Script::new(PASSWORD)
    };
    let mut session = signed_in(&pki, script).await;
    let selected = session.select(b"INBOX").await.unwrap();
    assert_eq!(selected.exists, 0);
    session.uid_search_all().await.unwrap();
    assert_eq!(idle_briefly(&mut session).await, IdleEnd::Changed);

    // Told once: the same count again is not news (some servers repeat it).
    session.uid_search_all().await.unwrap();
    assert_eq!(idle_briefly(&mut session).await, IdleEnd::Stopped);

    // Selecting again starts afresh: the mailbox's count comes from SELECT.
    session.select(b"INBOX").await.unwrap();
    assert_eq!(idle_briefly(&mut session).await, IdleEnd::Stopped);
}

#[tokio::test]
async fn changes_sent_before_idle_is_accepted_end_it() {
    let pki = pki(&["127.0.0.1"]);
    let script = Script {
        before_idling: "* 1 EXISTS\r\n",
        ..Script::new(PASSWORD)
    };
    let mut session = signed_in(&pki, script).await;
    session.select(b"INBOX").await.unwrap();
    assert_eq!(idle_briefly(&mut session).await, IdleEnd::Changed);
    session
        .uid_search_all()
        .await
        .expect("the session still works after DONE");
}
