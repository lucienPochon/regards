//! The launcher against the real `regards` binary: start, reuse and replace.

use regards_core::paths::{DATA_DIR_ENV, DataDirs};
use regards_ipc::launcher::{LaunchError, spawn_daemon};
use regards_ipc::protocol::{ClientKind, Principal, codes};
use regards_ipc::{Client, ClientError, Launcher};
use std::path::PathBuf;

fn program() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_regards"))
}

/// Stops a daemon left behind by a failed assertion, before the temp dir goes.
struct StopOnDrop(DataDirs);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        let _ = std::process::Command::new(program())
            .args(["daemon", "stop"])
            .env(DATA_DIR_ENV, self.0.data())
            .output();
    }
}

#[tokio::test]
async fn desktop_starts_reuses_and_replaces_the_daemon() {
    let temp = tempfile::tempdir().unwrap();
    let dirs = DataDirs::at(temp.path());
    let _guard = StopOnDrop(dirs.clone());
    let launcher = Launcher::new(dirs.clone(), program());

    // Nothing running: the desktop starts a daemon and gets the ui principal.
    let first = launcher
        .connect_or_start(ClientKind::Desktop, None, true)
        .await
        .unwrap();
    let launched = first.launched.clone().expect("a daemon was started");
    assert!(!first.replaced);
    assert_eq!(first.client.principal(), Principal::Ui);
    assert_ne!(launched.pid, std::process::id());

    // With its token, the desktop reuses that daemon.
    let again = launcher
        .connect_or_start(ClientKind::Desktop, Some(launched.ui_token.clone()), true)
        .await
        .unwrap();
    assert!(again.launched.is_none() && !again.replaced);
    assert_eq!(again.client.principal(), Principal::Ui);

    // The CLI is an agent and leaves a working daemon alone.
    let cli = launcher.connect_or_start(ClientKind::Cli, None, false).await.unwrap();
    assert!(cli.launched.is_none() && !cli.replaced);
    assert_eq!(cli.client.principal(), Principal::Agent);
    drop((first, again, cli));

    // A desktop process without the token replaces the daemon with its own.
    let mut fresh = launcher
        .connect_or_start(ClientKind::Desktop, None, true)
        .await
        .unwrap();
    let replacement = fresh.launched.clone().expect("a replacement was started");
    assert!(fresh.replaced);
    assert_ne!(replacement.pid, launched.pid);
    assert_eq!(fresh.client.principal(), Principal::Ui);
    assert_eq!(fresh.client.health().await.unwrap().pid, replacement.pid);

    // The first daemon's token means nothing to the replacement.
    let stale = Client::connect_with(launcher.endpoint(), ClientKind::Desktop, Some(&launched.ui_token)).await;
    assert!(matches!(stale, Err(ClientError::Rpc(ref e)) if e.code == codes::FORBIDDEN));

    launcher.stop(fresh.client).await.unwrap();
    assert!(matches!(
        Client::connect(launcher.endpoint(), ClientKind::Cli).await,
        Err(ClientError::NotRunning { .. })
    ));
}

#[tokio::test]
async fn a_second_spawn_reports_already_running() {
    let temp = tempfile::tempdir().unwrap();
    let dirs = DataDirs::at(temp.path());
    let _guard = StopOnDrop(dirs.clone());

    let first = spawn_daemon(&program(), &dirs).await.unwrap();
    let second = spawn_daemon(&program(), &dirs).await;
    assert!(matches!(second, Err(LaunchError::AlreadyRunning)), "got {second:?}");

    let launcher = Launcher::new(dirs, program());
    let client = Client::connect_with(launcher.endpoint(), ClientKind::Desktop, Some(&first.ui_token))
        .await
        .unwrap();
    assert_eq!(client.principal(), Principal::Ui);
    launcher.stop(client).await.unwrap();
}

#[tokio::test]
async fn a_missing_program_is_reported_clearly() {
    let temp = tempfile::tempdir().unwrap();
    let missing = temp.path().join("no-such-regards");
    let result = spawn_daemon(&missing, &DataDirs::at(temp.path())).await;
    assert!(matches!(result, Err(LaunchError::ProgramMissing(ref path)) if *path == missing));
}

#[tokio::test]
async fn daemon_stop_returns_once_the_daemon_let_go() {
    let temp = tempfile::tempdir().unwrap();
    let dirs = DataDirs::at(temp.path());
    let _guard = StopOnDrop(dirs.clone());
    let launcher = Launcher::new(dirs.clone(), program());
    drop(
        launcher
            .connect_or_start(ClientKind::Desktop, None, true)
            .await
            .unwrap(),
    );

    let stopped = std::process::Command::new(program())
        .args(["daemon", "stop"])
        .env(DATA_DIR_ENV, dirs.data())
        .output()
        .unwrap();
    assert!(stopped.status.success(), "{stopped:?}");
    // What an installer relies on before it replaces the files: the daemon is gone.
    let lock = std::fs::File::options()
        .read(true)
        .write(true)
        .open(dirs.lock_file())
        .unwrap();
    assert!(lock.try_lock().is_ok(), "the daemon still holds its data directory");
}
