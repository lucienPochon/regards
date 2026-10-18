//! Sync against a real IMAP server (Dovecot in Docker; see spikes/imap-trial).
//! Opt-in: `REGARDS_TEST_IMAP=localhost:31993 REGARDS_TEST_IMAP_CA=ca.pem \
//! cargo test -p regards-daemon --test live -- --ignored`

use regards_core::mailbox::Role;
use regards_core::paths::DataDirs;
use regards_core::secret::Secret;
use regards_daemon::Daemon;
use regards_imap::{ImapServer, Session, SystemFlag, TlsOptions};
use regards_ipc::protocol::{
    AccountAddParams, AccountRef, AccountSummary, ClientKind, ConnectionSecurity, MailboxListResult, MessageDetail,
    MessageHtml, MessageHtmlParams, MessageIds, MessageListParams, MessageListResult, MessageRef, MoveResult,
    ServerSettings, SetFlagsParams, SetFlagsResult, SyncResult, ThreadView, UndoParams, UndoResult, methods,
};
use regards_ipc::{Client, UiToken};
use regards_vault::{SecretKey, SecretKind, Vault};
use serde_json::Value;
use std::time::{Duration, Instant};

/// Tests run in parallel and the clock can repeat: number the users too.
static NEXT_USER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

struct Live {
    server: ImapServer,
    tls: TlsOptions,
    user: String,
    password: Secret,
}

fn live() -> Live {
    let address = std::env::var("REGARDS_TEST_IMAP").expect("REGARDS_TEST_IMAP=host:port");
    let (host, port) = address.rsplit_once(':').unwrap();
    let ca = std::fs::read(std::env::var("REGARDS_TEST_IMAP_CA").expect("REGARDS_TEST_IMAP_CA=ca.pem")).unwrap();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    Live {
        server: ImapServer {
            host: host.into(),
            port: port.parse().unwrap(),
        },
        tls: TlsOptions::platform().with_extra_roots_pem(&ca).unwrap(),
        // A fresh user, so the mailbox starts empty.
        user: format!(
            "sync-{nanos}-{}",
            NEXT_USER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ),
        password: Secret::new(std::env::var("REGARDS_TEST_IMAP_PASSWORD").unwrap_or_else(|_| "pass".into())),
    }
}

fn message(n: u32, subject: &str, from: &str) -> Vec<u8> {
    format!(
        "From: {from}\r\nTo: Bob <bob@example.test>\r\nSubject: {subject}\r\n\
         Message-ID: <sync-{n}@example.test>\r\nDate: Sun, 18 Oct 2026 10:{n:02}:00 +0000\r\n\r\nBody {n}\r\n"
    )
    .into_bytes()
}

async fn sync(desktop: &mut Client, account_id: &str) -> SyncResult {
    desktop
        .call(
            methods::ACCOUNT_SYNC,
            AccountRef {
                account_id: account_id.into(),
            },
        )
        .await
        .unwrap()
}

async fn folders(desktop: &mut Client, account_id: &str) -> MailboxListResult {
    desktop
        .call(
            methods::MAILBOX_LIST,
            AccountRef {
                account_id: account_id.into(),
            },
        )
        .await
        .unwrap()
}

