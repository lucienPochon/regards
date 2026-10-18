//! Robustness against a hostile or broken server.
//!
//! A local plain-TCP server answers LOGIN and SELECT normally, then replies to
//! FETCH with malformed bytes. Each library must return an error or skip the
//! bad response: no panic, no hang, no process abort. Cases run in child
//! processes (see `main.rs`) because stack overflows and failed allocations
//! abort rather than panic.

use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use futures::StreamExt;
use imap_next::{
    client::Event,
    imap_types::{
        command::CommandBody,
        fetch::MessageDataItemName,
        mailbox::Mailbox,
        response::{Data, Status, StatusKind, Tagged},
        sequence::SequenceSet,
    },
    stream::{Error as StreamError, Stream},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
};

use crate::with_imap_next::Conn;

const GREETING: &[u8] = b"* OK [CAPABILITY IMAP4rev1] trial server ready\r\n";

const VALID_FETCH: &str = "* 1 FETCH (UID 1 FLAGS (\\Seen) ENVELOPE (\"Sun, 18 Oct 2026 10:00:00 +0000\" \"subject\" \
     ((\"A\" NIL \"a\" \"example.test\")) NIL NIL NIL NIL NIL NIL \"<id@example.test>\") \
     BODYSTRUCTURE (\"TEXT\" \"PLAIN\" (\"CHARSET\" \"utf-8\") NIL NIL \"7BIT\" 5 1))\r\n";

pub struct Case {
    pub name: String,
    greeting: Vec<u8>,
    /// Bytes sent in reply to FETCH; `{tag}` is replaced by the command tag.
    fetch_reply: Vec<u8>,
    timeout: Duration,
}

fn case(name: &str, greeting: &[u8], fetch_reply: impl Into<Vec<u8>>) -> Case {
    Case {
        name: name.into(),
        greeting: greeting.to_vec(),
        fetch_reply: fetch_reply.into(),
        timeout: Duration::from_secs(5),
    }
}

fn envelope_with_subject(subject: &[u8]) -> Vec<u8> {
    let mut out = b"* 1 FETCH (UID 1 ENVELOPE (NIL ".to_vec();
    out.extend_from_slice(subject);
    out.extend_from_slice(b" NIL NIL NIL NIL NIL NIL NIL NIL))\r\n{tag} OK done\r\n");
    out
}

pub fn fixed_cases() -> Vec<Case> {
    let nested = format!(
        "* 1 FETCH (BODYSTRUCTURE {}\"TEXT\"{})\r\n{{tag}} OK done\r\n",
        "(".repeat(100_000),
        ")".repeat(100_000)
    );
    let mut oversized = b"* OK ".to_vec();
    oversized.resize(128 * 1024 * 1024, b'A');
    oversized.extend_from_slice(b"\r\n{tag} OK done\r\n");
    let utf8 = "\"Gr\u{fc}\u{df}e \u{1f642}\"".as_bytes();
    let utf8_literal = format!("{{{}}}\r\nGr\u{fc}\u{df}e \u{1f642}", "Grüße 🙂".len());
    vec![
        case("binary garbage greeting", b"\x00\xff\xfeNOT IMAP AT ALL\r\n", ""),
        case(
            "unbalanced parentheses",
            GREETING,
            "* 1 FETCH (UID 1 FLAGS (\\Seen)\r\n{tag} OK done\r\n",
        ),
        case(
            "UID beyond u32",
            GREETING,
            "* 1 FETCH (UID 99999999999)\r\n{tag} OK done\r\n",
        ),
        case("UID 0 (invalid)", GREETING, "* 1 FETCH (UID 0)\r\n{tag} OK done\r\n"),
        case("sequence number 0", GREETING, "* 0 FETCH (UID 1)\r\n{tag} OK done\r\n"),
        case(
            "literal claims 1 TiB, then EOF",
            GREETING,
            "* 1 FETCH (BODY[] {1099511627776}\r\nabc",
        ),
        case("truncated mid-response, EOF", GREETING, "* 1 FETCH (UID"),
        case("LF-only line endings", GREETING, "* 1 FETCH (UID 1)\n{tag} OK done\n"),
        case("NUL inside quoted string", GREETING, envelope_with_subject(b"\"a\0b\"")),
        case(
            "invalid UTF-8 in quoted string",
            GREETING,
            envelope_with_subject(b"\"\xc3\x28\xff\""),
        ),
        case("valid UTF-8 in quoted string", GREETING, envelope_with_subject(utf8)),
        case(
            "Latin-1 8-bit in quoted string",
            GREETING,
            envelope_with_subject(b"\"Gr\xfc\xdfe\""),
        ),
        case(
            "valid UTF-8 as literal (correct form)",
            GREETING,
            envelope_with_subject(utf8_literal.as_bytes()),
        ),
        case(
            "one malformed FETCH between two valid",
            GREETING,
            "* 1 FETCH (UID 1)\r\n* 2 FETCH (UID 2 FLAGS (\\Seen)\r\n* 3 FETCH (UID 3)\r\n{tag} OK done\r\n",
        ),
        case("100k nested parentheses", GREETING, nested),
        Case {
            timeout: Duration::from_secs(60),
            ..case("128 MiB line", GREETING, oversized)
        },
        case(
            "tagged reply with wrong tag",
            GREETING,
            "* 1 FETCH (UID 1)\r\nZZZ OK done\r\n",
        ),
        case("EOF before any tagged reply", GREETING, "* 1 FETCH (UID 1)\r\n"),
    ]
}

