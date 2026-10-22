//! The `regards` command on the person's PATH, so agents can run it from a
//! terminal: where it stands, and on Windows, putting its folder on the
//! user's Path or taking it off. The program does it (`regards path`), as
//! the uninstaller does.

use serde::{Deserialize, Serialize};

/// Mirrors `regards path --json`.
#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct CommandLine {
    /// `regards` is found on the PATH of new terminals.
    on_path: bool,
    /// The folder it is in.
    folder: String,
    /// Whether it can be put there from here (Windows).
    can_add: bool,
    /// Whether this call changed the Path.
    changed: bool,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Change {
    Add,
    Remove,
}

/// Where the `regards` command stands, after `change` if one is asked for.
#[tauri::command]
pub(crate) async fn command_line(change: Option<Change>) -> Result<CommandLine, String> {
    let (_, program) = crate::locate()?;
    let mut command = tokio::process::Command::new(program);
    command.arg("path");
    match change {
        Some(Change::Add) => command.arg("add"),
        Some(Change::Remove) => command.arg("remove"),
        None => &mut command,
    };
    command.arg("--json");
    // No console window flashing up (CREATE_NO_WINDOW).
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    let output = command
        .output()
        .await
        .map_err(|error| format!("could not run regards: {error}"))?;
    if !output.status.success() {
        let said = String::from_utf8_lossy(&output.stderr);
        let said = said.trim().trim_start_matches("error: ");
        return Err(if said.is_empty() {
            "regards path failed".into()
        } else {
            said.to_string()
        });
    }
    serde_json::from_slice(&output.stdout).map_err(|error| format!("unexpected answer from regards: {error}"))
}
