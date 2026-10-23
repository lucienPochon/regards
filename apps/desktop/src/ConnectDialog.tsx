import { type ReactNode, useEffect, useRef, useState } from "react";
import {
  AGENT_APPS,
  type AgentApp,
  type AgentsSettings,
  addToCursor,
  agentAppsFound,
  agentsSettings,
  type CommandLine,
  commandLine,
  type FoundAgentApp,
  isConnected,
  isFound,
  openInAgent,
} from "./agents";
import { useDialogFocus } from "./dialogFocus";
import { Icon } from "./icons";
import { useShortcuts } from "./shortcuts";

/** A card of the dialog: an agent app Regards knows, or any other agent. */
export type ConnectId = AgentApp["id"] | "other";

/** How often the dialog looks again at what is set up, while it is open. */
const LOOK_EVERY_MS = 3_000;

/** What an agent is asked so that it learns Regards (`regards skill`). */
const LEARN = "Regards is the email app on this computer, and its `regards` command lets you read, search and organise my mail. Please run `regards skill` to see what it can do, then install that skill";
const LEARN_CLAUDE_CODE = `${LEARN} with \`regards skill --install ~/.claude/skills\`.`;
const LEARN_ANY = `${LEARN} where you keep your skills, with \`regards skill --install <that folder>\`.`;
/** A first request, to see the connection work. */
const FIRST_ASK = "Which mail accounts can you see in Regards?";
const MCP_JSON = '{\n  "mcpServers": {\n    "regards": { "command": "regards", "args": ["mcp"] }\n  }\n}';
const CLAUDE_CODE_MCP = "claude mcp add --scope user regards -- regards mcp";
const CODEX_MCP = "codex mcp add regards -- regards mcp";

const LABELS: Record<ConnectId, string> = {
  "claude-code": "Claude Code",
  "claude-app": "Claude app",
  codex: "Codex",
  cursor: "Cursor",
  other: "Other agents",
};

/** A small mark for each agent: its initials, not its logo. */
export function AgentMark({ id }: { id: ConnectId }) {
  if (id === "other") {
    return (
      <span className="agent-mark other" aria-hidden="true">
        <Icon name="agent" size={14} />
      </span>
    );
  }
  const initials = { "claude-code": "CC", "claude-app": "C", codex: "Cx", cursor: "Cu" }[id];
  return (
    <span className={`agent-mark ${id}`} aria-hidden="true">
      {initials}
    </span>
  );
}

/** One numbered step: a title, or none, and what to do. */
function Step({ n, title, children }: { n: number; title?: string; children?: ReactNode }) {
  return (
    <li className="connect-step">
      <span className="connect-step-n" aria-hidden="true">
        {n}
      </span>
      <div className="connect-step-body">
        {title && <strong className="connect-step-title">{title}</strong>}
        {children}
      </div>
    </li>
  );
}

/**
 * "Connect an agent": a card per agent, with its steps. The first tab has the
 * agents found on this computer, the second all of them. Each found one says
 * whether it is active: Regards is set up in it, or it is connected now. That
 * is looked at again every few seconds, so it turns active as it gets set up.
 * Regards changes nothing in the agents' files: the person, or the agent, does.
 */