/// Deterministic xorshift, so a failing seed can be replayed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

pub fn fuzz_case(seed: u64) -> Case {
    const INTERESTING: &[u8] = b"(){}[]\"\\ 0123456789*+-.\r\n\x00\xff\x80NIL";
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut bytes = VALID_FETCH.as_bytes().to_vec();
    for _ in 0..1 + rng.below(8) {
        let at = rng.below(bytes.len());
        match rng.below(4) {
            0 => bytes[at] = INTERESTING[rng.below(INTERESTING.len())],
            1 => bytes.insert(at, INTERESTING[rng.below(INTERESTING.len())]),
            2 => {
                bytes.remove(at);
            }
            _ => {
                let end = (at + 1 + rng.below(16)).min(bytes.len());
                let slice = bytes[at..end].to_vec();
                bytes.splice(at..at, slice);
            }
        }
    }
    bytes.extend_from_slice(b"{tag} OK done\r\n");
    case(&format!("fuzz seed {seed}"), GREETING, bytes)
}

/// Serves exactly one connection with the case's script, then closes it.
async fn serve(listener: TcpListener, case: &Case) -> Result<()> {
    let (socket, _) = listener.accept().await?;
    let (read, mut write) = socket.into_split();
    write.write_all(&case.greeting).await?;
    let mut lines = BufReader::new(read);
    let mut line = String::new();
    loop {
        line.clear();
        if lines.read_line(&mut line).await? == 0 {
            return Ok(());
        }
        let mut words = line.split_whitespace();
        let tag = words.next().unwrap_or("*").to_owned();
        let verb = words.next().unwrap_or("").to_ascii_uppercase();
        let reply: Vec<u8> = match verb.as_str() {
            "LOGIN" => format!("{tag} OK logged in\r\n").into_bytes(),
            "SELECT" => format!(
                "* 1 EXISTS\r\n* OK [UIDVALIDITY 1] ok\r\n* OK [UIDNEXT 2] ok\r\n{tag} OK [READ-WRITE] selected\r\n"
            )
            .into_bytes(),
            "UID" | "FETCH" => {
                let reply = replace(&case.fetch_reply, b"{tag}", tag.as_bytes());
                write.write_all(&reply).await?;
                write.shutdown().await?;
                return Ok(());
            }
            _ => format!("{tag} BAD unexpected\r\n").into_bytes(),
        };
        write.write_all(&reply).await?;
    }
}

fn replace(haystack: &[u8], needle: &[u8], with: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(haystack.len());
    let mut i = 0;
    while i < haystack.len() {
        if haystack[i..].starts_with(needle) {
            out.extend_from_slice(with);
            i += needle.len();
        } else {
            out.push(haystack[i]);
            i += 1;
        }
    }
    out
}

