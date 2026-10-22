//! `regards path`: whether this command is on the PATH that new terminals
//! get, so an agent (Claude Code, Codex, Cursor…) can run it from anywhere
//! (docs/agents.md § 4). On Windows it puts its own folder on the user's
//! Path, or takes it off; the uninstaller takes it off too. Elsewhere the
//! person links it into a folder on their PATH.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// Where the command stands.
pub(crate) struct Status {
    /// `regards` is found on the PATH of new terminals.
    pub(crate) on_path: bool,
    /// The folder this program is in.
    pub(crate) folder: PathBuf,
    /// Whether `regards path add` can put it there (Windows).
    pub(crate) can_add: bool,
    /// Whether this call changed the Path.
    pub(crate) changed: bool,
}

impl Status {
    pub(crate) fn json(&self) -> Value {
        json!({
            "on_path": self.on_path,
            "folder": self.folder.display().to_string(),
            "can_add": self.can_add,
            "changed": self.changed,
        })
    }

    pub(crate) fn text(&self, program: &Path, change: Option<Change>) -> String {
        match (change.filter(|_| self.changed), self.on_path, self.can_add) {
            (Some(Change::Add), _, _) => format!(
                "Added {} to your Path. Terminals opened from now on find `regards`; restart the apps already open.",
                self.folder.display()
            ),
            (Some(Change::Remove), _, _) => format!("Took {} off your Path.", self.folder.display()),
            (None, true, _) => "`regards` is on your PATH.".into(),
            (None, false, true) => "`regards` is not on your Path. Run `regards path add` to put it there.".into(),
            (None, false, false) => format!(
                "`regards` is not on your PATH. Link it into a folder on it, for example:\n  sudo ln -sf \"{}\" /usr/local/bin/regards",
                program.display()
            ),
        }
    }
}

/// The program's name in a folder.
const PROGRAM: &str = if cfg!(windows) { "regards.exe" } else { "regards" };

/// `%NAME%` in a Path entry, from the environment; unknown names are kept.
#[cfg(any(windows, test))]
fn expand(entry: &str, var: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::new();
    let mut rest = entry;
    while let Some(start) = rest.find('%') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('%') else { break };
        out.push_str(&rest[..start]);
        match Some(&after[..end]).filter(|name| !name.is_empty()).and_then(&var) {
            Some(value) => out.push_str(&value),
            None => out.push_str(&rest[start..start + end + 2]),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Whether a Path entry names `folder`: Windows' rules, any case, with or
/// without quotes or a trailing separator.
#[cfg(any(windows, test))]
fn names(entry: &str, folder: &Path, var: &impl Fn(&str) -> Option<String>) -> bool {
    let clean = |text: &str| {
        text.trim()
            .trim_matches('"')
            .trim_end_matches(['\\', '/'])
            .replace('/', "\\")
            .to_lowercase()
    };
    let entry = clean(&expand(entry, var));
    !entry.is_empty() && entry == clean(&folder.to_string_lossy())
}

/// The Path with `folder` at its end, or `None` when it is already on it.
#[cfg(any(windows, test))]
fn with(path: &str, folder: &Path, var: impl Fn(&str) -> Option<String>) -> Option<String> {
    if path.split(';').any(|entry| names(entry, folder, &var)) {
        return None;
    }
    let path = path.trim_end_matches(';');
    let folder = folder.to_string_lossy();
    Some(if path.is_empty() {
        folder.into_owned()
    } else {
        format!("{path};{folder}")
    })
}

/// The Path without `folder`, or `None` when it is not on it.
#[cfg(any(windows, test))]
fn without(path: &str, folder: &Path, var: impl Fn(&str) -> Option<String>) -> Option<String> {
    let kept: Vec<&str> = path.split(';').filter(|entry| !names(entry, folder, &var)).collect();
    (kept.len() != path.split(';').count()).then(|| kept.join(";"))
}

/// The folder this program is in.
fn folder(program: &Path) -> anyhow::Result<PathBuf> {
    Ok(program
        .parent()
        .ok_or_else(|| anyhow::anyhow!("no folder for {}", program.display()))?
        .to_path_buf())
}

/// Where the command stands, and on Windows, `add` or `remove` it first.
pub(crate) fn run(program: &Path, change: Option<Change>) -> anyhow::Result<Status> {
    let folder = folder(program)?;
    let changed = match change {
        Some(change) => registry::change(&folder, change)?,
        None => false,
    };
    Ok(Status {
        on_path: registry::on_path().unwrap_or_else(unix_on_path),
        folder,
        can_add: cfg!(windows),
        changed,
    })
}

/// What `regards path` changes.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Change {
    Add,
    Remove,
}

/// Outside Windows: the PATH this program got, and the folders a terminal
/// usually has that an app started from the desktop may not.
fn unix_on_path() -> bool {
    let mut folders: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    folders.extend(["/usr/local/bin", "/opt/homebrew/bin", "/usr/bin"].map(PathBuf::from));
    if let Some(home) = std::env::var_os("HOME") {
        folders.push(PathBuf::from(home).join(".local/bin"));
    }
    folders.iter().any(|dir| dir.join(PROGRAM).is_file())
}

#[cfg(windows)]
mod registry {
    use super::{Change, PROGRAM, expand, with, without};
    use std::path::Path;
    use windows_registry::{CURRENT_USER, LOCAL_MACHINE, Type};

    const USER: &str = "Environment";
    const MACHINE: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment";

    fn var(name: &str) -> Option<String> {
        std::env::var(name).ok()
    }

    /// Whether a folder on the user's or the computer's Path holds `regards`,
    /// as stored now (this program's own PATH may be older).
    pub(super) fn on_path() -> Option<bool> {
        let mut entries = Vec::new();
        for key in [CURRENT_USER.open(USER), LOCAL_MACHINE.open(MACHINE)]
            .into_iter()
            .flatten()
        {
            if let Ok(path) = key.get_string("Path") {
                entries.extend(path.split(';').map(|entry| expand(entry, var)));
            }
        }
        Some(entries.iter().any(|entry| {
            let entry = entry.trim().trim_matches('"');
            !entry.is_empty() && Path::new(entry).join(PROGRAM).is_file()
        }))
    }

    /// Put `folder` on the user's Path, or take it off; whether it changed.
    pub(super) fn change(folder: &Path, change: Change) -> anyhow::Result<bool> {
        let key = CURRENT_USER.options().read().write().create().open(USER)?;
        let (kind, path) = match key.get_type("Path") {
            Ok(kind) => (kind, key.get_string("Path")?),
            Err(_) => (Type::ExpandString, String::new()),
        };
        let next = match change {
            Change::Add => with(&path, folder, var),
            Change::Remove => without(&path, folder, var),
        };
        let Some(next) = next else { return Ok(false) };
        // Kept as it was stored: entries such as %USERPROFILE%\bin keep working.
        if kind == Type::String {
            key.set_string("Path", &next)?;
        } else {
            key.set_expand_string("Path", &next)?;
        }
        tell_apps();
        Ok(true)
    }

    /// Tell running apps (Explorer above all) that the environment changed,
    /// so what they start from now on gets the new Path.
    #[allow(unsafe_code)]
    fn tell_apps() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
        };
        let what: Vec<u16> = "Environment\0".encode_utf16().collect();
        let mut result = 0;
        // SAFETY: `what` is a NUL-terminated UTF-16 string that outlives the
        // call, which waits at most five seconds for each window.
        unsafe {
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                0,
                what.as_ptr() as isize,
                SMTO_ABORTIFHUNG,
                5_000,
                &mut result,
            );
        }
    }
}

