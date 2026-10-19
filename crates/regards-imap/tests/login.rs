//! Sign-in checks against a fake TLS IMAP server with a throwaway CA.

#[path = "support/fake_imap.rs"]
mod fake_imap;

use fake_imap::{PASSWORD, Pki, fake_server, pki};
use regards_core::secret::Secret;
use regards_imap::{ImapError, ImapServer, MessageSet, Session, Since, SystemFlag, TlsOptions, verify_login};
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
    let (port, mut password) = fake_server(&pki, "* OK ready\r\n", PASSWORD).await;
    let check = verify_login(&localhost(port), "ada", &Secret::new(PASSWORD), &trusting(&pki))
        .await
        .unwrap();
    assert_eq!(
        check.capabilities,
        ["IMAP4REV1", "IDLE", "MOVE", "UIDPLUS", "SPECIAL-USE"]
    );
    assert_eq!(password.next().await.as_deref(), Some(PASSWORD));
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
    let (port, mut received) = fake_server(&pki, "* OK ready\r\n", tricky).await;
    verify_login(&localhost(port), "ada", &Secret::new(tricky), &trusting(&pki))
        .await
        .unwrap();
    assert_eq!(received.next().await.as_deref(), Some(tricky));
}

#[tokio::test]
async fn a_certificate_from_an_unknown_authority_is_refused() {
    let pki = pki(&["localhost"]);
    let (port, mut password) = fake_server(&pki, "* OK ready\r\n", PASSWORD).await;
    let error = verify_login(&localhost(port), "ada", &Secret::new(PASSWORD), &TlsOptions::platform())
        .await
        .unwrap_err();
    assert!(matches!(error, ImapError::Tls(_)), "got {error:?}");
    assert_eq!(password.next().await, None, "no password may be sent");
}

#[tokio::test]
async fn a_certificate_for_another_host_is_refused() {
    let pki = pki(&["mail.other.test"]);
    let (port, mut password) = fake_server(&pki, "* OK ready\r\n", PASSWORD).await;
    let error = verify_login(&localhost(port), "ada", &Secret::new(PASSWORD), &trusting(&pki))
        .await
        .unwrap_err();
    assert!(matches!(error, ImapError::Tls(_)), "got {error:?}");
    assert_eq!(password.next().await, None);
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

#[tokio::test]
async fn lists_mailboxes_with_decoded_names_and_attributes() {
    let pki = pki(&["localhost"]);
    let (port, _) = fake_server(&pki, "* OK ready\r\n", PASSWORD).await;
    let mut session = Session::login(&localhost(port), "ada", &Secret::new(PASSWORD), &trusting(&pki))
        .await
        .unwrap();
    assert!(session.has_capability("special-use"));
    let mailboxes = session.list_mailboxes().await.unwrap();
    session.logout().await;

    let names: Vec<&str> = mailboxes.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "INBOX",
            "Sent",
            "Drafts",
            "Trash",
            "Spam",
            "Projects",
            "Projects/Résumé"
        ]
    );
    let resume = &mailboxes[6];
    assert_eq!(
        resume.remote_name, b"Projects/R&AOk-sum&AOk-",
        "the wire name is kept for commands"
    );
    assert_eq!(resume.delimiter, Some('/'));
    assert!(mailboxes[1].attributes.iter().any(|a| a == r"\Sent"));
    assert!(
        mailboxes[5]
            .attributes
            .iter()
            .any(|a| a.eq_ignore_ascii_case(r"\Noselect"))
    );
}

/// Folder list of a real server: `REGARDS_TEST_IMAP=… REGARDS_TEST_IMAP_CA=… cargo test -- --ignored`
#[tokio::test]
#[ignore = "needs a real IMAP server (REGARDS_TEST_IMAP, REGARDS_TEST_IMAP_CA)"]
async fn live_server_lists_mailboxes() {
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
    let mut session = Session::login(&server, &user, &password, &tls).await.unwrap();
    let mailboxes = session.list_mailboxes().await.unwrap();
    session.logout().await;
    for mailbox in &mailboxes {
        println!("{:?} {:?} {:?}", mailbox.name, mailbox.delimiter, mailbox.attributes);
    }
    assert!(mailboxes.iter().any(|m| m.name == "INBOX"));
}

