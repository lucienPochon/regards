//! Opening the operating system's own settings, and web pages, from the
//! app's Settings: notification permissions, the default mail app, help
//! pages. Only these panes, and only `https://` pages, in the system's
//! browser.

use serde::Deserialize;

/// A pane of the system's settings.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Pane {
    /// Whether Regards may show notifications.
    Notifications,
    /// Which app opens `mailto:` links.
    DefaultApps,
}

/// Where each pane is, or why there is none to open.
fn pane_address(pane: Pane) -> Result<&'static str, &'static str> {
    match pane {
        Pane::Notifications if cfg!(windows) => Ok("ms-settings:notifications"),
        Pane::DefaultApps if cfg!(windows) => Ok("ms-settings:defaultapps"),
        Pane::Notifications if cfg!(target_os = "macos") => {
            Ok("x-apple.systempreferences:com.apple.preference.notifications")
        }
        Pane::DefaultApps if cfg!(target_os = "macos") => {
            Err("on macOS, the default mail app is chosen in Mail, under Settings › General")
        }
        _ => Err("open your desktop's settings to change this"),
    }
}

#[tauri::command]
pub(crate) fn open_system_settings(pane: Pane) -> Result<(), String> {
    let address = pane_address(pane)?;
    open::that_detached(address).map_err(|error| format!("could not open the settings: {error}"))
}

/// A web page in the system's browser: `https://` only.
#[tauri::command]
pub(crate) fn open_url(url: String) -> Result<(), String> {
    let safe = url
        .strip_prefix("https://")
        .is_some_and(|rest| !rest.is_empty() && !rest.chars().any(|c| c.is_whitespace() || c.is_control()));
    if !safe {
        return Err("only https:// pages are opened".into());
    }
    open::that_detached(&url).map_err(|error| format!("could not open the browser: {error}"))
}

/// An agent app a request can be handed to (the sidebar's Agents menu).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum AgentApp {
    /// Claude Code, in a new terminal (`claude-cli://`).
    ClaudeCode,
    /// The Claude desktop app, in a new chat (`claude://`).
    ClaudeApp,
}

/// The longest request handed over, as the agents' links take it.
const PROMPT_MAX: usize = 5_000;

/// The link that opens `app` with `prompt` ready, as documented by Anthropic
/// (code.claude.com/docs/en/deep-links; support.claude.com, "Open Claude
/// Desktop with a link"). Neither sends it: the person does, there.
fn agent_link(app: AgentApp, prompt: &str) -> Result<String, String> {
    let prompt = prompt.trim();
    if prompt.is_empty() || prompt.chars().count() > PROMPT_MAX {
        return Err(format!("a request needs 1 to {PROMPT_MAX} characters"));
    }
    if prompt.chars().any(|c| c.is_control() && c != '\n') {
        return Err("a request cannot hold control characters".into());
    }
    let mut q = String::with_capacity(prompt.len() * 3);
    for byte in prompt.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => q.push(char::from(byte)),
            _ => q.push_str(&format!("%{byte:02X}")),
        }
    }
    Ok(match app {
        AgentApp::ClaudeCode => format!("claude-cli://open?q={q}"),
        AgentApp::ClaudeApp => format!("claude://claude.ai/new?q={q}"),
    })
}

/// Open an agent app with a request ready to send.
#[tauri::command]
pub(crate) fn open_in_agent(app: AgentApp, prompt: String) -> Result<(), String> {
    let link = agent_link(app, &prompt)?;
    open::that_detached(&link).map_err(|error| format!("could not open the agent: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_go_to_agents_as_their_documented_links() {
        assert_eq!(
            agent_link(AgentApp::ClaudeCode, " Morning briefing, please.\nThanks "),
            Ok("claude-cli://open?q=Morning%20briefing%2C%20please.%0AThanks".into())
        );
        assert_eq!(
            agent_link(AgentApp::ClaudeApp, "Résumé & co"),
            Ok("claude://claude.ai/new?q=R%C3%A9sum%C3%A9%20%26%20co".into())
        );
        assert!(agent_link(AgentApp::ClaudeCode, "   ").is_err());
        assert!(agent_link(AgentApp::ClaudeCode, &"a".repeat(PROMPT_MAX + 1)).is_err());
        assert!(agent_link(AgentApp::ClaudeCode, "bell \u{7}").is_err());
    }

    #[test]
    fn only_secure_pages_and_known_panes() {
        assert!(open_url("http://example.test".into()).is_err());
        assert!(open_url("javascript:alert(1)".into()).is_err());
        assert!(open_url("https://".into()).is_err());
        assert!(open_url("https://example.test/a b".into()).is_err());
        if cfg!(windows) {
            assert_eq!(pane_address(Pane::DefaultApps), Ok("ms-settings:defaultapps"));
        }
        if cfg!(target_os = "macos") {
            assert!(pane_address(Pane::DefaultApps).is_err());
        }
    }
}
