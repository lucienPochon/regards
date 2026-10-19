//! What the code may reach (ADR-0015 §3, and the privacy statement): an
//! egress allow-list, checked over the source.
//!
//! - **Fixed addresses:** every web address written in the code (outside
//!   tests and comments) is one of [`ALLOWED`].
//! - **Connections:** only `regards-net` opens them, so what the person set
//!   up (mail servers, DNS, the servers an address suggests) goes through
//!   one place, with TLS always verified.
//!
//! Adding a destination means adding it here and to `docs/privacy.md`.

use std::path::{Path, PathBuf};

/// Hosts the code may name, and why.
const ALLOWED: &[(&str, &str)] = &[
    ("login.microsoftonline.com", "Microsoft sign-in (OAuth)"),
    ("accounts.google.com", "Google sign-in (OAuth)"),
    ("oauth2.googleapis.com", "Google tokens (OAuth)"),
    ("outlook.office.com", "a Microsoft OAuth scope's name, not a connection"),
    ("mail.google.com", "a Google OAuth scope's name, not a connection"),
    ("api.github.com", "the update check, when the person turns it on"),
    (
        "api.anthropic.com",
        "the assistant, when the person chooses Anthropic with their key",
    ),
    ("github.com", "release pages, opened in the browser"),
    ("localhost", "this computer"),
    ("127.0.0.1", "this computer"),
    ("regards-mail.localhost", "the reading frame, inside the app"),
];

/// Reserved for examples and tests (RFC 2606, RFC 6761).
fn reserved(host: &str) -> bool {
    [".test", ".example", ".invalid", ".localhost"]
        .iter()
        .any(|end| host.ends_with(end))
        || ["example.org", "example.com", "example.net"].contains(&host)
}

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// The Rust sources that ship: each crate's and the desktop app's `src`.
fn sources() -> Vec<PathBuf> {
    fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, found);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                found.push(path);
            }
        }
    }
    let root = workspace();
    let mut found = Vec::new();
    if let Ok(crates) = std::fs::read_dir(root.join("crates")) {
        for krate in crates.flatten() {
            walk(&krate.path().join("src"), &mut found);
        }
    }
    walk(
        &root.join("apps").join("desktop").join("src-tauri").join("src"),
        &mut found,
    );
    found
}

/// The code of a file as it ships: without its tests and comments.
fn shipped(text: &str) -> String {
    let code = text.split("#[cfg(test)]").next().unwrap_or(text);
    code.lines()
        .filter(|line| {
            let line = line.trim_start();
            !line.starts_with("//")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn hosts(code: &str) -> Vec<String> {
    let mut found = Vec::new();
    for scheme in ["https://", "http://"] {
        let mut rest = code;
        while let Some(at) = rest.find(scheme) {
            rest = &rest[at + scheme.len()..];
            let host: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
                .collect();
            if !host.is_empty() {
                found.push(host.to_ascii_lowercase());
            }
        }
    }
    found
}

#[test]
fn the_code_names_only_destinations_the_privacy_statement_lists() {
    let files = sources();
    assert!(files.len() > 50, "the sources were found ({})", files.len());
    let mut unknown = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file).unwrap();
        for host in hosts(&shipped(&text)) {
            if !reserved(&host) && !ALLOWED.iter().any(|(allowed, _)| *allowed == host) {
                unknown.push(format!("{host} in {}", file.display()));
            }
        }
    }
    assert!(
        unknown.is_empty(),
        "not on the egress allow-list (add it here and to docs/privacy.md, or remove it):\n{}",
        unknown.join("\n")
    );
}

#[test]
fn only_regards_net_opens_connections() {
    let mut elsewhere = Vec::new();
    for file in sources() {
        let path = file.to_string_lossy().replace('\\', "/");
        if path.contains("/crates/regards-net/") {
            continue;
        }
        let code = shipped(&std::fs::read_to_string(&file).unwrap());
        for primitive in ["TcpStream::connect", "UdpSocket::bind", "reqwest::", "ureq::"] {
            if code.contains(primitive) {
                elsewhere.push(format!("{primitive} in {path}"));
            }
        }
    }
    assert!(
        elsewhere.is_empty(),
        "connections outside regards-net:\n{}",
        elsewhere.join("\n")
    );
}

#[test]
fn the_privacy_statement_names_each_outside_destination() {
    let statement = std::fs::read_to_string(workspace().join("docs").join("privacy.md")).unwrap();
    for needle in ["Microsoft or Google sign-in", "api.github.com", "DNS"] {
        assert!(statement.contains(needle), "docs/privacy.md does not mention {needle}");
    }
}

#[test]
fn the_check_sees_addresses_in_code_but_not_in_comments_or_tests() {
    let file = "const A: &str = \"https://cdn.tracker.com/x\";\n// see https://docs.example.org\n\
                #[cfg(test)]\nmod tests { const B: &str = \"https://elsewhere.com\"; }";
    assert_eq!(hosts(&shipped(file)), ["cdn.tracker.com"]);
    assert!(!reserved("cdn.tracker.com") && reserved("mail.example.test"));
}