/// Message operations against a real server, as a fresh user.
#[tokio::test]
#[ignore = "needs a real IMAP server (REGARDS_TEST_IMAP, REGARDS_TEST_IMAP_CA)"]
async fn live_server_message_operations() {
    let address = std::env::var("REGARDS_TEST_IMAP").unwrap();
    let (host, port) = address.rsplit_once(':').unwrap();
    let ca = std::fs::read(std::env::var("REGARDS_TEST_IMAP_CA").unwrap()).unwrap();
    let tls = TlsOptions::platform().with_extra_roots_pem(&ca).unwrap();
    let server = ImapServer {
        host: host.into(),
        port: port.parse().unwrap(),
    };
    let user = format!("live-{}", std::process::id());
    let password = Secret::new(std::env::var("REGARDS_TEST_IMAP_PASSWORD").unwrap_or_else(|_| "pass".into()));
    let mut session = Session::login(&server, &user, &password, &tls).await.unwrap();

    let message = |n: u32, subject: &str| {
        format!(
            "From: Ada <ada@example.test>\r\nTo: Bob <bob@example.test>\r\nSubject: {subject}\r\n\
             Message-ID: <live-{n}@example.test>\r\nDate: Sun, 18 Oct 2026 10:0{n}:00 +0000\r\n\r\nBody {n}\r\n"
        )
    };
    let first = session
        .append(b"INBOX", &[], message(1, "Plain").as_bytes())
        .await
        .unwrap();
    session
        .append(
            b"INBOX",
            &[SystemFlag::Seen],
            message(2, "=?UTF-8?Q?Caf=C3=A9?=").as_bytes(),
        )
        .await
        .unwrap();
    session
        .append(b"INBOX", &[], message(3, "Third").as_bytes())
        .await
        .unwrap();
    assert!(first.is_some(), "Dovecot reports APPENDUID");

    let selected = session.select(b"INBOX").await.unwrap();
    assert_eq!(selected.exists, 3);
    let fetched = session.fetch_headers(MessageSet::Sequence("1:*".into())).await.unwrap();
    assert_eq!(fetched.skipped, 0);
    assert_eq!(fetched.items.len(), 3);
    let cafe = &fetched.items[1];
    assert!(cafe.flags.seen);
    assert!(String::from_utf8_lossy(&cafe.header).contains("Subject: =?UTF-8?Q?Caf=C3=A9?="));
    assert!(cafe.internal_date.is_some() && cafe.size.is_some());

    let uids: Vec<u32> = fetched.items.iter().map(|m| m.uid).collect();
    assert_eq!(session.uid_search_all().await.unwrap(), uids);

    // CHANGEDSINCE sees only the message flagged afterwards.
    let modseq = fetched.items.iter().filter_map(|m| m.modseq).max();
    session
        .store_flags(&uids[0].to_string(), true, &[SystemFlag::Flagged])
        .await
        .unwrap();
    let changed = session.fetch_flags("1:*", modseq).await.unwrap();
    assert_eq!(changed.items.len(), 1);
    assert!(changed.items[0].flags.flagged);

    session
        .store_flags(&uids[2].to_string(), true, &[SystemFlag::Deleted])
        .await
        .unwrap();
    session.expunge().await.unwrap();
    assert_eq!(session.uid_search_all().await.unwrap(), &uids[..2]);
    session.logout().await;
}

