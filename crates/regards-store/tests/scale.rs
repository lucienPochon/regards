//! The 100k-message fixture (plan §5.2, Phase 1 acceptance): a mailbox that
//! large must stay responsive. Opt-in, and meaningful in release builds:
//! `cargo test -p regards-store --release --test scale -- --ignored --nocapture`

use regards_core::headers::Headers;
use regards_core::mailbox::Role;
use regards_core::paths::DataDirs;
use regards_store::Store;
use regards_store::accounts::{self, AuthKind, NewAccount, Security, Server};
use regards_store::mailboxes::{self, RemoteMailbox};
use regards_store::marks::{self, Mark};
use regards_store::messages::{self, Cursor, MessageFlags, NewMessage};
use regards_store::operations::{self, FlagName};
use std::time::{Duration, Instant};

const MESSAGES: u32 = 100_000;
const BATCH: u32 = 2_000;

fn message(uid: u32) -> NewMessage {
    // A third of the messages answer one of the 20 before them.
    let parent = (uid.is_multiple_of(3) && uid > 20).then(|| format!("m{}@example.test", uid - 1 - uid % 20));
    NewMessage {
        uid,
        modseq: Some(u64::from(uid)),
        flags: MessageFlags {
            seen: !uid.is_multiple_of(4),
            ..MessageFlags::default()
        },
        received_at: Some(1_700_000_000_000 + i64::from(uid) * 60_000),
        size: Some(4_000),
        headers: Headers {
            subject: format!("Message {uid}"),
            message_id: Some(format!("m{uid}@example.test")),
            in_reply_to: parent.iter().cloned().collect(),
            references: parent.into_iter().collect(),
            ..Headers::default()
        },
    }
}

/// Run `f` once cold, then five times; the slowest warm run must fit `budget`
/// and the cold one twice that.
async fn time<T, F, Fut>(label: &str, budget: Duration, mut f: F) -> T
where
    F: FnMut() -> Fut,
    Fut: Future<Output = T>,
{
    let start = Instant::now();
    let mut result = f().await;
    let cold = start.elapsed();
    let mut slowest = Duration::ZERO;
    for _ in 0..5 {
        let start = Instant::now();
        result = f().await;
        slowest = slowest.max(start.elapsed());
    }
    println!("{label:<40} {slowest:>10.2?}  (cold {cold:.2?}, budget {budget:?})");
    assert!(slowest <= budget, "{label} took {slowest:?}, over {budget:?}");
    assert!(cold <= budget * 2, "{label} took {cold:?} cold, over {:?}", budget * 2);
    result
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "benchmark: cargo test -p regards-store --release --test scale -- --ignored --nocapture"]
async fn bench_a_100k_message_mailbox_stays_responsive() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
    let account = accounts::new_id();
    let id = account.clone();
    let inbox = store
        .write(move |tx| {
            let server = Server {
                host: "h".into(),
                port: 993,
                security: Security::Tls,
            };
            accounts::insert(
                tx,
                NewAccount {
                    id: id.clone(),
                    display_name: "Scale".into(),
                    email: "scale@example.test".into(),
                    auth_kind: AuthKind::Password,
                    username: "scale".into(),
                    secret_key: "k".into(),
                    imap: server.clone(),
                    smtp: server,
                    imap_capabilities: vec![],
                },
            )?;
            mailboxes::reconcile(
                tx,
                &id,
                &[RemoteMailbox {
                    remote_name: b"INBOX".to_vec(),
                    name: "INBOX".into(),
                    delimiter: None,
                    role: Some(Role::Inbox),
                    selectable: true,
                    attributes: vec![],
                }],
            )?;
            Ok(mailboxes::list(tx, &id)?[0].id)
        })
        .await
        .unwrap()
        .value;

    // Filling it the way sync does: batches of headers, threaded as they are stored.
    let start = Instant::now();
    let mut slowest_batch = Duration::ZERO;
    for first in (1..=MESSAGES).step_by(BATCH as usize) {
        let batch: Vec<NewMessage> = (first..first + BATCH).map(message).collect();
        let began = Instant::now();
        store
            .write(move |tx| messages::upsert(tx, inbox, &batch))
            .await
            .unwrap();
        slowest_batch = slowest_batch.max(began.elapsed());
    }
    println!(
        "{:<40} {:>10.2?}  (slowest batch of {BATCH}: {slowest_batch:.2?})",
        format!("store {MESSAGES} messages"),
        start.elapsed()
    );
    // A sync's batch holds the writer: other writes wait at most this long.
    assert!(slowest_batch < Duration::from_secs(2), "a batch took {slowest_batch:?}");

    // What the app does all the time.
    let page = time("first page (50 newest)", Duration::from_millis(20), || {
        store.read(move |conn| messages::list(conn, inbox, 50, None))
    })
    .await
    .unwrap();
    assert_eq!(page[0].uid, MESSAGES);

    // The inbox as shown: what is set aside or snoozed stays out (every 10th of the newest 1,000 here).
    let a = account.clone();
    store
        .write(move |tx| {
            for uid in (MESSAGES - 999..=MESSAGES).step_by(10) {
                marks::set(tx, &a, &format!("m{uid}@example.test"), Mark::Aside, Some(1))?;
            }
            Ok(())
        })
        .await
        .unwrap();
    let shown = time("inbox page, 100 set aside", Duration::from_millis(20), || {
        let a = account.clone();
        store.read(move |conn| messages::list_without_parked(conn, inbox, 50, None, Some((&a, 0))))
    })
    .await
    .unwrap();
    assert_eq!(shown.len(), 50);
    // 99001, 99011, … were set aside.
    assert!(shown.iter().all(|m| m.uid % 10 != 1), "the set-aside ones are left out");
    time("the Set aside view", Duration::from_millis(20), || {
        let a = account.clone();
        store.read(move |conn| messages::marked(conn, &a, Mark::Aside, 0, 50, None))
    })
    .await
    .unwrap();

    let middle = Cursor {
        received_at: 1_700_000_000_000 + i64::from(MESSAGES / 2) * 60_000,
        id: i64::MAX,
    };
    time("a page halfway down", Duration::from_millis(20), || {
        store.read(move |conn| messages::list(conn, inbox, 50, Some(middle)))
    })
    .await
    .unwrap();

    // Every third message is a reply.
    let thread = page
        .iter()
        .find(|m| m.uid.is_multiple_of(3))
        .and_then(|m| m.thread_id)
        .unwrap();
    let conversation = time("open a conversation", Duration::from_millis(20), || {
        store.read(move |conn| messages::in_thread(conn, thread))
    })
    .await
    .unwrap();
    assert!(conversation.len() > 1, "replies were threaded");

    let newest = page[0].id;
    time("open a message's row", Duration::from_millis(5), || {
        store.read(move |conn| messages::get(conn, newest))
    })
    .await
    .unwrap();

    // What every sync of the mailbox reads.
    time("all stored UIDs (each sync)", Duration::from_millis(150), || {
        store.read(move |conn| messages::uids(conn, inbox))
    })
    .await
    .unwrap();

    // An action: mark 50 read, with its queue entry, in one write.
    let uids: Vec<u32> = (MESSAGES - 49..=MESSAGES).collect();
    time("mark 50 messages read", Duration::from_millis(50), || {
        let uids = uids.clone();
        store.write(move |tx| operations::apply_locally(tx, inbox, &uids, &[FlagName::Seen], true))
    })
    .await
    .unwrap();

    time("reopen the store", Duration::from_millis(500), || async {
        Store::open(&DataDirs::at(temp.path())).await.map(drop)
    })
    .await
    .unwrap();
}
