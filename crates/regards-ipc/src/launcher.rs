//! Starting the daemon from a client (ADR-0005).
//!
//! A client that finds no daemon starts `regards daemon run --ui-handoff` as a
//! detached background process. Once listening, the daemon prints one
//! [`Announcement`] line on its stdout, which only the starting process can
//! read. That line doubles as the readiness signal and carries the desktop
//! bridge's [`UiToken`].
//!
//! A running daemon from a different build is replaced, so a client never talks
//! to a stale daemon after an update. The desktop also replaces a daemon that
//! does not accept its token (started by the CLI, or by an earlier desktop
//! process), because only a daemon it started can grant it the `ui` principal.

use crate::client::{Client, ClientError};
use crate::endpoint::Endpoint;
use crate::protocol::{ClientKind, Principal, codes};
use crate::token::UiToken;
use crate::transport;
use regards_core::paths::{DATA_DIR_ENV, DataDirs};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

/// Flag that makes `regards daemon run` print its [`Announcement`] on stdout.
pub const HANDOFF_FLAG: &str = "--ui-handoff";

/// How long a new daemon may take to start listening.
const ANNOUNCE_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a replaced daemon may take to stop.
const STOP_TIMEOUT: Duration = Duration::from_secs(10);
/// Connect/start rounds before giving up, for races with other clients.
const ATTEMPTS: u32 = 5;
/// A daemon log larger than this is rotated (one old file kept) at the next start.
const MAX_LOG_BYTES: u64 = 10 * 1024 * 1024;

/// The single line printed on stdout by a daemon started with [`HANDOFF_FLAG`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Announcement {
    /// Listening. The token gives the `ui` principal to whoever presents it.
    Ready {
        pid: u32,
        build_id: String,
        ui_token: UiToken,
    },
    /// Another daemon owns the data directory.
    AlreadyRunning,
    /// Startup failed; details are in the daemon log.
    Failed { message: String },
}

impl Announcement {
    /// The announcement as one line of JSON, without the newline.
    pub fn to_line(&self) -> String {
        serde_json::to_string(self).expect("announcements always serialize")
    }
}

/// Errors starting or replacing the daemon.
#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    #[error("the Regards service program was not found at {}", .0.display())]
    ProgramMissing(PathBuf),
    #[error("could not start the Regards service: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("another Regards service is already running")]
    AlreadyRunning,
    #[error("the Regards service failed to start: {0}")]
    Failed(String),
    #[error("the Regards service did not report that it was ready: {0}")]
    NoAnnouncement(String),
    #[error("the running Regards service did not stop in time")]
    StopTimeout,
    #[error("could not get a working Regards service after {ATTEMPTS} attempts")]
    GaveUp,
    #[error(transparent)]
    Client(#[from] ClientError),
}

/// A daemon this process started.
#[derive(Debug, Clone)]
pub struct Launched {
    pub pid: u32,
    pub ui_token: UiToken,
}

/// The result of [`Launcher::connect_or_start`].
pub struct Connected {
    pub client: Client,
    /// Set when this call started the daemon. Keep the token for later connections.
    pub launched: Option<Launched>,
    /// Whether a running daemon was asked to stop first.
    pub replaced: bool,
}

/// Where the `regards` program is expected: next to the calling executable.
/// Tauri installs sidecars there, and Cargo builds all workspace binaries
/// into the same directory.
pub fn sibling_program() -> std::io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    Ok(exe.with_file_name(format!("regards{}", std::env::consts::EXE_SUFFIX)))
}

/// Starts and connects to the daemon for one data directory.
pub struct Launcher {
    dirs: DataDirs,
    endpoint: Endpoint,
    program: PathBuf,
}