/// IDLE push and clean stop against a real server.
#[tokio::test]
#[ignore = "needs a real IMAP server (REGARDS_TEST_IMAP, REGARDS_TEST_IMAP_CA)"]
async fn live_server_idle_reports_new_mail_and_stops_cleanly() {
    let address = std::env::var("REGARDS_TEST_IMAP").unwrap();
    let (host, port) = address.rsplit_once(':').unwrap();
    let ca = std::fs::read(std::env::var("REGARDS_TEST_IMAP_CA").unwrap()).unwrap();
    let tls = TlsOptions::platform().with_extra_roots_pem(&ca).unwrap();
    let server = ImapServer {
        host: host.into(),
        port: port.parse().unwrap(),
    };
    let user = format!("idle-{}", std::process::id());
    let password = Secret::new(std::env::var("REGARDS_TEST_IMAP_PASSWORD").unwrap_or_else(|_| "pass".into()));
    let mut watcher = Session::login(&server, &user, &password, &tls).await.unwrap();
    watcher.select(b"INBOX").await.unwrap();

    let mut sender = Session::login(&server, &user, &password, &tls).await.unwrap();
    let appended = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let at = std::time::Instant::now();
        sender
            .append(b"INBOX", &[], b"Subject: ping\r\nFrom: a@example.test\r\n\r\nhi\r\n")
            .await
            .unwrap();
        sender.logout().await;
        at
    });
    let end = watcher
        .idle(std::time::Duration::from_secs(20), std::future::pending())
        .await
        .unwrap();
    let latency = appended.await.unwrap().elapsed();
    assert_eq!(end, regards_imap::IdleEnd::Changed);
    assert!(latency < std::time::Duration::from_secs(5), "push took {latency:?}");

    let started = std::time::Instant::now();
    let end = watcher
        .idle(
            std::time::Duration::from_secs(60),
            tokio::time::sleep(std::time::Duration::from_millis(200)),
        )
        .await
        .unwrap();
    assert_eq!(end, regards_imap::IdleEnd::Stopped);
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    assert_eq!(
        watcher.uid_search_all().await.unwrap().len(),
        1,
        "the session still works after DONE"
    );
    watcher.logout().await;
}

/// MOVE with COPYUID against a real server.
#[tokio::test]
#[ignore = "needs a real IMAP server (REGARDS_TEST_IMAP, REGARDS_TEST_IMAP_CA)"]
async fn live_server_moves_messages_and_reports_their_new_uids() {
    let address = std::env::var("REGARDS_TEST_IMAP").unwrap();
    let (host, port) = address.rsplit_once(':').unwrap();
    let ca = std::fs::read(std::env::var("REGARDS_TEST_IMAP_CA").unwrap()).unwrap();
    let tls = TlsOptions::platform().with_extra_roots_pem(&ca).unwrap();
    let server = ImapServer {
        host: host.into(),
        port: port.parse().unwrap(),
    };
    let user = format!("move-{}", std::process::id());
    let password = Secret::new(std::env::var("REGARDS_TEST_IMAP_PASSWORD").unwrap_or_else(|_| "pass".into()));
    let mut session = Session::login(&server, &user, &password, &tls).await.unwrap();
    session.create_mailbox(b"Archive").await.unwrap();
    for n in 1..=2 {
        session
            .append(b"INBOX", &[], format!("Subject: m{n}\r\n\r\nbody\r\n").as_bytes())
            .await
            .unwrap();
    }
    session.select(b"INBOX").await.unwrap();
    let uids = session.uid_search_all().await.unwrap();
    let moved = session
        .move_messages(&uids[0].to_string(), b"Archive")
        .await
        .unwrap()
        .expect("Dovecot reports COPYUID");
    assert_eq!(moved.uids.len(), 1);
    assert_eq!(moved.uids[0].0, uids[0]);
    assert_eq!(session.uid_search_all().await.unwrap(), &uids[1..]);
    let archive = session.select(b"Archive").await.unwrap();
    assert_eq!(archive.uid_validity, moved.uid_validity);
    assert_eq!(session.uid_search_all().await.unwrap(), [moved.uids[0].1]);
    session.logout().await;
}