#[cfg(not(windows))]
mod registry {
    use super::Change;
    use std::path::Path;

    pub(super) fn on_path() -> Option<bool> {
        None
    }

    pub(super) fn change(_folder: &Path, _change: Change) -> anyhow::Result<bool> {
        anyhow::bail!("on this system, link `regards` into a folder on your PATH (see `regards path`)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn var(name: &str) -> Option<String> {
        (name == "LOCALAPPDATA").then(|| r"C:\Users\ann\AppData\Local".to_string())
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "changes the user's Path and puts it back: cargo test -p regards-cli --bin regards -- --ignored round_trip"]
    fn round_trip_on_the_users_path() {
        use windows_registry::CURRENT_USER;
        let stored = || CURRENT_USER.open("Environment").unwrap().get_string("Path").ok();
        let folder = std::env::temp_dir().join("regards-path-round-trip");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(PROGRAM), b"").unwrap();

        let before = stored();
        assert!(registry::change(&folder, Change::Add).unwrap());
        assert!(!registry::change(&folder, Change::Add).unwrap());
        assert_eq!(registry::on_path(), Some(true));
        assert!(registry::change(&folder, Change::Remove).unwrap());
        assert_eq!(stored(), before);
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn variables_in_entries_are_expanded() {
        assert_eq!(
            expand(r"%LOCALAPPDATA%\Regards", var),
            r"C:\Users\ann\AppData\Local\Regards"
        );
        assert_eq!(expand(r"%NOPE%\x;%", var), r"%NOPE%\x;%");
        assert_eq!(expand("plain", var), "plain");
        assert_eq!(expand("50%", var), "50%");
    }

    #[test]
    fn the_folder_is_added_once() {
        let folder = Path::new(r"C:\Users\ann\AppData\Local\Regards");
        assert_eq!(
            with("", folder, var).as_deref(),
            Some(r"C:\Users\ann\AppData\Local\Regards")
        );
        assert_eq!(
            with(r"C:\bin;", folder, var).as_deref(),
            Some(r"C:\bin;C:\Users\ann\AppData\Local\Regards")
        );
        // Already there, written another way.
        assert_eq!(with(r"C:\bin;%LOCALAPPDATA%\regards\", folder, var), None);
        assert_eq!(with(r#""c:\users\ann\appdata\local\regards""#, folder, var), None);
    }

    #[test]
    fn the_folder_is_taken_off_and_nothing_else() {
        let folder = Path::new(r"C:\Users\ann\AppData\Local\Regards");
        assert_eq!(
            without(r"C:\bin;%LOCALAPPDATA%\Regards;D:\tools", folder, var).as_deref(),
            Some(r"C:\bin;D:\tools")
        );
        assert_eq!(without(r"C:\bin;D:\tools", folder, var), None);
        assert_eq!(
            without(r"C:\Users\ann\AppData\Local\Regards", folder, var).as_deref(),
            Some("")
        );
    }
}
