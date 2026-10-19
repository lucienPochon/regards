//! The mail commands as agents run them: the real `regards` binary against
//! a daemon on a scratch data directory. What they find is tested in the
//! daemon; here, that each command reaches it and answers as documented.

use regards_core::paths::{DATA_DIR_ENV, DataDirs};
use regards_daemon::Daemon;
use serde_json::Value;
use std::path::Path;

/// `regards --json <args>`: its exit code and what it printed.
async fn regards(data: &Path, args: &[&str]) -> (i32, Value) {
    let (data, args): (std::path::PathBuf, Vec<String>) =
        (data.to_owned(), args.iter().map(|a| (*a).to_owned()).collect());
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new(env!("CARGO_BIN_EXE_regards"))
            .arg("--json")
            .args(&args)
            .env(DATA_DIR_ENV, &data)
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    let printed = String::from_utf8(output.stdout).unwrap();
    let value = serde_json::from_str(printed.trim()).unwrap_or_else(|_| panic!("not JSON: {printed:?}"));
    (output.status.code().unwrap_or(-1), value)
}

#[tokio::test(flavor = "multi_thread")]
async fn the_mail_commands_reach_the_daemon_and_answer_in_json() {
    let temp = tempfile::tempdir().unwrap();
    let daemon = Daemon::bind(DataDirs::at(temp.path())).await.unwrap();
    let handle = tokio::spawn(daemon.serve(std::future::pending()));
    let data = temp.path();

    // Nothing open to agents: nothing listed, nothing found.
    let (code, accounts) = regards(data, &["accounts"]).await;
    assert_eq!((code, accounts), (0, serde_json::json!({ "accounts": [] })));
    let (code, found) = regards(data, &["search", "garden", "from:lena"]).await;
    assert_eq!(code, 0);
    assert_eq!(found["messages"], serde_json::json!([]));

    // What does not exist, or is not allowed: an error, as JSON, and its exit code.
    let (code, missing) = regards(data, &["read", "999"]).await;
    assert_eq!(code, 6);
    assert_eq!(missing["error"]["code"], -32004);
    let (code, refused) = regards(data, &["folders", "--account-id", "nobody"]).await;
    assert_eq!((code, refused["error"]["code"].as_i64()), (6, Some(-32004)));
    let (code, unsent) = regards(data, &["send", "1"]).await;
    assert_eq!((code, unsent["error"]["message"].as_str()), (6, Some("no such draft")));

    // Checked before asking the daemon.
    let (code, unclear) = regards(data, &["mark", "4"]).await;
    assert_eq!(
        (code, unclear["error"]["message"].as_str()),
        (2, Some("say read, flagged or both"))
    );
    let (code, unclear) = regards(data, &["snooze", "4", "--until", "soon"]).await;
    assert_eq!(code, 2, "{unclear}");

    handle.abort();
}
