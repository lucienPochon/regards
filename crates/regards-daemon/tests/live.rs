//! Sync against a real IMAP server (Dovecot in Docker; see spikes/imap-trial).
//! Opt-in: `REGARDS_TEST_IMAP=localhost:31993 REGARDS_TEST_IMAP_CA=ca.pem \
//! cargo test -p regards-daemon --test live -- --ignored`

use regards_core::headers::Person;
use regards_core::mailbox::Role;
use regards_core::paths::DataDirs;
use regards_core::secret::Secret;
use regards_daemon::{Daemon, SyncLimits};
use regards_imap::{ImapServer, Session, SystemFlag, TlsOptions};
use regards_ipc::protocol::{
    AccountAddParams, AccountRef, AccountSummary, AsideParams, ClientKind, ConnectionSecurity, DeleteResult,
    DraftAttachFileParams, DraftCreateParams, DraftDetachParams, DraftListParams, DraftListResult, DraftRef,
    DraftReplyParams, DraftSendParams, DraftSendResult, DraftUpdateParams, DraftView, MailboxListResult, MarkResult,
    MessageDetail, MessageHtml, MessageHtmlParams, MessageIds, MessageListParams, MessageListResult, MessageRef,
    MoveResult, OutboxEntryView, OutboxListResult, OutboxRef, OutboxState, OutboxUndoResult, PinParams, ServerSettings,
    SetFlagsParams, SetFlagsResult, SnoozeParams, SyncResult, ThreadView, UndoParams, UndoResult, View, ViewParams,
    methods,
};
use regards_ipc::{Client, UiToken};
use regards_vault::{SecretKey, SecretKind, Vault};
use serde_json::Value;
use std::time::{Duration, Instant};

#[path = "support/proxy.rs"]
mod proxy;

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
        .with_tls(live.tls.clone());
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
        .with_tls(live.tls.clone());
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
type WithAccount = (
    tempfile::TempDir,
    Client,
    AccountSummary,
    i64,
    tokio::task::JoinHandle<()>,
);

async fn daemon_with_account(live: &Live) -> WithAccount {
    daemon_with_limits(live, SyncLimits::default()).await
}

async fn daemon_with_limits(live: &Live, limits: SyncLimits) -> WithAccount {
    let smtp = ServerSettings {
        host: live.server.host.clone(),
        port: 587,
        security: ConnectionSecurity::Starttls,
    };
    daemon_with_settings(live, limits, smtp, live.tls.clone()).await
}

