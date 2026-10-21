//! The connection log (Settings › Support): what Regards and the mail
//! servers say to each other, to diagnose a server that misbehaves. Off by
//! default; on, it goes to `logs/connections.log` and into diagnostics.
//!
//! It never holds mail, passwords, tokens or addresses. Callers record only
//! command names and replies' statuses (never a message's text, a folder's
//! name, a search or a recipient), sign-ins as `[redacted]`, and what a
//! server says passes through [`redact`] first.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// The log is started again past this size, one older copy kept.
const MAX_BYTES: u64 = 2 * 1024 * 1024;
/// What a server says is cut to this many characters.
const MAX_SERVER_TEXT: usize = 200;

static ON: AtomicBool = AtomicBool::new(false);
static SINK: Mutex<Option<Sink>> = Mutex::new(None);
static NEXT: AtomicU64 = AtomicU64::new(1);

struct Sink {
    path: PathBuf,
    file: File,
    written: u64,
}

/// Log connections to `path` from now on; `None` stops.
pub fn set(path: Option<PathBuf>) -> std::io::Result<()> {
    let mut sink = SINK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    *sink = match path {
        Some(path) => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let file = OpenOptions::new().create(true).append(true).open(&path)?;
            let written = file.metadata().map(|meta| meta.len()).unwrap_or(0);
            Some(Sink { path, file, written })
        }
        None => None,
    };
    ON.store(sink.is_some(), Ordering::SeqCst);
    Ok(())
}

pub fn enabled() -> bool {
    ON.load(Ordering::Relaxed)
}

/// One connection's lines, numbered so that several can be told apart.
#[derive(Debug)]
pub struct Conversation {
    id: u64,
    protocol: &'static str,
    host: String,
}

impl Conversation {
    /// `protocol`: "imap" or "smtp"; `host`: the server's name.
    pub fn new(protocol: &'static str, host: &str) -> Self {
        let conversation = Self {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            protocol,
            host: host.to_owned(),
        };
        conversation.note("connected");
        conversation
    }

    /// What Regards sent: a command's name and safe parts only.
    pub fn client(&self, line: &str) {
        self.write("C:", line);
    }

    /// What the server answered, with anything like an address masked.
    pub fn server(&self, line: &str) {
        if enabled() {
            self.write("S:", &redact(line));
        }
    }

    /// Something that happened: a TLS upgrade, an error, the end.
    pub fn note(&self, text: &str) {
        self.write("--", text);
    }

    fn write(&self, direction: &str, text: &str) {
        if !enabled() {
            return;
        }
        let line = format!(
            "{} [{}#{} {}] {direction} {text}\n",
            timestamp(),
            self.protocol,
            self.id,
            self.host
        );
        let mut sink = SINK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(open) = sink.as_mut() else { return };
        if open.written + line.len() as u64 > MAX_BYTES {
            let older = open.path.with_extension("log.1");
            let _ = std::fs::rename(&open.path, older);
            match OpenOptions::new().create(true).append(true).open(&open.path) {
                Ok(file) => {
                    open.file = file;
                    open.written = 0;
                }
                Err(_) => return,
            }
        }
        if open.file.write_all(line.as_bytes()).is_ok() {
            open.written += line.len() as u64;
        }
    }
}

impl Drop for Conversation {
    fn drop(&mut self) {
        self.note("closed");
    }
}

/// What a server says, made safe to keep: on one line, cut short, every
/// word with an `@` in it (an address, or a login name that is one)
/// replaced by `<address>`, and every IP address (this computer's, which
/// servers like to repeat) by `<ip>`.
pub fn redact(text: &str) -> String {
    let words: Vec<String> = text
        .split_whitespace()
        .map(|word| {
            let bare = word.trim_matches(|c: char| "[]()<>,;\"'".contains(c));
            if word.contains('@') {
                "<address>".to_owned()
            } else if bare.parse::<std::net::IpAddr>().is_ok() {
                "<ip>".to_owned()
            } else {
                word.to_owned()
            }
        })
        .collect();
    let mut line = words.join(" ");
    if line.chars().count() > MAX_SERVER_TEXT {
        line = line.chars().take(MAX_SERVER_TEXT).collect::<String>() + "…";
    }
    line
}

/// "2026-10-21 23:40:12.345Z".
fn timestamp() -> String {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0);
    let (secs, millis) = (ms.div_euclid(1000), ms.rem_euclid(1000));
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}.{millis:03}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_servers_say_keeps_no_address() {
        assert_eq!(
            redact("OK  Logged in as ada@example.org\r\n"),
            "OK Logged in as <address>"
        );
        assert_eq!(
            redact("250 2.1.5 <bob@example.net>... Recipient ok"),
            "250 2.1.5 <address> Recipient ok"
        );
        assert!(redact(&"x".repeat(500)).chars().count() <= MAX_SERVER_TEXT + 1);
        assert_eq!(
            redact("250-smtp.example.org at your service, [203.0.113.7]"),
            "250-smtp.example.org at your service, <ip>"
        );
        assert_eq!(redact("hello 2001:db8::1 and 12:30"), "hello <ip> and 12:30");
    }

    #[test]
    fn lines_go_to_the_file_only_while_on() {
        let temp = std::env::temp_dir().join(format!("regards-connlog-{}", std::process::id()));
        let path = temp.join("connections.log");
        let quiet = Conversation::new("imap", "imap.example.org");
        quiet.client("A1 CAPABILITY");
        set(Some(path.clone())).unwrap();
        let talk = Conversation::new("imap", "imap.example.org");
        talk.client("A2 LOGIN [redacted]");
        talk.server("A2 OK Logged in as ada@example.org");
        drop(talk);
        set(None).unwrap();
        quiet.client("A3 LOGOUT");
        let log = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_dir_all(&temp);
        assert!(log.contains("] C: A2 LOGIN [redacted]"), "{log}");
        assert!(log.contains("] S: A2 OK Logged in as <address>"), "{log}");
        assert!(log.contains("] -- closed"), "{log}");
        assert!(!log.contains("CAPABILITY") && !log.contains("LOGOUT"), "{log}");
        assert!(!log.contains("ada@"), "{log}");
        assert!(log.starts_with("20"), "dated: {log}");
    }
}