export function ConnectDialog({
  open,
  onSettings,
  onClose,
}: {
  /** The card open on arrival. */
  open?: ConnectId;
  /** Settings › Agents, where each account's level is chosen. */
  onSettings?: () => void;
  onClose: () => void;
}) {
  const box = useRef<HTMLElement>(null);
  useDialogFocus(box);
  useShortcuts({ Escape: onClose }, { modal: true });

  const [found, setFound] = useState<FoundAgentApp[] | null>(null);
  const [settings, setSettings] = useState<AgentsSettings | null>(null);
  const [tab, setTab] = useState<"here" | "all" | null>(null);
  const [openId, setOpenId] = useState<ConnectId | null>(open ?? null);
  const [line, setLine] = useState<CommandLine | null>(null);
  const [adding, setAdding] = useState(false);
  const [copied, setCopied] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const look = () => {
      agentAppsFound().then(setFound, () => setFound((before) => before ?? []));
      agentsSettings().then(setSettings, () => undefined);
    };
    look();
    const timer = window.setInterval(look, LOOK_EVERY_MS);
    commandLine().then(setLine, () => setLine(null));
    return () => window.clearInterval(timer);
  }, []);

  const here: ConnectId[] = found ? AGENT_APPS.filter((app) => isFound(app, settings, found)).map((app) => app.id) : [];

  // The first tab when something was found there, and the asked-for card is in it.
  useEffect(() => {
    if (tab !== null || found === null) return;
    setTab(here.length > 0 && (!open || here.includes(open)) ? "here" : "all");
  }, [found, tab, open, here]);

  const everyone: ConnectId[] = [...here, ...[...AGENT_APPS.map((app) => app.id), "other" as const].filter((id) => !here.includes(id))];
  const base = tab === "here" ? here : everyone;
  // The open card goes first.
  const cards = openId && base.includes(openId) ? [openId, ...base.filter((id) => id !== openId)] : base;

  const choose = (next: "here" | "all") => {
    setTab(next);
    setOpenId(null);
  };
  const copy = (key: string, text: string) => {
    navigator.clipboard?.writeText(text).then(
      () => setCopied(key),
      () => setError("Copying didn't work here: select the text and copy it."),
    );
  };
  const addToPath = () => {
    setAdding(true);
    setError(null);
    commandLine("add").then(
      (next) => {
        setLine(next);
        setAdding(false);
      },
      (failure: unknown) => {
        setAdding(false);
        setError(String(failure));
      },
    );
  };
  const run = (action: Promise<void>) => action.catch((failure: unknown) => setError(String(failure)));

  /** A box of text to copy, with its button at its foot. */
  const copyBox = (key: string, text: string, extra?: ReactNode) => (
    <div className="connect-copy">
      <pre>
        <code>{text}</code>
      </pre>
      <div className="connect-copy-actions">
        {extra}
        <button type="button" className="pill-btn" onClick={() => copy(key, text)}>
          <Icon name={copied === key ? "check" : "copy"} size={13} />
          {copied === key ? "Copied" : "Copy"}
        </button>
      </div>
    </div>
  );

  const steps = (id: ConnectId) => {
    switch (id) {
      case "claude-code":
        return (
          <>
            <p className="connect-lead">
              Send Claude Code this request. It installs the Regards skill, and from then on knows how to work with your mail
              through Regards.
            </p>
            {copyBox(
              "claude-code",
              LEARN_CLAUDE_CODE,
              <button type="button" className="pill-btn" onClick={() => run(openInAgent("claude-code", LEARN_CLAUDE_CODE))}>
                <Icon name="external" size={13} />
                Open in Claude Code
              </button>,
            )}
            <ol className="connect-steps">
              <Step n={1}>Open Claude Code, or use the button above: it opens in a new terminal.</Step>
              <Step n={2}>Start a new session.</Step>
              <Step n={3}>Paste the request and send it.</Step>
              <Step n={4}>Claude Code installs the skill. This card then says Active.</Step>
            </ol>
            <p className="connect-lead">Or connect it as an MCP server, from a terminal:</p>
            {copyBox("claude-code-mcp", CLAUDE_CODE_MCP)}
          </>
        );
      case "claude-app":
        return (
          <ol className="connect-steps">
            <Step n={1} title="Open the Claude app's settings file">
              <p>
                In the Claude app, open <b>Settings › Developer</b> and choose <b>Edit Config</b>. It shows the file{" "}
                <code>claude_desktop_config.json</code>: open it in a text editor.
              </p>
            </Step>
            <Step n={2} title="Add Regards to it">
              <p>
                Put this in the file. If it already holds settings, add <code>"mcpServers"</code> beside them, inside the outer
                braces.
              </p>
              {copyBox("claude-app", MCP_JSON)}
            </Step>
            <Step n={3} title="Restart the Claude app">
              <p>Quit it from its icon by the clock, not only its window, and open it again.</p>
            </Step>
            <Step n={4} title="Ask Claude">
              <p>Start a new chat and send this, to see the connection work.</p>
              {copyBox("claude-app-ask", FIRST_ASK)}
            </Step>
          </ol>
        );
      case "cursor":
        return (
          <ol className="connect-steps">
            <Step n={1} title="Add Regards to Cursor">
              <p>Cursor opens and asks you to confirm adding Regards as one of its MCP servers.</p>
              <div className="connect-actions">
                <button type="button" className="btn primary small" onClick={() => run(addToCursor())}>
                  Add to Cursor
                </button>
              </div>
              <p>
                Or add it to <code>~/.cursor/mcp.json</code> yourself:
              </p>
              {copyBox("cursor", MCP_JSON)}
            </Step>
            <Step n={2} title="Ask Cursor">
              <p>Start a new chat in Agent mode and send this, to see the connection work.</p>
              {copyBox("cursor-ask", FIRST_ASK)}
            </Step>
          </ol>
        );
      case "codex":
        return (
          <ol className="connect-steps">
            <Step n={1} title="Add Regards to Codex">
              <p>
                Run this in a terminal. It adds Regards to Codex's MCP servers, in <code>~/.codex/config.toml</code>.
              </p>
              {copyBox("codex", CODEX_MCP)}
            </Step>
            <Step n={2} title="Ask Codex">
              <p>Start a new chat and send this, to see the connection work.</p>
              {copyBox("codex-ask", FIRST_ASK)}
            </Step>
          </ol>
        );
      case "other":
        return (
          <ol className="connect-steps">
            <Step n={1} title="Teach it Regards">
              <p>Any agent that runs commands can use Regards. Send it this request.</p>
              {copyBox("other", LEARN_ANY)}
            </Step>
            <Step n={2} title="Or add Regards as an MCP server">
              <p>
                For agents that take MCP servers: the command <code>regards</code> with the argument <code>mcp</code>, over
                stdio.
              </p>
            </Step>
          </ol>
        );
    }
  };

  const status = (id: ConnectId) => {
    const app = AGENT_APPS.find((entry) => entry.id === id);
    if (!app || !found || !isFound(app, settings, found)) return null;
    return (
      <span className="connect-status">
        This computer ·{" "}
        {isConnected(app, settings, found) ? <span className="connect-active">Active</span> : "not connected yet"}
      </span>
    );
  };

  return (
    <div className="backdrop" onMouseDown={onClose}>
      <section
        ref={box}
        tabIndex={-1}
        className="dialog connect-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="connect-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="connect-head">
          <div>
            <h2 id="connect-title">Connect an AI agent</h2>
            <p className="muted">Pick the agent you use, then follow its steps.</p>
          </div>
          <button type="button" className="icon-btn small" aria-label="Close" onClick={onClose}>
            <Icon name="close" size={16} />
          </button>
        </header>

        {line && !line.on_path && (
          <div className="connect-path">
            <p>
              Agents reach your mail through the <code>regards</code> command, which isn't on your{" "}
              {line.can_add ? "Path" : "PATH"} yet.
            </p>
            {line.can_add ? (
              <button type="button" className="btn primary small" disabled={adding} onClick={addToPath}>
                {adding ? "Adding…" : "Add it to the Path"}
              </button>
            ) : (
              copyBox("link", `sudo ln -sf "${line.folder}/regards" /usr/local/bin/regards`)
            )}
          </div>
        )}
        {line?.on_path && line.changed && (
          <div className="connect-path done" role="status">
            <Icon name="check" size={15} />
            <p>
              <code>regards</code> is on your Path. Terminals opened from now on find it: restart Claude Code, Cursor or
              Codex if they are open.
            </p>
          </div>
        )}

        <div className="connect-tabs" role="tablist" aria-label="Agents">
          {(["here", "all"] as const).map((id) => (
            <button
              key={id}
              type="button"
              role="tab"
              aria-selected={tab === id}
              className={tab === id ? "connect-tab chosen" : "connect-tab"}
              onClick={() => choose(id)}
            >
              {id === "here" ? "On this computer" : "All agents"}
              {id === "here" && found && <span className="connect-count">{here.length}</span>}
            </button>
          ))}
        </div>

        <div className="connect-cards" role="tabpanel">
          {tab === null && <p className="muted">Looking for agents on this computer…</p>}
          {tab === "here" && here.length === 0 && (
            <p className="muted">No AI agent was found on this computer. All agents lists every one Regards knows.</p>
          )}
          {tab !== null &&
            cards.map((id) => {
              const opened = openId === id;
              return (
                <section key={id} className={opened ? "connect-card open" : "connect-card"}>
                  <button
                    type="button"
                    className="connect-card-head"
                    aria-expanded={opened}
                    aria-controls={`connect-${id}`}
                    onClick={() => setOpenId(opened ? null : id)}
                  >
                    <AgentMark id={id} />
                    <span className="connect-card-name">
                      <strong>{LABELS[id]}</strong>
                      {status(id)}
                    </span>
                    <Icon name="chevronDown" size={16} className="connect-chevron" />
                  </button>
                  {opened && (
                    <div className="connect-card-body" id={`connect-${id}`}>
                      <h3>Connect {LABELS[id] === "Other agents" ? "another agent" : LABELS[id]}</h3>
                      {steps(id)}
                      <p className="connect-note">
                        Agents see an account only once you allow it, in Settings › Agents. Regards needn't be open: the{" "}
                        <code>regards</code> command starts it in the background.
                      </p>
                    </div>
                  )}
                </section>
              );
            })}
        </div>

        {error && (
          <p className="error" role="alert">
            {error}
          </p>
        )}
        <footer className="dialog-actions connect-foot">
          {onSettings && (
            <button
              type="button"
              className="btn"
              onClick={() => {
                onClose();
                onSettings();
              }}
            >
              Choose what agents may do
            </button>
          )}
          <button type="button" className="btn primary" onClick={onClose}>
            Done
          </button>
        </footer>
      </section>
    </div>
  );
}
