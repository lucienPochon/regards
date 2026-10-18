//! A broken or hostile server must never crash or hang the client (plan
//! §5.2: "no panics on malformed input"). Every command is answered with
//! each of a set of bad replies, after which the server hangs up; every call
//! must come back, with a result or an error, quickly.

#[path = "support/fake_imap.rs"]
mod fake_imap;

use fake_imap::{PASSWORD, Pki, Script, pki, serve};
use regards_core::secret::Secret;
use regards_imap::{ImapError, ImapServer, MessageSet, Session, SystemFlag, TlsOptions};
use std::time::Duration;

/// Bad replies; `{tag}` becomes the command's tag.
fn replies() -> Vec<(&'static str, &'static [u8])> {
    // Longer than any sensible line, and never finished.
    let endless: &'static [u8] = Box::leak(vec![b'a'; 2 * 1024 * 1024].into_boxed_slice());
    vec![
        ("binary noise", b"\x00\xff\xfe noise \x80\x81\x1b[2J\r\n"),
        ("a 4 GB literal that never comes", b"* 1 FETCH (UID 5 BODY[] {4294967295}\r\nshort"),
        ("a literal shorter than announced", b"* 1 FETCH (UID 1 BODY[] {50}\r\nab"),
        ("a truncated response", b"* 1 FETCH (UID 5 FLAGS (\\Seen"),
        ("an endless line", endless),
        ("a reply to someone else", b"A999 OK not yours\r\n"),
        ("BYE in the middle", b"* BYE going away\r\n"),
        ("an unexpected continuation", b"+ more please\r\n"),
        (
            "unknown and malformed data, then OK",
            b"* XYZZY 1 2 3\r\n* 1 FETCH (UID 1 FLAGS (\\Seen (()) )\r\n* 2 FETCH (UID 2 FLAGS ())\r\n{tag} OK done\r\n",
        ),
        (
            "numbers too large for their fields",
            b"* 99999999999999999999 EXISTS\r\n* OK [UIDVALIDITY 99999999999] x\r\n{tag} OK done\r\n",
        ),
        ("a zero UIDVALIDITY", b"* 1 EXISTS\r\n* OK [UIDVALIDITY 0] x\r\n{tag} OK done\r\n"),
        ("an invalid mailbox name", b"* LIST () \"/\" \"\xff\xfe\"\r\n* LIST (\\Noselect) NIL {3}\r\n\x00\x01\x02\r\n{tag} OK done\r\n"),
        ("a search with junk", b"* SEARCH 1 2 x 4 -5\r\n{tag} OK done\r\n"),
        ("OK with nothing", b"{tag} OK done\r\n"),
        ("NO and BAD", b"{tag} NO [ALERT] \xff\xfe broken\r\n"),
        ("a body with garbage headers", b"* 1 FETCH (UID 1 RFC822.SIZE -1 INTERNALDATE \"nonsense\" BODY[HEADER] {9}\r\n\x00\x01:\xff\r\n\r\n)\r\n{tag} OK done\r\n"),
    ]
}

/// One command, and the verb the fake server sees for it.
#[derive(Debug, Clone, Copy)]
enum Command {
    List,
    Select,
    Search,
    FetchHeadersByUid,
    FetchHeadersBySequence,
    FetchFlags,
    FetchSource,
    Store,
    Move,
    Idle,
}

impl Command {
    const ALL: [Command; 10] = [
        Command::List,
        Command::Select,
        Command::Search,
        Command::FetchHeadersByUid,
        Command::FetchHeadersBySequence,
        Command::FetchFlags,
        Command::FetchSource,
        Command::Store,
        Command::Move,
        Command::Idle,
    ];

    fn verb(self) -> &'static str {
        match self {
            Command::List => "LIST",
            Command::Select => "SELECT",
            Command::FetchHeadersBySequence => "FETCH",
            Command::Idle => "IDLE",
            _ => "UID",
        }
    }

    /// Run it; whatever comes back is fine as long as something does.
    async fn run(self, session: &mut Session) -> Result<(), ImapError> {
        if !matches!(self, Command::List | Command::Select) {
            session.select(b"INBOX").await?;
        }
        match self {
            Command::List => session.list_mailboxes().await.map(drop),
            Command::Select => session.select(b"INBOX").await.map(drop),
            Command::Search => session.uid_search_all().await.map(drop),
            Command::FetchHeadersByUid => session.fetch_headers(MessageSet::Uids("1:*".into())).await.map(drop),
            Command::FetchHeadersBySequence => session
                .fetch_headers(MessageSet::Sequence("1:*".into()))
                .await
                .map(drop),
            Command::FetchFlags => session.fetch_flags("1:*", None).await.map(drop),
            Command::FetchSource => session.fetch_source(1).await.map(drop),
            Command::Store => session.store_flags("1", true, &[SystemFlag::Seen]).await,
            Command::Move => session.move_messages("1", b"Archive").await.map(drop),
            Command::Idle => session
                .idle(Duration::from_secs(5), std::future::pending())
                .await
                .map(drop),
        }
    }
}

fn trusting(pki: &Pki) -> TlsOptions {
    TlsOptions::platform()
        .with_extra_roots_pem(pki.ca_pem.as_bytes())
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn bad_replies_to_any_command_end_in_a_result_or_an_error() {
    let pki = pki(&["127.0.0.1"]);
    let tls = trusting(&pki);
    let mut outcomes = Vec::new();
    for (name, reply) in replies() {
        for command in Command::ALL {
            let script = Script {
                hostile: Some((command.verb(), reply)),
                ..Script::new(PASSWORD)
            };
            let (port, _) = serve(&pki, script).await;
            let server = ImapServer {
                host: "127.0.0.1".into(),
                port,
            };
            let outcome = tokio::time::timeout(Duration::from_secs(10), async {
                let mut session = Session::login(&server, "ada", &Secret::new(PASSWORD), &tls).await?;
                command.run(&mut session).await
            })
            .await
            .unwrap_or_else(|_| panic!("{command:?} hung on {name}"));
            outcomes.push((name, command, outcome.is_ok()));
        }
    }
    // Hanging up always ends in an error, except where the server finished the reply first.
    for (name, command, ok) in &outcomes {
        if *ok {
            assert!(
                name.ends_with("then OK")
                    || name.contains("too large")
                    || name.contains("zero UIDVALIDITY")
                    || name.contains("invalid mailbox")
                    || name.contains("junk")
                    || name.contains("nothing")
                    || name.contains("garbage headers"),
                "{command:?} succeeded on {name}"
            );
        }
    }
}

#[tokio::test]
async fn malformed_fetch_lines_are_skipped_and_counted() {
    let pki = pki(&["127.0.0.1"]);
    let script = Script {
        hostile: Some((
            "UID",
            b"* 1 FETCH (UID 1 FLAGS (\\Seen (()) )\r\n* 2 FETCH (UID 2 FLAGS (\\Flagged))\r\n{tag} OK done\r\n",
        )),
        ..Script::new(PASSWORD)
    };
    let (port, _) = serve(&pki, script).await;
    let server = ImapServer {
        host: "127.0.0.1".into(),
        port,
    };
    let mut session = Session::login(&server, "ada", &Secret::new(PASSWORD), &trusting(&pki))
        .await
        .unwrap();
    session.select(b"INBOX").await.unwrap();
    let fetched = session.fetch_flags("1:*", None).await.unwrap();
    assert_eq!(fetched.skipped, 1);
    assert_eq!(fetched.items.len(), 1);
    assert_eq!(fetched.items[0].uid, 2);
    assert!(fetched.items[0].flags.flagged);
}