#[tokio::test]
#[ignore = "needs a real IMAP server (REGARDS_TEST_IMAP, REGARDS_TEST_IMAP_CA)"]
async fn live_server_deletes_exactly_the_messages_asked_for() {
    let address = std::env::var("REGARDS_TEST_IMAP").unwrap();
    let (host, port) = address.rsplit_once(':').unwrap();
    let ca = std::fs::read(std::env::var("REGARDS_TEST_IMAP_CA").unwrap()).unwrap();
    let tls = TlsOptions::platform().with_extra_roots_pem(&ca).unwrap();
    let server = ImapServer {
        host: host.into(),
        port: port.parse().unwrap(),
    };
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    let user = format!("delete-{}-{nanos}", std::process::id());
    let password = Secret::new(std::env::var("REGARDS_TEST_IMAP_PASSWORD").unwrap_or_else(|_| "pass".into()));
    let mut session = Session::login(&server, &user, &password, &tls).await.unwrap();
    for n in 1..=3 {
        session
            .append(b"INBOX", &[], format!("Subject: m{n}\r\n\r\nbody\r\n").as_bytes())
            .await
            .unwrap();
    }
    session.select(b"INBOX").await.unwrap();
    let uids = session.uid_search_all().await.unwrap();
    // Another client marked message 3 deleted without expunging it: it must survive.
    session
        .store_flags(&uids[2].to_string(), true, &[SystemFlag::Deleted])
        .await
        .unwrap();
    session.delete_messages(&uids[0].to_string()).await.unwrap();
    assert_eq!(session.uid_search_all().await.unwrap(), &uids[1..]);
    session.logout().await;
}

#[tokio::test]
#[ignore = "needs a real IMAP server (REGARDS_TEST_IMAP, REGARDS_TEST_IMAP_CA)"]
async fn live_server_reports_changes_since_a_modseq_in_the_select() {
    let address = std::env::var("REGARDS_TEST_IMAP").unwrap();
    let (host, port) = address.rsplit_once(':').unwrap();
    let ca = std::fs::read(std::env::var("REGARDS_TEST_IMAP_CA").unwrap()).unwrap();
    let tls = TlsOptions::platform().with_extra_roots_pem(&ca).unwrap();
    let server = ImapServer {
        host: host.into(),
        port: port.parse().unwrap(),
    };
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    let user = format!("qresync-{}-{nanos}", std::process::id());
    let password = Secret::new(std::env::var("REGARDS_TEST_IMAP_PASSWORD").unwrap_or_else(|_| "pass".into()));
    let mut session = Session::login(&server, &user, &password, &tls).await.unwrap();
    assert!(session.qresync(), "Dovecot has QRESYNC and enables it");
    for n in 1..=4 {
        session
            .append(b"INBOX", &[], format!("Subject: m{n}\r\n\r\nbody\r\n").as_bytes())
            .await
            .unwrap();
    }
    let first = session.select(b"INBOX").await.unwrap();
    let uids = session.uid_search_all().await.unwrap();
    // The sync point: what a sync would have stored now.
    let flags = session.fetch_flags("1:*", None).await.unwrap();
    let modseq = flags.items.iter().filter_map(|u| u.modseq).max().unwrap();

    // Meanwhile, elsewhere: one message read, one deleted.
    let mut other = Session::login(&server, &user, &password, &tls).await.unwrap();
    other.select(b"INBOX").await.unwrap();
    other
        .store_flags(&uids[1].to_string(), true, &[SystemFlag::Seen])
        .await
        .unwrap();
    other.delete_messages(&uids[2].to_string()).await.unwrap();
    other.logout().await;

    let since = Since {
        uid_validity: first.uid_validity,
        modseq,
        known: Some((uids[0], uids[3])),
    };
    let resynced = session.select_since(b"INBOX", &since).await.unwrap();
    assert!(resynced.resynced);
    assert!(
        resynced
            .vanished
            .iter()
            .any(|(low, high)| (*low..=*high).contains(&uids[2])),
        "{:?}",
        resynced.vanished
    );
    let changed: Vec<u32> = resynced.changed.iter().map(|u| u.uid).collect();
    assert!(changed.contains(&uids[1]), "{changed:?}");
    assert!(resynced.changed.iter().find(|u| u.uid == uids[1]).unwrap().flags.seen);
    assert!(
        !changed.contains(&uids[0]),
        "unchanged messages are not reported: {changed:?}"
    );

    // Under another UIDVALIDITY, nothing is relative to the old point.
    let stale = Since {
        uid_validity: first.uid_validity.wrapping_add(1).max(1),
        ..since
    };
    assert!(!session.select_since(b"INBOX", &stale).await.unwrap().resynced);
    session.logout().await;
}