/// Outcome of the FETCH step: "accepted" (all parsed), "recovered" (some skipped,
/// command completed) or an error.
type ClientResult = Result<(&'static str, String)>;

async fn async_imap_client(addr: String) -> ClientResult {
    let tcp = TcpStream::connect(addr).await?;
    let mut client = async_imap::Client::new(tcp);
    client.read_response().await?.context("no greeting")?;
    let mut s = client.login("u", "p").await.map_err(|(e, _)| e)?;
    s.select("INBOX").await?;
    let mut stream = s.uid_fetch("1:*", "(UID FLAGS ENVELOPE BODYSTRUCTURE)").await?;
    let (mut fetched, mut errors, mut first_error) = (0, 0, None);
    while let Some(item) = stream.next().await {
        match item {
            Ok(_) => fetched += 1,
            Err(e) => {
                errors += 1;
                first_error.get_or_insert_with(|| format!("{e:#}"));
            }
        }
    }
    match (errors, first_error) {
        (0, _) => Ok(("accepted", format!("{fetched} fetch(es)"))),
        (_, Some(e)) if fetched == 0 => bail!(e),
        (_, e) => Ok((
            "recovered",
            format!("{fetched} fetch(es), {errors} error item(s): {}", e.unwrap_or_default()),
        )),
    }
}

async fn imap_next_client(addr: String) -> ClientResult {
    let tcp = TcpStream::connect(addr).await?;
    let mut c = Conn::open(Stream::insecure(tcp)).await?;
    c.ok(CommandBody::login("u", "p")?).await?;
    c.ok(CommandBody::Select {
        mailbox: Mailbox::Inbox,
        parameters: vec![],
    })
    .await?;
    use MessageDataItemName as N;
    let tag = c.enqueue(CommandBody::Fetch {
        sequence_set: SequenceSet::try_from("1:*").map_err(|e| anyhow::anyhow!("{e:?}"))?,
        macro_or_item_names: vec![N::Uid, N::Flags, N::Envelope, N::BodyStructure].into(),
        uid: true,
        modifiers: vec![],
    });
    // Like a real engine: skip a malformed response and keep reading.
    let (mut fetched, mut skipped, mut first_error) = (0, 0, None);
    loop {
        match c.stream.next(&mut c.client).await {
            Ok(Event::DataReceived {
                data: Data::Fetch { .. },
            }) => fetched += 1,
            Ok(Event::StatusReceived {
                status: Status::Tagged(Tagged { tag: t, body }),
            }) if t == tag => {
                let ok = if body.kind == StatusKind::Ok {
                    "tagged OK"
                } else {
                    "tagged not OK"
                };
                return match skipped {
                    0 => Ok(("accepted", format!("{fetched} fetch(es), {ok}"))),
                    _ => Ok((
                        "recovered",
                        format!(
                            "{fetched} fetch(es), {skipped} skipped, {ok}: {}",
                            first_error.unwrap_or_default()
                        ),
                    )),
                };
            }
            Ok(_) => {}
            Err(StreamError::State(e)) => {
                skipped += 1;
                first_error.get_or_insert_with(|| e.to_string());
            }
            Err(e) => match first_error {
                Some(first) => bail!("{e} after skipping {skipped}: {first}"),
                None => bail!(e),
            },
        }
    }
}

static LAST_PANIC: Mutex<Option<String>> = Mutex::new(None);

pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        *LAST_PANIC.lock().unwrap_or_else(|e| e.into_inner()) = Some(format!("{payload} at {location}"));
    }));
}

/// Runs one case against one library; returns the outcome and detail.
pub async fn run_case(lib: &str, case: Case) -> (String, String) {
    let listener = match TcpListener::bind("127.0.0.1:0").await {
        Ok(l) => l,
        Err(e) => return ("SETUP".into(), e.to_string()),
    };
    let addr = listener.local_addr().map(|a| a.to_string()).unwrap_or_default();
    let timeout = case.timeout;
    let server = tokio::spawn(async move { serve(listener, &case).await });
    let mut client = match lib {
        "async-imap" => tokio::spawn(async_imap_client(addr)),
        _ => tokio::spawn(imap_next_client(addr)),
    };
    let started = Instant::now();
    let joined = tokio::time::timeout(timeout, &mut client).await;
    let ms = started.elapsed().as_millis();
    client.abort();
    server.abort();
    match joined {
        Err(_) => ("HANG".into(), format!("no result within {} s", timeout.as_secs())),
        Ok(Err(join)) if join.is_panic() => (
            "PANIC".into(),
            LAST_PANIC
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
                .unwrap_or_default(),
        ),
        Ok(Err(join)) => ("SETUP".into(), join.to_string()),
        Ok(Ok(Err(e))) => ("error".into(), format!("{e:#} ({ms} ms)")),
        Ok(Ok(Ok((outcome, note)))) => (outcome.into(), format!("{note} ({ms} ms)")),
    }
}
