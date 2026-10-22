import { useState } from "react";
import { openUrl } from "./background";
import { Icon } from "./icons";
import { Choice } from "./SettingsControls";

/** The agent guide, on the project's site. */
const GUIDE = "https://github.com/lucienPochon/regards/blob/main/docs/agents.md";

type App = "claude" | "claude-app" | "codex" | "cursor" | "skill";

/** How each agent app takes Regards: the one thing to run or paste (docs/agents.md § 4). */
const APPS: { id: App; label: string; where: string; text: string }[] = [
  {
    id: "claude",
    label: "Claude Code",
    // User scope: Regards in every folder Claude Code is started from.
    where: "Run in a terminal:",
    text: "claude mcp add --scope user regards -- regards mcp",
  },
  {
    id: "claude-app",
    label: "Claude app",
    where: "In the Claude app, Settings › Developer › Edit Config, add to the file, then restart the app:",
    text: '{ "mcpServers": { "regards": { "command": "regards", "args": ["mcp"] } } }',
  },
  {
    id: "codex",
    label: "Codex",
    where: "Add to ~/.codex/config.toml:",
    text: '[mcp_servers.regards]\ncommand = "regards"\nargs = ["mcp"]',
  },
  {
    id: "cursor",
    label: "Cursor",
    where: "Add to ~/.cursor/mcp.json:",
    text: '{ "mcpServers": { "regards": { "command": "regards", "args": ["mcp"] } } }',
  },
  {
    id: "skill",
    label: "Other agents",
    where: "Install the skill that explains Regards' commands, for agents that read skills:",
    text: "regards skill --install ~/.claude/skills",
  },
];

/**
 * Settings › Agents › Connect an agent: what to run or paste for each agent
 * app, to copy. Regards never edits another app's files; the person does.
 */
export function AgentConnect() {
  const [app, setApp] = useState<App>("claude");
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const chosen = APPS.find((entry) => entry.id === app) ?? APPS[0];
  const copy = () => {
    navigator.clipboard?.writeText(chosen.text).then(
      () => setCopied(true),
      () => setError("Copying didn't work here: select the text and copy it."),
    );
  };
  return (
    <section className="agent-connect" aria-labelledby="agent-connect-title">
      <h4 id="agent-connect-title" className="settings-subhead">
        Connect an agent
      </h4>
      <p className="muted settings-note">
        The <code>regards</code> command comes with Regards. An agent uses it through MCP, or as commands with a skill.
        Then choose above what it may do with each account.
      </p>
      <Choice
        label="Agent app"
        value={app}
        options={APPS.map(({ id, label }) => ({ id, label }))}
        onChange={(next) => {
          setApp(next);
          setCopied(false);
        }}
      />
      <p className="agent-connect-where">{chosen.where}</p>
      <div className="agent-connect-snippet">
        <pre>
          <code>{chosen.text}</code>
        </pre>
        <button type="button" className="btn small" onClick={copy}>
          <Icon name={copied ? "check" : "copy"} size={14} />
          {copied ? "Copied" : "Copy"}
        </button>
      </div>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      <button
        type="button"
        className="btn"
        onClick={() => openUrl(GUIDE).catch((failure: unknown) => setError(String(failure)))}
      >
        <Icon name="help" size={15} />
        Read the agent guide
      </button>
    </section>
  );
}