async fn messages(desktop: &mut Client, mailbox_id: i64) -> MessageListResult {
    desktop
        .call(
            methods::MESSAGE_LIST,
            MessageListParams {
                mailbox_id,
                limit: None,
                before: None,
            },
        )
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "needs a real IMAP server (REGARDS_TEST_IMAP, REGARDS_TEST_IMAP_CA)"]
async fn live_sync_follows_new_flagged_expunged_and_reset_mailboxes() {
    let live = live();

    // Seed the server.
    let mut seed = Session::login(&live.server, &live.user, &live.password, &live.tls)
        .await
        .unwrap();
    seed.append(b"INBOX", &[], &message(1, "Plain", "Ada <ada@example.test>"))
        .await
        .unwrap();
    seed.append(
        b"INBOX",
        &[SystemFlag::Seen],
        &message(
            2,
            "=?UTF-8?Q?R=C3=A9union?=",
            "=?UTF-8?Q?Ren=C3=A9e?= <renee@example.test>",
        ),
    )
    .await
    .unwrap();
    seed.append(b"INBOX", &[], &message(3, "Third", "carol@example.test"))
        .await
        .unwrap();
    seed.create_mailbox(b"Projects").await.unwrap();
    seed.append(b"Projects", &[], &message(4, "Old project", "ada@example.test"))
        .await
        .unwrap();

    // A daemon with the account.
    let temp = tempfile::tempdir().unwrap();
    let token = UiToken::generate().unwrap();
    let daemon = Daemon::bind(DataDirs::at(temp.path()))
        .await
        .unwrap()
        .with_ui_token(token.clone())
        .with_vault(Vault::in_memory())
        .with_imap_tls(live.tls.clone());
    let endpoint = daemon.endpoint().clone();
    let handle = tokio::spawn(daemon.serve(std::future::pending()));
    let mut desktop = Client::connect_with(&endpoint, ClientKind::Desktop, Some(&token))
        .await
        .unwrap();
    let account: AccountSummary = desktop
        .call(
            methods::ACCOUNT_ADD,
            AccountAddParams {
                display_name: "Sync test".into(),
                email: "sync@example.test".into(),
                username: Some(live.user.clone()),
                password: live.password.clone(),
                imap: ServerSettings {
                    host: live.server.host.clone(),
                    port: live.server.port,
                    security: ConnectionSecurity::Tls,
                },
                smtp: ServerSettings {
                    host: live.server.host.clone(),
                    port: 587,
                    security: ConnectionSecurity::Starttls,
                },
            },
        )
        .await
        .unwrap();

    // The background sync after adding.
    let deadline = Instant::now() + Duration::from_secs(20);
    let listed = loop {
        let listed = folders(&mut desktop, &account.id).await;
        if listed.sync.last_success_at.is_some() {
            break listed;
        }
        assert!(listed.sync.last_error.is_none(), "{:?}", listed.sync.last_error);
        assert!(Instant::now() < deadline, "the first sync did not finish");
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    let inbox = listed
        .mailboxes
        .iter()
        .find(|m| m.role == Some(Role::Inbox))
        .unwrap()
        .id;
    let projects = listed.mailboxes.iter().find(|m| m.name == "Projects").unwrap().id;

    let page = messages(&mut desktop, inbox).await;
    let subjects: Vec<&str> = page.messages.iter().map(|m| m.subject.as_str()).collect();
    assert_eq!(
        subjects,
        ["Third", "Réunion", "Plain"],
        "newest first, encoded words decoded"
    );
    let reunion = &page.messages[1];
    assert!(reunion.seen && !page.messages[0].seen);
    assert_eq!(reunion.from.as_ref().unwrap().name.as_deref(), Some("Renée"));
    assert_eq!(messages(&mut desktop, projects).await.messages.len(), 1);

    // Push: new mail shows up without asking for a sync (Phase 1 exit criterion: under 5 s).
    let deadline = Instant::now() + Duration::from_secs(20);
    while !folders(&mut desktop, &account.id).await.sync.push_active {
        assert!(Instant::now() < deadline, "the push connection did not start");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let sent = Instant::now();
    seed.append(b"INBOX", &[], &message(8, "Pushed", "eve@example.test"))
        .await
        .unwrap();
    loop {
        let page = messages(&mut desktop, inbox).await;
        if page.messages.iter().any(|m| m.subject == "Pushed") {
            break;
        }
        assert!(
            sent.elapsed() < Duration::from_secs(5),
            "new mail did not arrive by push within 5 s"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    println!("push latency: {:?}", sent.elapsed());

    // Change things on the server.
    let selected = seed.select(b"INBOX").await.unwrap();
    assert_eq!(selected.exists, 4);
    let uids = seed.uid_search_all().await.unwrap();
    seed.store_flags(&uids[0].to_string(), true, &[SystemFlag::Flagged])
        .await
        .unwrap();
    seed.store_flags(&uids[2].to_string(), true, &[SystemFlag::Deleted])
        .await
        .unwrap();
    seed.expunge().await.unwrap();
    seed.append(b"INBOX", &[], &message(5, "Fresh", "dave@example.test"))
        .await
        .unwrap();
    // Recreating a folder gives it a new UIDVALIDITY.
    seed.delete_mailbox(b"Projects").await.unwrap();
    seed.create_mailbox(b"Projects").await.unwrap();
    seed.append(b"Projects", &[], &message(6, "New project A", "ada@example.test"))
        .await
        .unwrap();
    seed.append(b"Projects", &[], &message(7, "New project B", "ada@example.test"))
        .await
        .unwrap();

    let result = sync(&mut desktop, &account.id).await;
    // Push may already have applied the inbox changes, so check the result, not the counts.
    assert_eq!(result.unreadable, 0);

    let page = messages(&mut desktop, inbox).await;
    let subjects: Vec<&str> = page.messages.iter().map(|m| m.subject.as_str()).collect();
    assert_eq!(subjects, ["Fresh", "Pushed", "Réunion", "Plain"]);
    assert!(page.messages[3].flagged, "the flag set on the server arrived");

    let projects = folders(&mut desktop, &account.id)
        .await
        .mailboxes
        .into_iter()
        .find(|m| m.name == "Projects")
        .unwrap()
        .id;
    let project_subjects: Vec<String> = messages(&mut desktop, projects)
        .await
        .messages
        .into_iter()
        .map(|m| m.subject)
        .collect();
    assert_eq!(
        project_subjects,
        ["New project B", "New project A"],
        "the old folder's message is gone"
    );

    // Paging.
    let first: MessageListResult = desktop
        .call(
            methods::MESSAGE_LIST,
            MessageListParams {
                mailbox_id: inbox,
                limit: Some(2),
                before: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(first.messages.len(), 2);
    let rest: MessageListResult = desktop
        .call(
            methods::MESSAGE_LIST,
            MessageListParams {
                mailbox_id: inbox,
                limit: Some(2),
                before: first.next,
            },
        )
        .await
        .unwrap();
    assert_eq!(rest.messages.len(), 2);
    assert!(rest.next.is_none());

    seed.logout().await;
    handle.abort();
}

#[tokio::test]
#[ignore = "needs a real IMAP server (REGARDS_TEST_IMAP, REGARDS_TEST_IMAP_CA)"]
async fn live_message_bodies_download_once_and_stay_unread() {
    let live = live();
    let vault = Vault::in_memory();
    let mut seed = Session::login(&live.server, &live.user, &live.password, &live.tls)
        .await
        .unwrap();
    let report = b"From: Ada <ada@example.test>\r\nTo: Bob <bob@example.test>\r\nSubject: Report\r\n\
        Date: Sun, 18 Oct 2026 11:00:00 +0000\r\nMIME-Version: 1.0\r\n\
        Content-Type: multipart/mixed; boundary=\"b\"\r\n\r\n\
        --b\r\nContent-Type: multipart/alternative; boundary=\"a\"\r\n\r\n\
        --a\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nPlease find the report attached.\r\n\
        --a\r\nContent-Type: text/html; charset=utf-8\r\n\r\n\
        <p>Please find the <b>report</b> attached. <a href=\"https://example.test/r?id=1\">Online</a></p>\r\n\
        --a--\r\n\
        --b\r\nContent-Type: application/pdf; name=\"report.pdf\"\r\n\
        Content-Disposition: attachment; filename=\"report.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\n\
        JVBERi0xLjQK\r\n--b--\r\n";
    seed.append(b"INBOX", &[], report).await.unwrap();

    let temp = tempfile::tempdir().unwrap();
    let token = UiToken::generate().unwrap();
    let daemon = Daemon::bind(DataDirs::at(temp.path()))
        .await
        .unwrap()
        .with_ui_token(token.clone())
        .with_vault(vault.clone())
        .with_imap_tls(live.tls.clone());
    let endpoint = daemon.endpoint().clone();
    let handle = tokio::spawn(daemon.serve(std::future::pending()));
    let mut desktop = Client::connect_with(&endpoint, ClientKind::Desktop, Some(&token))
        .await
        .unwrap();
    let account: AccountSummary = desktop
        .call(
            methods::ACCOUNT_ADD,
            AccountAddParams {
                display_name: "Bodies".into(),
                email: "bodies@example.test".into(),
                username: Some(live.user.clone()),
                password: live.password.clone(),
                imap: ServerSettings {
                    host: live.server.host.clone(),
                    port: live.server.port,
                    security: ConnectionSecurity::Tls,
                },
                smtp: ServerSettings {
                    host: live.server.host.clone(),
                    port: 587,
                    security: ConnectionSecurity::Starttls,
                },
            },
        )
        .await
        .unwrap();
    let inbox = loop {
        let listed = folders(&mut desktop, &account.id).await;
        if listed.sync.last_success_at.is_some() {
            break listed
                .mailboxes
                .iter()
                .find(|m| m.role == Some(Role::Inbox))
                .unwrap()
                .id;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    let id = messages(&mut desktop, inbox).await.messages[0].id;

    let detail: MessageDetail = desktop.call(methods::MESSAGE_GET, MessageRef { id }).await.unwrap();
    assert_eq!(detail.text.trim(), "Please find the report attached.");
    assert!(detail.has_html);
    assert_eq!(detail.attachments.len(), 1);
    assert_eq!(detail.attachments[0].name.as_deref(), Some("report.pdf"));
    assert_eq!(detail.to[0].address.as_deref(), Some("bob@example.test"));
    assert_eq!(detail.remote_images, 0);

    let html: MessageHtml = desktop
        .call(
            methods::MESSAGE_HTML,
            MessageHtmlParams {
                id,
                allow_remote: false,
                link_prefix: None,
            },
        )
        .await
        .unwrap();
    assert!(html.html.starts_with("<!doctype html>"));
    assert!(html.html.contains("<b>report</b>"), "{}", html.html);
    assert!(html.html.contains("href=\"https://example.test/r?id=1\""));
    assert_eq!(html.blocked_remote, 0);

    // For the reading frame, links point back at it, to be opened in the browser.
    let framed: MessageHtml = desktop
        .call(
            methods::MESSAGE_HTML,
            MessageHtmlParams {
                id,
                allow_remote: false,
                link_prefix: Some("regards-mail://localhost/open?t=k&url=".into()),
            },
        )
        .await
        .unwrap();
    assert!(
        framed
            .html
            .contains("href=\"regards-mail://localhost/open?t=k&amp;url=https%3A%2F%2Fexample.test%2Fr%3Fid%3D1\""),
        "{}",
        framed.html
    );

    // BODY.PEEK[]: reading it here did not mark it read on the server.
    seed.select(b"INBOX").await.unwrap();
    let flags = seed.fetch_flags("1:*", None).await.unwrap();
    assert!(!flags.items[0].flags.seen);

    // The second open comes from blobs/: with the password gone, a download would fail.
    vault
        .delete(&SecretKey::new(&account.id, SecretKind::MailPassword))
        .await
        .unwrap();
    let again: MessageDetail = desktop.call(methods::MESSAGE_GET, MessageRef { id }).await.unwrap();
    assert_eq!(again.text, detail.text);
    assert!(std::fs::read_dir(temp.path().join("blobs")).unwrap().next().is_some());

    seed.logout().await;
    handle.abort();
}

/// A daemon with one account on the live server, synced once.
async fn daemon_with_account(
    live: &Live,
) -> (
    tempfile::TempDir,
    Client,
    AccountSummary,
    i64,
    tokio::task::JoinHandle<()>,
) {
    let temp = tempfile::tempdir().unwrap();
    let token = UiToken::generate().unwrap();
    let daemon = Daemon::bind(DataDirs::at(temp.path()))
        .await
        .unwrap()
        .with_ui_token(token.clone())
        .with_vault(Vault::in_memory())
        .with_imap_tls(live.tls.clone());
    let endpoint = daemon.endpoint().clone();
    let handle = tokio::spawn(daemon.serve(std::future::pending()));
    let mut desktop = Client::connect_with(&endpoint, ClientKind::Desktop, Some(&token))
        .await
        .unwrap();
    let settings = |port| ServerSettings {
        host: live.server.host.clone(),
        port,
        security: ConnectionSecurity::Tls,
    };
    let account: AccountSummary = desktop
        .call(
            methods::ACCOUNT_ADD,
            AccountAddParams {
                display_name: "Live".into(),
                email: format!("{}@example.test", live.user),
                username: Some(live.user.clone()),
                password: live.password.clone(),
                imap: settings(live.server.port),
                smtp: ServerSettings {
                    security: ConnectionSecurity::Starttls,
                    ..settings(587)
                },
            },
        )
        .await
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let inbox = loop {
        let listed = folders(&mut desktop, &account.id).await;
        if listed.sync.last_success_at.is_some() {
            break listed
                .mailboxes
                .iter()
                .find(|m| m.role == Some(Role::Inbox))
                .unwrap()
                .id;
        }
        assert!(Instant::now() < deadline, "the first sync did not finish");
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    (temp, desktop, account, inbox, handle)
}

/// Wait until the server's flags for the INBOX match `check`.
async fn server_flags_become(seed: &mut Session, check: impl Fn(&[regards_imap::FlagUpdate]) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        seed.select(b"INBOX").await.unwrap();
        let flags = seed.fetch_flags("1:*", None).await.unwrap().items;
        if check(&flags) {
            return;
        }
        assert!(Instant::now() < deadline, "the server never got the change: {flags:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
#[ignore = "needs a real IMAP server (REGARDS_TEST_IMAP, REGARDS_TEST_IMAP_CA)"]
async fn live_flag_actions_apply_at_once_and_reach_the_server() {
    let live = live();
    let mut seed = Session::login(&live.server, &live.user, &live.password, &live.tls)
        .await
        .unwrap();
    seed.append(b"INBOX", &[], &message(1, "One", "ada@example.test"))
        .await
        .unwrap();
    seed.append(b"INBOX", &[], &message(2, "Two", "ada@example.test"))
        .await
        .unwrap();
    let (_temp, mut desktop, _account, inbox, handle) = daemon_with_account(&live).await;
    let listed = messages(&mut desktop, inbox).await.messages;
    let (two, one) = (listed[0].id, listed[1].id);

    let set = |ids: Vec<i64>, seen: Option<bool>, flagged: Option<bool>| SetFlagsParams { ids, seen, flagged };
    let result: SetFlagsResult = desktop
        .call(methods::MESSAGE_SET_FLAGS, set(vec![one], Some(true), None))
        .await
        .unwrap();
    assert_eq!(result.changed, 1);
    // Applied locally at once, before the server has answered.
    let local = messages(&mut desktop, inbox).await.messages;
    assert!(local.iter().find(|m| m.id == one).unwrap().seen);
    server_flags_become(&mut seed, |flags| flags.iter().any(|f| f.flags.seen)).await;

    // Repeating it changes nothing and queues nothing.
    let again: SetFlagsResult = desktop
        .call(methods::MESSAGE_SET_FLAGS, set(vec![one], Some(true), None))
        .await
        .unwrap();
    assert_eq!(again.changed, 0);

    let both: SetFlagsResult = desktop
        .call(methods::MESSAGE_SET_FLAGS, set(vec![one, two], None, Some(true)))
        .await
        .unwrap();
    assert_eq!(both.changed, 2);
    server_flags_become(&mut seed, |flags| {
        flags.len() == 2 && flags.iter().all(|f| f.flags.flagged)
    })
    .await;

    let _: SetFlagsResult = desktop
        .call(methods::MESSAGE_SET_FLAGS, set(vec![one], Some(false), None))
        .await
        .unwrap();
    server_flags_become(&mut seed, |flags| flags.iter().all(|f| !f.flags.seen)).await;

    // After a full sync, the local copy still agrees with the server.
    let _ = sync(&mut desktop, &_account.id).await;
    let local = messages(&mut desktop, inbox).await.messages;
    assert!(local.iter().all(|m| m.flagged && !m.seen), "{local:?}");

    seed.logout().await;
    handle.abort();
}

/// UIDs on the server in `mailbox`.
async fn server_uids(seed: &mut Session, mailbox: &[u8]) -> Vec<u32> {
    seed.select(mailbox).await.unwrap();
    seed.uid_search_all().await.unwrap()
}

/// Wait until `mailbox` on the server holds `count` messages.
async fn server_count_becomes(seed: &mut Session, mailbox: &[u8], count: usize) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let uids = server_uids(seed, mailbox).await;
        if uids.len() == count {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{} holds {} messages on the server, expected {count}",
            String::from_utf8_lossy(mailbox),
            uids.len()
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
#[ignore = "needs a real IMAP server (REGARDS_TEST_IMAP, REGARDS_TEST_IMAP_CA)"]
async fn live_archive_trash_and_undo_move_at_once_and_on_the_server() {
    let live = live();
    let mut seed = Session::login(&live.server, &live.user, &live.password, &live.tls)
        .await
        .unwrap();
    seed.create_mailbox(b"Archive").await.unwrap();
    for (n, subject) in [(1, "One"), (2, "Two"), (3, "Three")] {
        seed.append(b"INBOX", &[], &message(n, subject, "ada@example.test"))
            .await
            .unwrap();
    }
    let (_temp, mut desktop, account, inbox, handle) = daemon_with_account(&live).await;
    let listed = folders(&mut desktop, &account.id).await;
    let archive = listed
        .mailboxes
        .iter()
        .find(|m| m.role == Some(Role::Archive))
        .unwrap()
        .id;
    let subjects = |page: MessageListResult| page.messages.into_iter().map(|m| m.subject).collect::<Vec<_>>();
    let ids: Vec<(String, i64)> = messages(&mut desktop, inbox)
        .await
        .messages
        .into_iter()
        .map(|m| (m.subject, m.id))
        .collect();
    let id = |subject: &str| ids.iter().find(|(s, _)| s == subject).unwrap().1;

    // Archive: gone from the inbox and in the archive at once, then on the server.
    let archived: MoveResult = desktop
        .call(methods::MESSAGE_ARCHIVE, MessageIds { ids: vec![id("One")] })
        .await
        .unwrap();
    assert_eq!(archived.moved, 1);
    let undo = archived.undo.expect("archiving can be undone");
    assert_eq!(undo.description, "Archived");
    assert_eq!(subjects(messages(&mut desktop, inbox).await), ["Three", "Two"]);
    assert_eq!(subjects(messages(&mut desktop, archive).await), ["One"]);
    server_count_becomes(&mut seed, b"Archive", 1).await;
    server_count_becomes(&mut seed, b"INBOX", 2).await;

    // Undo after the server moved it: the same message comes back, there too.
    let undone: UndoResult = desktop
        .call(methods::UNDO_APPLY, UndoParams { id: undo.id })
        .await
        .unwrap();
    assert_eq!(undone.undone, 1);
    assert_eq!(subjects(messages(&mut desktop, inbox).await), ["Three", "Two", "One"]);
    server_count_becomes(&mut seed, b"INBOX", 3).await;
    server_count_becomes(&mut seed, b"Archive", 0).await;
    let again = desktop
        .call::<_, Value>(methods::UNDO_APPLY, UndoParams { id: undo.id })
        .await;
    assert!(again.is_err(), "an undo works once");

    // Trash.
    let trashed: MoveResult = desktop
        .call(methods::MESSAGE_TRASH, MessageIds { ids: vec![id("Two")] })
        .await
        .unwrap();
    assert_eq!(trashed.undo.unwrap().description, "Moved to Trash");
    server_count_becomes(&mut seed, b"Trash", 1).await;

    // Archive and undo at once: nothing should be lost, whichever reached the server first.
    let quick: MoveResult = desktop
        .call(methods::MESSAGE_ARCHIVE, MessageIds { ids: vec![id("Three")] })
        .await
        .unwrap();
    let _: UndoResult = desktop
        .call(
            methods::UNDO_APPLY,
            UndoParams {
                id: quick.undo.unwrap().id,
            },
        )
        .await
        .unwrap();
    assert!(subjects(messages(&mut desktop, inbox).await).contains(&"Three".to_owned()));
    server_count_becomes(&mut seed, b"INBOX", 2).await; // One and Three
    server_count_becomes(&mut seed, b"Archive", 0).await;

    seed.logout().await;
    handle.abort();
}

fn reply(n: u32, subject: &str, from: &str, parents: &[u32]) -> Vec<u8> {
    let ids: Vec<String> = parents.iter().map(|p| format!("<sync-{p}@example.test>")).collect();
    format!(
        "From: {from}\r\nTo: Ada <ada@example.test>\r\nSubject: {subject}\r\n\
         Message-ID: <sync-{n}@example.test>\r\nIn-Reply-To: {}\r\nReferences: {}\r\n\
         Date: Sun, 18 Oct 2026 10:{n:02}:00 +0000\r\n\r\nBody {n}\r\n",
        ids.last().unwrap(),
        ids.join(" ")
    )
    .into_bytes()
}

#[tokio::test]
#[ignore = "needs a real IMAP server (REGARDS_TEST_IMAP, REGARDS_TEST_IMAP_CA)"]
async fn live_conversations_span_folders() {
    let live = live();
    let mut seed = Session::login(&live.server, &live.user, &live.password, &live.tls)
        .await
        .unwrap();
    // Most servers have a Sent folder already; make sure of it.
    let _ = seed.create_mailbox(b"Sent").await;
    seed.append(b"INBOX", &[], &message(1, "Plans", "Ada <ada@example.test>"))
        .await
        .unwrap();
    seed.append(b"Sent", &[], &reply(2, "Re: Plans", "Bob <bob@example.test>", &[1]))
        .await
        .unwrap();
    seed.append(b"INBOX", &[], &message(3, "Unrelated", "carol@example.test"))
        .await
        .unwrap();
    seed.append(b"INBOX", &[], &reply(4, "Re: Plans", "Ada <ada@example.test>", &[1, 2]))
        .await
        .unwrap();
    let (_temp, mut desktop, _account, inbox, handle) = daemon_with_account(&live).await;

    let ids: Vec<(String, i64)> = messages(&mut desktop, inbox)
        .await
        .messages
        .into_iter()
        .map(|m| (m.subject, m.id))
        .collect();
    let id = |subject: &str| ids.iter().find(|(s, _)| s == subject).unwrap().1;
    let thread = |view: ThreadView| {
        view.messages
            .into_iter()
            .map(|e| (e.message.subject, e.mailbox_role))
            .collect::<Vec<_>>()
    };

    // The original, the reply in Sent and the answer to it, oldest first.
    let view: ThreadView = desktop
        .call(methods::THREAD_GET, MessageRef { id: id("Plans") })
        .await
        .unwrap();
    let plans = [
        ("Plans".to_owned(), Some(Role::Inbox)),
        ("Re: Plans".to_owned(), Some(Role::Sent)),
        ("Re: Plans".to_owned(), Some(Role::Inbox)),
    ];
    assert_eq!(thread(view), plans);
    let from_reply: ThreadView = desktop
        .call(methods::THREAD_GET, MessageRef { id: id("Re: Plans") })
        .await
        .unwrap();
    assert_eq!(thread(from_reply), plans);
    let alone: ThreadView = desktop
        .call(methods::THREAD_GET, MessageRef { id: id("Unrelated") })
        .await
        .unwrap();
    assert_eq!(thread(alone), [("Unrelated".to_owned(), Some(Role::Inbox))]);

    // A message on its way to the trash shows there at once.
    let _: MoveResult = desktop
        .call(methods::MESSAGE_TRASH, MessageIds { ids: vec![id("Plans")] })
        .await
        .unwrap();
    let moved: ThreadView = desktop
        .call(methods::THREAD_GET, MessageRef { id: id("Re: Plans") })
        .await
        .unwrap();
    assert_eq!(thread(moved)[0], ("Plans".to_owned(), Some(Role::Trash)));

    seed.logout().await;
    handle.abort();
}