#[tokio::test]
#[ignore = "needs a real IMAP server (REGARDS_TEST_IMAP, REGARDS_TEST_IMAP_CA)"]
async fn live_server_searches_text_people_flags_and_days() {
    use regards_imap::SearchTerm::{self, *};
    let address = std::env::var("REGARDS_TEST_IMAP").unwrap();
    let (host, port) = address.rsplit_once(':').unwrap();
    let ca = std::fs::read(std::env::var("REGARDS_TEST_IMAP_CA").unwrap()).unwrap();
    let tls = TlsOptions::platform().with_extra_roots_pem(&ca).unwrap();
    let server = ImapServer {
        host: host.into(),
        port: port.parse().unwrap(),
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    let user = format!("search-{}-{}", std::process::id(), now.subsec_nanos());
    let password = Secret::new(std::env::var("REGARDS_TEST_IMAP_PASSWORD").unwrap_or_else(|_| "pass".into()));
    let mut session = Session::login(&server, &user, &password, &tls).await.unwrap();
    let messages: [(&[SystemFlag], &str); 3] = [
        (
            &[],
            "From: Lena <lena@example.net>\r\nTo: Bob <bob@example.test>\r\nSubject: Trip plans\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\r\nWe climb the Matterhorn from Zermatt, near Täsch.\r\n",
        ),
        (
            &[SystemFlag::Seen, SystemFlag::Flagged],
            "From: Shop <billing@shop.example>\r\nTo: Ann <ann@example.test>\r\nCc: Bob <bob@example.test>\r\n\
             Subject: Invoice\r\n\r\nYour invoice for October.\r\n",
        ),
        (
            &[SystemFlag::Seen],
            "From: Ann <ann@example.test>\r\nSubject: Lunch\r\n\r\nNoon at the lake?\r\n",
        ),
    ];
    for (flags, source) in messages {
        session.append(b"INBOX", flags, source.as_bytes()).await.unwrap();
    }
    session.select(b"INBOX").await.unwrap();
    let all = session.uid_search_all().await.unwrap();
    let (trip, invoice, lunch) = (all[0], all[1], all[2]);
    let mut find = async |terms: &[SearchTerm]| session.uid_search(terms).await.unwrap();

    assert_eq!(
        find(&[Text("matterhorn".into())]).await,
        [trip],
        "in the text, any case"
    );
    assert_eq!(find(&[Text("lunch".into())]).await, [lunch], "in the header too");
    assert_eq!(find(&[From("lena".into())]).await, [trip]);
    assert_eq!(
        find(&[To("bob@example.test".into())]).await,
        [trip, invoice],
        "To or Cc"
    );
    assert_eq!(find(&[Subject("invoice".into())]).await, [invoice]);
    assert_eq!(find(&[Unseen]).await, [trip]);
    assert_eq!(find(&[Seen, Not(Box::new(Flagged))]).await, [lunch]);
    assert_eq!(find(&[Text("Täsch".into())]).await, [trip], "sent as UTF-8");
    let today = (now.as_secs() / 86_400) as i64;
    assert_eq!(find(&[Since(today - 1)]).await, all);
    assert!(find(&[Before(today - 1)]).await.is_empty());
    assert_eq!(find(&[]).await, all, "no terms: everything");
    session.logout().await;
}
