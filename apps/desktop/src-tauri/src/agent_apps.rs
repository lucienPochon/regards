//! The agent apps on this computer, and whether Regards is set up in each
//! (the sidebar's Agents menu). This only looks: whether an app's settings
//! folder is there, and whether `regards` is among its skills or MCP
//! servers. Nothing of theirs is changed or kept.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;
use tauri::Manager;

/// An agent app Regards knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Id {
    ClaudeCode,
    ClaudeApp,
    Codex,
    Cursor,
}

/// An agent app found on this computer.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Found {
    id: Id,
    /// Regards is set up in it: its skill is installed (`regards skill
    /// --install`), or `regards` is one of its MCP servers.
    set_up: bool,
}

/// Where the agent apps keep their settings.
struct Places {
    home: PathBuf,
    /// The Claude app's settings folder, by the ways it is installed.
    claude_app: Vec<PathBuf>,
}

/// The skill `regards skill --install <dir>` writes.
fn has_skill(dir: &Path) -> bool {
    dir.join("regards").join("SKILL.md").is_file()
}

/// A JSON settings file whose `mcpServers` has `regards` (Claude Code, the
/// Claude app, Cursor).
fn has_mcp_json(file: &Path) -> bool {
    let Ok(text) = fs::read_to_string(file) else {
        return false;
    };
    let Ok(json) = serde_json::from_str::<Value>(&text) else {
        return false;
    };
    json.pointer("/mcpServers/regards").is_some()
}

/// Codex's `config.toml` with an `[mcp_servers.regards]` table.
fn has_mcp_toml(file: &Path) -> bool {
    let Ok(text) = fs::read_to_string(file) else {
        return false;
    };
    text.lines()
        .map(str::trim)
        .any(|line| line == "[mcp_servers.regards]" || line == "[mcp_servers.\"regards\"]")
}

fn look(places: &Places) -> Vec<Found> {
    let home = &places.home;
    let mut found = Vec::new();

    let claude = home.join(".claude");
    let claude_json = home.join(".claude.json");
    if claude.is_dir() || claude_json.is_file() {
        let set_up = has_skill(&claude.join("skills")) || has_mcp_json(&claude_json);
        found.push(Found {
            id: Id::ClaudeCode,
            set_up,
        });
    }

    let claude_app: Vec<&PathBuf> = places.claude_app.iter().filter(|dir| dir.is_dir()).collect();
    if !claude_app.is_empty() {
        let set_up = claude_app
            .iter()
            .any(|dir| has_mcp_json(&dir.join("claude_desktop_config.json")));
        found.push(Found {
            id: Id::ClaudeApp,
            set_up,
        });
    }

    let codex = home.join(".codex");
    if codex.is_dir() {
        let set_up = has_skill(&codex.join("skills")) || has_mcp_toml(&codex.join("config.toml"));
        found.push(Found { id: Id::Codex, set_up });
    }

    let cursor = home.join(".cursor");
    if cursor.is_dir() {
        let set_up = has_skill(&cursor.join("skills")) || has_mcp_json(&cursor.join("mcp.json"));
        found.push(Found { id: Id::Cursor, set_up });
    }

    found
}

/// The Claude app's settings folders: `Claude` in the app data folder
/// (`%APPDATA%`, `~/Library/Application Support`, `~/.config`), and on
/// Windows, installed from the Store, in its package's own copy of it.
fn claude_app_dirs(app: &tauri::AppHandle) -> Vec<PathBuf> {
    let paths = app.path();
    let mut dirs: Vec<PathBuf> = [paths.data_dir(), paths.config_dir()]
        .into_iter()
        .flatten()
        .map(|dir| dir.join("Claude"))
        .collect();
    if cfg!(windows)
        && let Ok(packages) = paths.local_data_dir().map(|dir| dir.join("Packages"))
        && let Ok(entries) = fs::read_dir(packages)
    {
        dirs.extend(
            entries
                .flatten()
                .filter(|entry| entry.file_name().to_string_lossy().starts_with("Claude_"))
                .map(|entry| entry.path().join("LocalCache").join("Roaming").join("Claude")),
        );
    }
    dirs.dedup();
    dirs
}

/// The agent apps on this computer, and whether Regards is set up in each.
#[tauri::command]
pub(crate) fn agent_apps_found(app: tauri::AppHandle) -> Result<Vec<Found>, String> {
    let home = app
        .path()
        .home_dir()
        .map_err(|error| format!("no home folder: {error}"))?;
    Ok(look(&Places {
        home,
        claude_app: claude_app_dirs(&app),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty folder of its own under the system's temporary folder.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("regards-agent-apps-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: PathBuf, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn nothing_found_in_an_empty_home() {
        let home = scratch("empty");
        let claude_app = vec![home.join("AppData").join("Claude")];
        assert_eq!(look(&Places { home, claude_app }), vec![]);
    }

    #[test]
    fn found_apps_and_where_regards_is_set_up() {
        let home = scratch("found");
        // Claude Code, with the skill.
        write(
            home.join(".claude/skills/regards/SKILL.md"),
            "---\nname: regards\n---\n",
        );
        // The Claude app, with other MCP servers only.
        let claude_app = home.join("AppData/Claude");
        write(
            claude_app.join("claude_desktop_config.json"),
            r#"{"mcpServers":{"other":{"command":"x"}}}"#,
        );
        // Codex, with the MCP server.
        write(
            home.join(".codex/config.toml"),
            "model = \"x\"\n\n[mcp_servers.regards]\ncommand = \"regards\"\n",
        );
        // Cursor, with a broken mcp.json.
        write(home.join(".cursor/mcp.json"), "{ not json");

        let found = look(&Places {
            home: home.clone(),
            claude_app: vec![home.join("missing"), claude_app],
        });
        assert_eq!(
            found,
            vec![
                Found {
                    id: Id::ClaudeCode,
                    set_up: true
                },
                Found {
                    id: Id::ClaudeApp,
                    set_up: false
                },
                Found {
                    id: Id::Codex,
                    set_up: true
                },
                Found {
                    id: Id::Cursor,
                    set_up: false
                },
            ]
        );
    }

    #[test]
    fn claude_code_as_an_mcp_server_of_the_user() {
        let home = scratch("mcp");
        write(
            home.join(".claude.json"),
            r#"{"projects":{"/x":{"mcpServers":{"regards":{}}}},"mcpServers":{"regards":{"command":"regards","args":["mcp"]}}}"#,
        );
        let found = look(&Places {
            home,
            claude_app: vec![],
        });
        assert_eq!(
            found,
            vec![Found {
                id: Id::ClaudeCode,
                set_up: true
            }]
        );
    }

    #[test]
    fn only_the_users_own_servers_count() {
        // Set up for one project only: not for Claude Code at large.
        let home = scratch("project");
        write(
            home.join(".claude.json"),
            r#"{"projects":{"/x":{"mcpServers":{"regards":{}}}}}"#,
        );
        let found = look(&Places {
            home,
            claude_app: vec![],
        });
        assert_eq!(
            found,
            vec![Found {
                id: Id::ClaudeCode,
                set_up: false
            }]
        );
    }
}