impl Launcher {
    /// `program` is the `regards` executable used to start a daemon.
    pub fn new(dirs: DataDirs, program: PathBuf) -> Self {
        let endpoint = Endpoint::for_dirs(&dirs);
        Self {
            dirs,
            endpoint,
            program,
        }
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// Connect to a daemon of this build, starting one if none is running.
    ///
    /// With `require_ui`, a daemon that does not accept `ui_token` is replaced
    /// by one this process starts; this is the desktop's behaviour. Without it,
    /// only a daemon from a different build is replaced.
    pub async fn connect_or_start(
        &self,
        kind: ClientKind,
        ui_token: Option<UiToken>,
        require_ui: bool,
    ) -> Result<Connected, LaunchError> {
        let mut token = ui_token;
        let mut launched = None;
        let mut replaced = false;

        for attempt in 0..ATTEMPTS {
            match Client::connect_with(&self.endpoint, kind, token.as_ref()).await {
                Ok(client) => {
                    let same_build = client.is_same_build();
                    let has_ui = client.principal() == Principal::Ui;
                    if same_build && (has_ui || !require_ui) {
                        return Ok(Connected {
                            client,
                            launched,
                            replaced,
                        });
                    }
                    tracing::info!(same_build, has_ui, "replacing the running daemon");
                    self.stop(client).await?;
                    replaced = true;
                }
                Err(ClientError::Rpc(error)) if error.code == codes::FORBIDDEN => {
                    // The token belongs to a daemon that has since been replaced.
                    token = None;
                    continue;
                }
                Err(ClientError::NotRunning { .. }) => {}
                Err(error) => return Err(error.into()),
            }

            match spawn_daemon(&self.program, &self.dirs).await {
                Ok(new) => {
                    tracing::info!(pid = new.pid, "started the daemon");
                    token = Some(new.ui_token.clone());
                    launched = Some(new);
                }
                // Another client started one in the meantime, or a replaced
                // daemon has not released its lock yet: connect again.
                Err(LaunchError::AlreadyRunning) => {
                    tokio::time::sleep(Duration::from_millis(100 * u64::from(attempt + 1))).await;
                }
                Err(error) => return Err(error),
            }
        }
        Err(LaunchError::GaveUp)
    }

    /// Ask a daemon to stop and wait until its endpoint is gone.
    pub async fn stop(&self, mut client: Client) -> Result<(), LaunchError> {
        client.shutdown().await?;
        drop(client);
        wait_until_stopped(&self.endpoint, STOP_TIMEOUT).await
    }
}

/// Start `program daemon run --ui-handoff` for `dirs`, detached from the
/// calling process, and wait for its announcement.
pub async fn spawn_daemon(program: &Path, dirs: &DataDirs) -> Result<Launched, LaunchError> {
    if !program.is_file() {
        return Err(LaunchError::ProgramMissing(program.to_path_buf()));
    }
    let stderr = match open_log(dirs) {
        Ok(file) => Stdio::from(file),
        Err(error) => {
            tracing::warn!(%error, "could not open the daemon log; its output is discarded");
            Stdio::null()
        }
    };

    let mut command = Command::new(program);
    command
        .args(["daemon", "run", HANDOFF_FLAG])
        .env(DATA_DIR_ENV, dirs.data())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(stderr);
    detach(&mut command);
    let mut child = command.spawn().map_err(LaunchError::Spawn)?;
    let stdout = child.stdout.take().expect("stdout is piped");

    let mut line = String::new();
    match tokio::time::timeout(ANNOUNCE_TIMEOUT, BufReader::new(stdout).read_line(&mut line)).await {
        Err(_) => {
            let _ = child.start_kill();
            return Err(LaunchError::NoAnnouncement("timed out".into()));
        }
        Ok(Err(error)) => return Err(LaunchError::NoAnnouncement(error.to_string())),
        Ok(Ok(0)) => return Err(LaunchError::NoAnnouncement("it exited without a word".into())),
        Ok(Ok(_)) => {}
    }
    // Dropping `child` leaves the daemon running; it has nothing more to say on stdout.
    match serde_json::from_str(line.trim()) {
        Ok(Announcement::Ready { pid, ui_token, .. }) => Ok(Launched { pid, ui_token }),
        Ok(Announcement::AlreadyRunning) => Err(LaunchError::AlreadyRunning),
        Ok(Announcement::Failed { message }) => Err(LaunchError::Failed(message)),
        Err(error) => Err(LaunchError::NoAnnouncement(format!("unreadable announcement: {error}"))),
    }
}

/// Wait until nothing listens on `endpoint` any more.
pub async fn wait_until_stopped(endpoint: &Endpoint, timeout: Duration) -> Result<(), LaunchError> {
    let deadline = Instant::now() + timeout;
    loop {
        match transport::connect(endpoint).await {
            Err(error) if transport::is_not_running(&error) => return Ok(()),
            _ if Instant::now() >= deadline => return Err(LaunchError::StopTimeout),
            _ => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    }
}

fn open_log(dirs: &DataDirs) -> std::io::Result<File> {
    let path = dirs.daemon_log();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if std::fs::metadata(&path).is_ok_and(|meta| meta.len() > MAX_LOG_BYTES) {
        // Best effort: a daemon that is still stopping may hold the file open on Windows.
        let _ = std::fs::rename(&path, path.with_extension("log.1"));
    }
    File::options().create(true).append(true).open(path)
}

/// The daemon must outlive a console window or terminal the client was started from.
#[cfg(windows)]
fn detach(command: &mut Command) {
    use windows_sys::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW};
    command.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
}

#[cfg(unix)]
fn detach(command: &mut Command) {
    command.process_group(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn announcements_round_trip_as_one_json_line() {
        let ready = Announcement::Ready {
            pid: 42,
            build_id: "0.1.0+dev".into(),
            ui_token: UiToken::generate().unwrap(),
        };
        for announcement in [
            ready,
            Announcement::AlreadyRunning,
            Announcement::Failed { message: "x".into() },
        ] {
            let line = announcement.to_line();
            assert!(!line.contains('\n'));
            assert_eq!(serde_json::from_str::<Announcement>(&line).unwrap(), announcement);
        }
        assert_eq!(Announcement::AlreadyRunning.to_line(), r#"{"event":"already_running"}"#);
    }
}