/// A daemon with one account: IMAP on the live server, SMTP as given.
async fn daemon_with_settings(live: &Live, limits: SyncLimits, smtp: ServerSettings, tls: TlsOptions) -> WithAccount {
    let temp = tempfile::tempdir().unwrap();
    let token = UiToken::generate().unwrap();
    let daemon = Daemon::bind(DataDirs::at(temp.path()))
        .await
        .unwrap()
        .with_ui_token(token.clone())
        .with_vault(Vault::in_memory())
        .with_tls(tls)
        .with_sync_limits(limits);
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
                smtp,
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

#[tokio::test]
#[ignore = "needs a real IMAP server (REGARDS_TEST_IMAP, REGARDS_TEST_IMAP_CA)"]
async fn live_messages_in_trash_can_be_deleted_for_good() {
    let live = live();
    let mut seed = Session::login(&live.server, &live.user, &live.password, &live.tls)
        .await
        .unwrap();
    for (n, subject) in [(1, "Keep"), (2, "Gone")] {
        seed.append(b"INBOX", &[], &message(n, subject, "ada@example.test"))
            .await
            .unwrap();
    }
    let (_temp, mut desktop, account, inbox, handle) = daemon_with_account(&live).await;
    let trash = folders(&mut desktop, &account.id)
        .await
        .mailboxes
        .iter()
        .find(|m| m.role == Some(Role::Trash))
        .unwrap()
        .id;
    let listed = messages(&mut desktop, inbox).await.messages;
    let id = |subject: &str| listed.iter().find(|m| m.subject == subject).unwrap().id;

    // Not from the inbox: only Trash and Junk.
    let refused = desktop
        .call::<_, Value>(methods::MESSAGE_DELETE, MessageIds { ids: vec![id("Keep")] })
        .await;
    assert!(refused.is_err(), "deleting outside Trash is refused");

    let _: MoveResult = desktop
        .call(methods::MESSAGE_TRASH, MessageIds { ids: vec![id("Gone")] })
        .await
        .unwrap();
    server_count_becomes(&mut seed, b"Trash", 1).await;
    // The move kept the row (COPYUID), so the same id now names the copy in Trash.
    let detail: MessageDetail = desktop
        .call(methods::MESSAGE_GET, MessageRef { id: id("Gone") })
        .await
        .unwrap();
    assert_eq!(detail.mailbox_id, trash);

    let deleted: DeleteResult = desktop
        .call(methods::MESSAGE_DELETE, MessageIds { ids: vec![id("Gone")] })
        .await
        .unwrap();
    assert_eq!(deleted.deleted, 1);
    assert!(messages(&mut desktop, trash).await.messages.is_empty(), "gone at once");
    server_count_becomes(&mut seed, b"Trash", 0).await;
    server_count_becomes(&mut seed, b"INBOX", 1).await;

    // A sync afterwards does not bring it back.
    sync(&mut desktop, &account.id).await;
    assert!(messages(&mut desktop, trash).await.messages.is_empty());

    seed.logout().await;
    handle.abort();
}

#[tokio::test]
#[ignore = "needs a real IMAP server (REGARDS_TEST_IMAP, REGARDS_TEST_IMAP_CA)"]
async fn live_older_mail_is_fetched_in_the_background() {
    let live = live();
    let mut seed = Session::login(&live.server, &live.user, &live.password, &live.tls)
        .await
        .unwrap();
    for n in 1..=10 {
        let flags: &[SystemFlag] = if n == 2 { &[SystemFlag::Flagged] } else { &[] };
        seed.append(b"INBOX", flags, &message(n, &format!("M{n}"), "ada@example.test"))
            .await
            .unwrap();
    }
    // The first sync takes the newest 3; the rest come 2 at a time.
    let limits = SyncLimits {
        initial_window: 3,
        backfill_batch: 2,
    };
    let (_temp, mut desktop, account, inbox, handle) = daemon_with_limits(&live, limits).await;
    // Appended within a second, they share an arrival time: compare them as a set.
    let subjects = |page: MessageListResult| {
        page.messages
            .into_iter()
            .map(|m| m.subject)
            .collect::<std::collections::BTreeSet<_>>()
    };
    let all: std::collections::BTreeSet<String> = (1..=10).map(|n| format!("M{n}")).collect();
    // Only the newest 3 at first (the backfill may already have begun).
    assert!(messages(&mut desktop, inbox).await.messages.len() < 10);

    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let shown = subjects(messages(&mut desktop, inbox).await);
        if shown == all {
            break;
        }
        assert!(Instant::now() < deadline, "older mail did not arrive: {shown:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let page = messages(&mut desktop, inbox).await;
    assert!(page.messages.iter().find(|m| m.subject == "M2").unwrap().flagged);

    // A later sync keeps them (deletions come only from the server's UID list).
    sync(&mut desktop, &account.id).await;
    assert_eq!(subjects(messages(&mut desktop, inbox).await), all);

    seed.logout().await;
    handle.abort();
}

/// Wait until the account's push connection is (or is no longer) up.
async fn push_becomes(desktop: &mut Client, account_id: &str, active: bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while folders(desktop, account_id).await.sync.push_active != active {
        assert!(Instant::now() < deadline, "push never became active={active}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
#[ignore = "needs a real IMAP server (REGARDS_TEST_IMAP, REGARDS_TEST_IMAP_CA)"]
async fn live_actions_taken_offline_reach_the_server_once_it_is_back() {
    let live = live();
    let mut seed = Session::login(&live.server, &live.user, &live.password, &live.tls)
        .await
        .unwrap();
    seed.create_mailbox(b"Archive").await.unwrap();
    for (n, subject) in [(1, "Read me"), (2, "Archive me")] {
        seed.append(b"INBOX", &[], &message(n, subject, "ada@example.test"))
            .await
            .unwrap();
    }
    // The daemon reaches the server only through a proxy that can cut the line.
    let proxy = proxy::Proxy::start(format!("{}:{}", live.server.host, live.server.port)).await;
    let via_proxy = Live {
        server: ImapServer {
            host: live.server.host.clone(),
            port: proxy.port,
        },
        tls: live.tls.clone(),
        user: live.user.clone(),
        password: live.password.clone(),
    };
    let (_temp, mut desktop, account, inbox, handle) = daemon_with_account(&via_proxy).await;
    push_becomes(&mut desktop, &account.id, true).await;
    let archive = folders(&mut desktop, &account.id)
        .await
        .mailboxes
        .iter()
        .find(|m| m.role == Some(Role::Archive))
        .unwrap()
        .id;
    let listed = messages(&mut desktop, inbox).await.messages;
    let id = |subject: &str| listed.iter().find(|m| m.subject == subject).unwrap().id;

    // Offline: the push connection drops and nothing gets through.
    proxy.go_offline();
    push_becomes(&mut desktop, &account.id, false).await;
    let _: SetFlagsResult = desktop
        .call(
            methods::MESSAGE_SET_FLAGS,
            SetFlagsParams {
                ids: vec![id("Read me")],
                seen: Some(true),
                flagged: None,
            },
        )
        .await
        .unwrap();
    let _: MoveResult = desktop
        .call(
            methods::MESSAGE_ARCHIVE,
            MessageIds {
                ids: vec![id("Archive me")],
            },
        )
        .await
        .unwrap();

    // Both show at once.
    let inbox_now = messages(&mut desktop, inbox).await.messages;
    assert_eq!(inbox_now.len(), 1);
    assert!(inbox_now[0].seen);
    assert_eq!(messages(&mut desktop, archive).await.messages.len(), 1);
    // The server has heard nothing.
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(server_uids(&mut seed, b"INBOX").await.len(), 2);

    // Back online: the push connection returns and the waiting actions go at
    // once, not after their retry delay (measured: about 5 s with, 9 s without).
    proxy.go_online();
    let back = Instant::now();
    server_count_becomes(&mut seed, b"Archive", 1).await;
    server_count_becomes(&mut seed, b"INBOX", 1).await;
    server_flags_become(&mut seed, |flags| flags.len() == 1 && flags[0].flags.seen).await;
    assert!(back.elapsed() < Duration::from_secs(8), "took {:?}", back.elapsed());
    // And the app still shows the same.
    assert!(messages(&mut desktop, inbox).await.messages[0].seen);
    assert_eq!(messages(&mut desktop, archive).await.messages.len(), 1);

    seed.logout().await;
    handle.abort();
}

#[path = "../../regards-smtp/tests/support/fake_smtp.rs"]
mod fake_smtp;

/// A daemon whose account sends through a fake SMTP server (STARTTLS on 127.0.0.1).
async fn daemon_sending_through(live: &Live, script: fake_smtp::Script) -> (WithAccount, fake_smtp::Sessions) {
    let smtp_pki = fake_smtp::pki();
    let (port, sessions) = fake_smtp::serve(
        &smtp_pki,
        fake_smtp::Script {
            password: Box::leak(live.password.expose().to_owned().into_boxed_str()),
            ..script
        },
    )
    .await;
    let tls = live
        .tls
        .clone()
        .with_extra_roots_pem(smtp_pki.ca_pem.as_bytes())
        .unwrap();
    let smtp = ServerSettings {
        host: "127.0.0.1".into(),
        port,
        security: ConnectionSecurity::Starttls,
    };
    (
        daemon_with_settings(live, SyncLimits::default(), smtp, tls).await,
        sessions,
    )
}

async fn outbox(desktop: &mut Client) -> Vec<OutboxEntryView> {
    desktop
        .call::<_, OutboxListResult>(methods::OUTBOX_LIST, Value::Null)
        .await
        .unwrap()
        .entries
}

/// Wait until the outbox entries satisfy `check`.
async fn outbox_becomes(desktop: &mut Client, check: impl Fn(&[OutboxEntryView]) -> bool) -> Vec<OutboxEntryView> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let entries = outbox(desktop).await;
        if check(&entries) {
            return entries;
        }
        assert!(Instant::now() < deadline, "the outbox stayed {entries:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn person(address: &str) -> Person {
    Person {
        name: None,
        address: Some(address.into()),
    }
}

#[tokio::test]
#[ignore = "needs a real IMAP server (REGARDS_TEST_IMAP, REGARDS_TEST_IMAP_CA)"]
async fn live_a_reply_can_be_undone_then_goes_out_once_and_lands_in_sent() {
    let live = live();
    let mut seed = Session::login(&live.server, &live.user, &live.password, &live.tls)
        .await
        .unwrap();
    let _ = seed.create_mailbox(b"Sent").await;
    seed.append(b"INBOX", &[], &message(1, "Plans", "Ada <ada@example.test>"))
        .await
        .unwrap();
    let script = fake_smtp::Script::new(fake_smtp::Mode::StartTls {
        offer: true,
        inject: false,
    });
    let ((_temp, mut desktop, account, inbox, handle), mut sent) = daemon_sending_through(&live, script).await;
    let original = messages(&mut desktop, inbox).await.messages[0].id;

    // Reply: addressed to the sender, quoted, threaded.
    let draft: DraftView = desktop
        .call(
            methods::DRAFT_REPLY,
            DraftReplyParams {
                message_id: original,
                all: false,
            },
        )
        .await
        .unwrap();
    assert_eq!(draft.to[0].address.as_deref(), Some("ada@example.test"));
    assert_eq!(draft.subject, "Re: Plans");
    assert!(draft.body.contains("Ada wrote:\n> Body 1"), "{}", draft.body);
    assert_eq!(draft.replying_to, Some(original));
    let me = format!("{}@example.test", live.user);
    assert_eq!(draft.from.address.as_deref(), Some(me.as_str()));
    assert_eq!(draft.from.name.as_deref(), Some("Live"));
    let draft: DraftView = desktop
        .call(
            methods::DRAFT_UPDATE,
            DraftUpdateParams {
                id: draft.id,
                to: draft.to.clone(),
                cc: vec![],
                bcc: vec![person("hidden@example.test")],
                subject: draft.subject.clone(),
                body: String::new(),
                html: Some(format!(
                    "<p>Thanks! <strong>Bold</strong><script>steal()</script></p><blockquote><p>{}</p></blockquote>",
                    "Body 1"
                )),
            },
        )
        .await
        .unwrap();

    // Send, and undo at once: nothing goes out, the draft is editable again.
    let queued: DraftSendResult = desktop
        .call(
            methods::DRAFT_SEND,
            DraftSendParams {
                id: draft.id,
                undo_seconds: Some(10),
            },
        )
        .await
        .unwrap();
    assert_eq!(outbox(&mut desktop).await[0].state, OutboxState::Queued);
    let undone: OutboxUndoResult = desktop
        .call(methods::OUTBOX_UNDO, OutboxRef { id: queued.outbox_id })
        .await
        .unwrap();
    assert_eq!(undone.draft_id, Some(draft.id));
    assert!(outbox(&mut desktop).await.is_empty());
    let saved: DraftListResult = desktop
        .call(methods::DRAFT_LIST, DraftListParams::default())
        .await
        .unwrap();
    assert_eq!(saved.drafts.len(), 1, "back among the drafts");

    // Send for real, with a 1-second window.
    let _: DraftSendResult = desktop
        .call(
            methods::DRAFT_SEND,
            DraftSendParams {
                id: draft.id,
                undo_seconds: Some(1),
            },
        )
        .await
        .unwrap();
    let delivered = tokio::time::timeout(Duration::from_secs(20), sent.next())
        .await
        .expect("the message reached the SMTP server");
    assert_eq!(delivered.from.as_deref(), Some(me.as_str()));
    assert_eq!(delivered.to, ["ada@example.test", "hidden@example.test"]);
    let data = String::from_utf8(delivered.data.unwrap()).unwrap();
    assert!(data.contains("Subject: Re: Plans"), "{data}");
    assert!(data.contains("In-Reply-To: <sync-1@example.test>"), "{data}");
    assert!(data.contains("Thanks!"));
    let delivered_body = regards_core::body::parse(data.as_bytes());
    assert!(delivered_body.has_html, "sent with its formatting");
    assert!(delivered_body.text.contains("Thanks! Bold"), "{}", delivered_body.text);
    let shown = regards_core::html::render(data.as_bytes(), false, None).unwrap().html;
    assert!(
        shown.contains("<strong>Bold</strong>") && !shown.contains("steal"),
        "{shown}"
    );
    assert!(!data.contains("<script"), "sanitised before it was stored");
    assert!(!data.contains("hidden@example.test"), "Bcc is not in the message");

    // The copy lands in Sent, the original is answered, the outbox and draft are done.
    server_count_becomes(&mut seed, b"Sent", 1).await;
    server_flags_become(&mut seed, |flags| flags.len() == 1 && flags[0].flags.answered).await;
    outbox_becomes(&mut desktop, <[_]>::is_empty).await;
    let gone = desktop
        .call::<_, Value>(methods::DRAFT_GET, DraftRef { id: draft.id })
        .await;
    assert!(gone.is_err(), "the draft is gone once sent");
    let saved: DraftListResult = desktop
        .call(methods::DRAFT_LIST, DraftListParams::default())
        .await
        .unwrap();
    assert!(saved.drafts.is_empty());
    let sent_folder = folders(&mut desktop, &account.id)
        .await
        .mailboxes
        .iter()
        .find(|m| m.role == Some(Role::Sent))
        .unwrap()
        .id;
    // Synced into Sent right after the copy, or by the next sync if that failed.
    if messages(&mut desktop, sent_folder).await.messages.is_empty() {
        sync(&mut desktop, &account.id).await;
    }
    assert_eq!(
        messages(&mut desktop, sent_folder).await.messages[0].subject,
        "Re: Plans"
    );
    let conversation: ThreadView = desktop
        .call(methods::THREAD_GET, MessageRef { id: original })
        .await
        .unwrap();
    assert_eq!(conversation.messages.len(), 2, "the reply joined the conversation");
    // Sent exactly once.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(sent.try_next().is_none());

    seed.logout().await;
    handle.abort();
}

#[tokio::test]
#[ignore = "needs a real IMAP server (REGARDS_TEST_IMAP, REGARDS_TEST_IMAP_CA)"]
async fn live_a_message_that_may_have_gone_is_never_resent_by_itself() {
    let live = live();
    let script = fake_smtp::Script {
        fault: fake_smtp::Fault::DropAfterData,
        reject: &["nobody@example.test"],
        ..fake_smtp::Script::new(fake_smtp::Mode::StartTls {
            offer: true,
            inject: false,
        })
    };
    let ((_temp, mut desktop, account, _inbox, handle), mut sent) = daemon_sending_through(&live, script).await;
    let new = |address: &str| DraftCreateParams {
        account_id: account.id.clone(),
        to: vec![person(address)],
        subject: "Hello".into(),
        body: "Hi".into(),
        ..DraftCreateParams::default()
    };

    // The connection drops after the whole message: uncertain, and it stays so.
    let draft: DraftView = desktop
        .call(methods::DRAFT_CREATE, new("bob@example.test"))
        .await
        .unwrap();
    let _: DraftSendResult = desktop
        .call(
            methods::DRAFT_SEND,
            DraftSendParams {
                id: draft.id,
                undo_seconds: Some(0),
            },
        )
        .await
        .unwrap();
    let entries = outbox_becomes(&mut desktop, |e| {
        e.first().is_some_and(|e| e.state == OutboxState::Uncertain)
    })
    .await;
    assert!(entries[0].last_error.as_deref().unwrap().contains("may or may not"));
    assert!(sent.next().await.data.is_some());
    tokio::time::sleep(Duration::from_secs(6)).await;
    assert!(sent.try_next().is_none(), "never retried by itself");
    // The user says it went: it is forgotten with its draft.
    let _: Value = desktop
        .call(methods::OUTBOX_DISMISS, OutboxRef { id: entries[0].id })
        .await
        .unwrap();
    assert!(outbox(&mut desktop).await.is_empty());

    // A refused recipient: failed, with the reason, and back to the draft on "edit".
    let draft: DraftView = desktop
        .call(methods::DRAFT_CREATE, new("nobody@example.test"))
        .await
        .unwrap();
    let _: DraftSendResult = desktop
        .call(
            methods::DRAFT_SEND,
            DraftSendParams {
                id: draft.id,
                undo_seconds: Some(0),
            },
        )
        .await
        .unwrap();
    let entries = outbox_becomes(&mut desktop, |e| {
        e.first().is_some_and(|e| e.state == OutboxState::Failed)
    })
    .await;
    assert!(
        entries[0]
            .last_error
            .as_deref()
            .unwrap()
            .contains("nobody@example.test")
    );
    let _: OutboxUndoResult = desktop
        .call(methods::OUTBOX_UNDO, OutboxRef { id: entries[0].id })
        .await
        .unwrap();
    let editable: DraftView = desktop
        .call(methods::DRAFT_GET, DraftRef { id: draft.id })
        .await
        .unwrap();
    assert!(!editable.sending);

    handle.abort();
}

#[tokio::test]
#[ignore = "needs a real IMAP server (REGARDS_TEST_IMAP, REGARDS_TEST_IMAP_CA)"]
async fn live_attached_files_go_out_with_the_message() {
    let live = live();
    let script = fake_smtp::Script::new(fake_smtp::Mode::StartTls {
        offer: true,
        inject: false,
    });
    let ((temp, mut desktop, account, _inbox, handle), mut sent) = daemon_sending_through(&live, script).await;
    let files = tempfile::tempdir().unwrap();
    let report = files.path().join("Rapport été.pdf");
    std::fs::write(&report, b"%PDF-1.4 \x00\x01\x02 binary").unwrap();
    let extra = files.path().join("extra.txt");
    std::fs::write(&extra, b"not wanted").unwrap();
    let huge = files.path().join("huge.bin");
    std::fs::File::create(&huge).unwrap().set_len(26 * 1024 * 1024).unwrap();

    let draft: DraftView = desktop
        .call(
            methods::DRAFT_CREATE,
            DraftCreateParams {
                account_id: account.id.clone(),
                to: vec![person("bob@example.test")],
                subject: "The report".into(),
                body: "Attached.".into(),
                ..DraftCreateParams::default()
            },
        )
        .await
        .unwrap();
    let attach = |path: &std::path::Path| DraftAttachFileParams {
        draft_id: draft.id,
        path: path.to_string_lossy().into_owned(),
    };
    let _: DraftView = desktop.call(methods::DRAFT_ATTACH_FILE, attach(&report)).await.unwrap();
    let with_both: DraftView = desktop.call(methods::DRAFT_ATTACH_FILE, attach(&extra)).await.unwrap();
    assert_eq!(with_both.attachments.len(), 2);
    assert_eq!(with_both.attachments[0].name, "Rapport été.pdf");
    assert_eq!(with_both.attachments[0].mime, "application/pdf");
    let too_big = desktop
        .call::<_, Value>(methods::DRAFT_ATTACH_FILE, attach(&huge))
        .await;
    assert!(too_big.is_err(), "over the size limit");
    let relative = desktop
        .call::<_, Value>(
            methods::DRAFT_ATTACH_FILE,
            DraftAttachFileParams {
                draft_id: draft.id,
                path: "relative.txt".into(),
            },
        )
        .await;
    assert!(relative.is_err(), "only full paths");
    let one: DraftView = desktop
        .call(
            methods::DRAFT_DETACH,
            DraftDetachParams {
                draft_id: draft.id,
                attachment_id: with_both.attachments[1].id,
            },
        )
        .await
        .unwrap();
    assert_eq!(one.attachments.len(), 1);

    // The file is kept by the daemon: deleting the original changes nothing.
    std::fs::remove_file(&report).unwrap();
    let _: DraftSendResult = desktop
        .call(
            methods::DRAFT_SEND,
            DraftSendParams {
                id: draft.id,
                undo_seconds: Some(0),
            },
        )
        .await
        .unwrap();
    let delivered = tokio::time::timeout(Duration::from_secs(20), sent.next())
        .await
        .expect("the message reached the SMTP server");
    let body = regards_core::body::parse(&delivered.data.unwrap());
    assert_eq!(body.attachments.len(), 1);
    assert_eq!(body.attachments[0].name.as_deref(), Some("Rapport été.pdf"));
    assert_eq!(body.attachments[0].mime, "application/pdf");
    assert_eq!(body.attachments[0].size, b"%PDF-1.4 \x00\x01\x02 binary".len());
    drop(temp);
    handle.abort();
}

#[tokio::test]
#[ignore = "needs a real IMAP server (REGARDS_TEST_IMAP, REGARDS_TEST_IMAP_CA)"]
async fn live_a_forward_carries_the_original_files() {
    let live = live();
    let mut seed = Session::login(&live.server, &live.user, &live.password, &live.tls)
        .await
        .unwrap();
    let report = b"From: Ada <ada@example.test>\r\nTo: Bob <bob@example.test>\r\nSubject: Report\r\n\
        Date: Sun, 18 Oct 2026 11:00:00 +0000\r\nMIME-Version: 1.0\r\n\
        Content-Type: multipart/mixed; boundary=\"b\"\r\n\r\n\
        --b\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nPlease find the report attached.\r\n\
        --b\r\nContent-Type: application/pdf; name=\"report.pdf\"\r\n\
        Content-Disposition: attachment; filename=\"report.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\n\
        JVBERi0xLjQK\r\n--b--\r\n";
    seed.append(b"INBOX", &[], report).await.unwrap();
    let script = fake_smtp::Script::new(fake_smtp::Mode::StartTls {
        offer: true,
        inject: false,
    });
    let ((_temp, mut desktop, _account, inbox, handle), mut sent) = daemon_sending_through(&live, script).await;
    let original = messages(&mut desktop, inbox).await.messages[0].id;

    let draft: DraftView = desktop
        .call(methods::DRAFT_FORWARD, MessageRef { id: original })
        .await
        .unwrap();
    assert_eq!(draft.subject, "Fwd: Report");
    assert!(draft.body.contains("Please find the report attached."));
    assert_eq!(draft.attachments.len(), 1);
    assert_eq!(draft.attachments[0].name, "report.pdf");
    assert_eq!(draft.attachments[0].size, 9);

    let _: DraftView = desktop
        .call(
            methods::DRAFT_UPDATE,
            DraftUpdateParams {
                id: draft.id,
                to: vec![person("carol@example.test")],
                cc: vec![],
                bcc: vec![],
                subject: draft.subject.clone(),
                body: draft.body.clone(),
                html: None,
            },
        )
        .await
        .unwrap();
    let _: DraftSendResult = desktop
        .call(
            methods::DRAFT_SEND,
            DraftSendParams {
                id: draft.id,
                undo_seconds: Some(0),
            },
        )
        .await
        .unwrap();
    let delivered = tokio::time::timeout(Duration::from_secs(20), sent.next())
        .await
        .expect("the forward reached the SMTP server");
    let files = regards_core::body::files(&delivered.data.unwrap());
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, "report.pdf");
    assert_eq!(files[0].data, b"%PDF-1.4\n");

    seed.logout().await;
    handle.abort();
}

#[tokio::test]
#[ignore = "needs a real IMAP server (REGARDS_TEST_IMAP, REGARDS_TEST_IMAP_CA)"]
async fn live_pin_and_set_aside_stay_on_the_device_and_can_be_undone() {
    let live = live();
    let mut seed = Session::login(&live.server, &live.user, &live.password, &live.tls)
        .await
        .unwrap();
    for (n, subject) in [(1, "Keep in view"), (2, "Later"), (3, "Plain")] {
        seed.append(b"INBOX", &[], &message(n, subject, "ada@example.test"))
            .await
            .unwrap();
    }
    let (_temp, mut desktop, account, inbox, handle) = daemon_with_account(&live).await;
    let listed = messages(&mut desktop, inbox).await.messages;
    let id = |subject: &str| listed.iter().find(|m| m.subject == subject).unwrap().id;
    let subjects = |page: MessageListResult| page.messages.into_iter().map(|m| m.subject).collect::<Vec<_>>();
    let view = |view: View| ViewParams {
        account_id: account.id.clone(),
        view,
        limit: None,
        before: None,
    };

    let pinned: MarkResult = desktop
        .call(
            methods::MESSAGE_PIN,
            PinParams {
                ids: vec![id("Keep in view")],
                pinned: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(pinned.changed, 1);
    assert_eq!(pinned.undo.unwrap().description, "Pinned");
    let page = messages(&mut desktop, inbox).await;
    assert!(
        page.messages
            .iter()
            .find(|m| m.subject == "Keep in view")
            .unwrap()
            .pinned
    );
    let in_view: MessageListResult = desktop.call(methods::MESSAGE_VIEW, view(View::Pinned)).await.unwrap();
    assert_eq!(subjects(in_view), ["Keep in view"]);
    let detail: MessageDetail = desktop
        .call(methods::MESSAGE_GET, MessageRef { id: id("Keep in view") })
        .await
        .unwrap();
    assert!(detail.message.pinned, "the reading pane knows");

    // Set aside: out of the inbox, in its view; undo brings it back.
    let aside: MarkResult = desktop
        .call(
            methods::MESSAGE_SET_ASIDE,
            AsideParams {
                ids: vec![id("Later")],
                aside: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(subjects(messages(&mut desktop, inbox).await), ["Plain", "Keep in view"]);
    let in_view: MessageListResult = desktop.call(methods::MESSAGE_VIEW, view(View::Aside)).await.unwrap();
    assert_eq!(subjects(in_view), ["Later"]);
    let undone: UndoResult = desktop
        .call(
            methods::UNDO_APPLY,
            UndoParams {
                id: aside.undo.unwrap().id,
            },
        )
        .await
        .unwrap();
    assert_eq!(undone.undone, 1);
    assert_eq!(
        subjects(messages(&mut desktop, inbox).await),
        ["Plain", "Later", "Keep in view"]
    );

    // Marks stay on the device: the server's flags did not change.
    seed.select(b"INBOX").await.unwrap();
    let flags = seed.fetch_flags("1:*", None).await.unwrap().items;
    assert!(flags.iter().all(|f| !f.flags.flagged && !f.flags.seen));
    // And they survive a sync.
    sync(&mut desktop, &account.id).await;
    let in_view: MessageListResult = desktop.call(methods::MESSAGE_VIEW, view(View::Pinned)).await.unwrap();
    assert_eq!(subjects(in_view), ["Keep in view"]);

    seed.logout().await;
    handle.abort();
}

#[tokio::test]
#[ignore = "needs a real IMAP server (REGARDS_TEST_IMAP, REGARDS_TEST_IMAP_CA)"]
async fn live_snoozed_mail_comes_back_as_new_or_when_someone_replies() {
    let live = live();
    let mut seed = Session::login(&live.server, &live.user, &live.password, &live.tls)
        .await
        .unwrap();
    seed.append(
        b"INBOX",
        &[SystemFlag::Seen],
        &message(1, "Read already", "ada@example.test"),
    )
    .await
    .unwrap();
    seed.append(b"INBOX", &[], &message(2, "Waiting for Bob", "ada@example.test"))
        .await
        .unwrap();
    seed.append(b"INBOX", &[], &message(3, "Stays", "ada@example.test"))
        .await
        .unwrap();
    let (_temp, mut desktop, account, inbox, handle) = daemon_with_account(&live).await;
    let listed = messages(&mut desktop, inbox).await.messages;
    let id = |subject: &str| listed.iter().find(|m| m.subject == subject).unwrap().id;
    let subjects = |page: MessageListResult| page.messages.into_iter().map(|m| m.subject).collect::<Vec<_>>();
    let now = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64
    };
    let snooze = |ids: Vec<i64>, until: Option<i64>, someday: bool| SnoozeParams { ids, until, someday };

    // In the past: refused.
    let past = desktop
        .call::<_, Value>(
            methods::MESSAGE_SNOOZE,
            snooze(vec![id("Stays")], Some(now() - 1000), false),
        )
        .await;
    assert!(past.is_err());

    // Snoozed for two seconds: out of the inbox, in its view, back unread when the time comes.
    let snoozed: MarkResult = desktop
        .call(
            methods::MESSAGE_SNOOZE,
            snooze(vec![id("Read already")], Some(now() + 2000), false),
        )
        .await
        .unwrap();
    assert_eq!(snoozed.undo.unwrap().description, "Snoozed");
    assert!(!subjects(messages(&mut desktop, inbox).await).contains(&"Read already".to_owned()));
    let view: MessageListResult = desktop
        .call(
            methods::MESSAGE_VIEW,
            ViewParams {
                account_id: account.id.clone(),
                view: View::Snoozed,
                limit: None,
                before: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(view.messages[0].subject, "Read already");
    assert!(view.messages[0].snoozed_until.is_some());
    let deadline = Instant::now() + Duration::from_secs(15);
    let back = loop {
        let page = messages(&mut desktop, inbox).await;
        if let Some(message) = page.messages.into_iter().find(|m| m.subject == "Read already") {
            break message;
        }
        assert!(Instant::now() < deadline, "the snooze did not end");
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    assert!(!back.seen, "back as new");
    assert_eq!(back.snoozed_until, None);
    // ...and unread on the server too.
    server_flags_become(&mut seed, |flags| flags.iter().all(|f| !f.flags.seen)).await;

    // Snoozed "someday": a reply from someone else brings it back.
    let _: MarkResult = desktop
        .call(methods::MESSAGE_SNOOZE, snooze(vec![id("Waiting for Bob")], None, true))
        .await
        .unwrap();
    assert!(!subjects(messages(&mut desktop, inbox).await).contains(&"Waiting for Bob".to_owned()));
    let reply = b"From: Bob <bob@example.test>\r\nTo: Live <live@example.test>\r\nSubject: Re: Waiting for Bob\r\n\
        Message-ID: <bob-reply@example.test>\r\nIn-Reply-To: <sync-2@example.test>\r\n\
        References: <sync-2@example.test>\r\nDate: Sun, 18 Oct 2026 12:00:00 +0000\r\n\r\nHere I am\r\n";
    seed.append(b"INBOX", &[], reply).await.unwrap();
    sync(&mut desktop, &account.id).await;
    let shown = subjects(messages(&mut desktop, inbox).await);
    assert!(shown.contains(&"Waiting for Bob".to_owned()), "{shown:?}");
    assert!(shown.contains(&"Re: Waiting for Bob".to_owned()));

    // Undo a snooze.
    let snoozed: MarkResult = desktop
        .call(
            methods::MESSAGE_SNOOZE,
            snooze(vec![id("Stays")], Some(now() + 3_600_000), false),
        )
        .await
        .unwrap();
    assert!(!subjects(messages(&mut desktop, inbox).await).contains(&"Stays".to_owned()));
    let _: UndoResult = desktop
        .call(
            methods::UNDO_APPLY,
            UndoParams {
                id: snoozed.undo.unwrap().id,
            },
        )
        .await
        .unwrap();
    assert!(subjects(messages(&mut desktop, inbox).await).contains(&"Stays".to_owned()));

    seed.logout().await;
    handle.abort();
}
