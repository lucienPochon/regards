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

    // Old mail that came back (snoozes that ended): every 10th of the oldest 1,000, now on top.
    let a = account.clone();
    store
        .write(move |tx| {
            for uid in (1..=1000).step_by(10) {
                let back = 1_900_000_000_000 + i64::from(uid);
                marks::set(tx, &a, &format!("m{uid}@example.test"), Mark::Surfaced, Some(back))?;
            }
            Ok(())
        })
        .await
        .unwrap();
    let shown = time("inbox page, 100 came back", Duration::from_millis(20), || {
        let a = account.clone();
        store.read(move |conn| messages::list_without_parked(conn, inbox, 50, None, Some((&a, 0))))
    })
    .await
    .unwrap();
    assert_eq!(shown[0].uid, 991, "the last to come back is on top");
    assert!(shown.iter().all(|m| m.uid <= 1000), "what came back comes first");

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

/// Deterministic noise for the search fixture.
struct Noise(u64);

impl Noise {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    /// A word of a 5,000-word language, the first ones far more common (roughly Zipf).
    fn word(&mut self) -> String {
        let u = (self.next() % 1_000_000) as f64 / 1_000_000.0;
        word_of((5_000.0 * u.powi(3)) as u64)
    }

    fn words(&mut self, count: usize) -> String {
        (0..count).map(|_| self.word()).collect::<Vec<_>>().join(" ")
    }
}

/// The word of rank `rank` in the fixture's language: `kalo` is the commonest.
fn word_of(rank: u64) -> String {
    const SYLLABLES: [&str; 20] = [
        "ka", "lo", "mi", "ne", "ru", "sa", "ti", "vo", "be", "da", "fe", "go", "hu", "ji", "pe", "qua", "ro", "si",
        "to", "ze",
    ];
    let mut word = String::new();
    let mut n = rank + 20;
    while n > 0 {
        word.push_str(SYLLABLES[(n % 20) as usize]);
        n /= 20;
    }
    word
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "benchmark: cargo test -p regards-store --release --test scale -- --ignored --nocapture"]
async fn bench_searching_100k_messages() {
    use regards_core::headers::Person;
    use regards_core::search::parse;
    use regards_store::search;

    let temp = tempfile::tempdir().unwrap();
    let dirs = DataDirs::at(temp.path());
    let store = Store::open(&dirs).await.unwrap();
    let id = accounts::new_id();
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

    // Mail as sync stores it, with its list preview: indexed as it is written.
    let mut noise = Noise(7);
    let start = Instant::now();
    let mut slowest_batch = Duration::ZERO;
    for first in (1..=MESSAGES).step_by(BATCH as usize) {
        let batch: Vec<(NewMessage, String)> = (first..first + BATCH)
            .map(|uid| {
                let sender = noise.next() % 500;
                let mut message = message(uid);
                message.headers.subject = noise.words(5);
                message.headers.from = Some(Person {
                    name: Some(format!("Person {sender}")),
                    address: Some(format!("p{sender}@{}.example", noise.word())),
                });
                message.headers.to = vec![Person {
                    name: Some("Scale".into()),
                    address: Some("scale@example.test".into()),
                }];
                (message, noise.words(30))
            })
            .collect();
        let began = Instant::now();
        store
            .write(move |tx| {
                let messages: Vec<NewMessage> = batch.iter().map(|(m, _)| m.clone()).collect();
                messages::upsert(tx, inbox, &messages)?;
                for (message, preview) in &batch {
                    messages::set_details(tx, inbox, message.uid, message.uid % 7 == 0, Some(preview), &[])?;
                }
                Ok(())
            })
            .await
            .unwrap();
        slowest_batch = slowest_batch.max(began.elapsed());
    }
    println!(
        "{:<40} {:>10.2?}  (slowest batch of {BATCH}: {slowest_batch:.2?})",
        format!("store {MESSAGES} messages, queued"),
        start.elapsed()
    );
    assert!(slowest_batch < Duration::from_secs(2), "a batch took {slowest_batch:?}");
    index_all(&store, "index them").await;

    let budget = Duration::from_millis(150);
    let mut found = Vec::new();
    let (rare, some) = (word_of(3_000), word_of(500));
    for (label, query) in [
        ("search: a rare word (~300 messages)", rare.as_str()),
        ("search: a word in ~1,000 messages", some.as_str()),
        ("search: a common word (~88%)", "kalo"),
        ("search: two words", "kalo lomi"),
        ("search: two letters typed", "ka"),
        ("search: from a sender", "from:p42"),
        ("search: a word, unread, with files", "kalo is:unread has:attachment"),
        ("search: unread, no words", "is:unread"),
        ("search: a phrase", r#""kalo lomi""#),
        // The worst case: nothing passes, so the whole mailbox is walked.
        ("search: flagged (none are)", "kalo is:flagged"),
    ] {
        let query = parse(query);
        let hits = time(label, budget, || {
            let query = query.clone();
            store.read(move |conn| search::search(conn, &query, None, 50, None))
        })
        .await
        .unwrap();
        found.push((label, hits.page.len()));
    }
    println!("{found:?}");
    assert!(
        found[..found.len() - 1].iter().all(|(_, n)| *n > 0),
        "every query finds something"
    );

    // Did you mean: a word typed with a letter too many.
    let typo = format!("{rare}x");
    let suggested = time("suggest: a word typed wrong", budget, || {
        let typo = typo.clone();
        store.read(move |conn| search::suggest(conn, &typo, None))
    })
    .await
    .unwrap();
    println!("{typo} → {suggested:?}");
    assert!(suggested.is_some());

    // A new index schema: rebuilt from what is stored.
    drop(store);
    {
        let store = Store::open(&dirs).await.unwrap();
        store
            .write(|tx| Ok(tx.execute_batch("PRAGMA search.user_version = 0")?))
            .await
            .unwrap();
    }
    let store = Store::open(&dirs).await.unwrap();
    index_all(&store, "rebuild the index").await;
}

/// Work off the search queue as the daemon does, in batches of 500.
async fn index_all(store: &Store, label: &str) {
    let start = Instant::now();
    let mut slowest_batch = Duration::ZERO;
    loop {
        let began = Instant::now();
        let left = store
            .write(|tx| regards_store::search::index(tx, 500))
            .await
            .unwrap()
            .value;
        slowest_batch = slowest_batch.max(began.elapsed());
        if left == 0 {
            break;
        }
    }
    println!(
        "{label:<40} {:>10.2?}  (slowest batch of 500: {slowest_batch:.2?})",
        start.elapsed()
    );
    // It holds the writer: other writes wait at most this long.
    assert!(
        slowest_batch < Duration::from_millis(500),
        "an index batch took {slowest_batch:?}"
    );
}
