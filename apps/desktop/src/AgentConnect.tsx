import { useState } from "react";
import { openUrl } from "./background";
import { ConnectDialog } from "./ConnectDialog";
import { Icon } from "./icons";
import { t } from "./i18n";

/** The agent guide, on the project's site. */
const GUIDE = "https://github.com/lucienPochon/regards/blob/main/docs/agents.md";

/**
 * Settings › Agents › Connect an agent: opens the Connect dialog, with each
 * agent's steps. Regards never edits another app's files; the person, or the
 * agent, does.
 */
export function AgentConnect() {
  const [open, setOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  return (
    <section className="agent-connect" aria-labelledby="agent-connect-title">
      <h4 id="agent-connect-title" className="settings-subhead">
        {t("Connect an agent")}
      </h4>
      <p className="muted settings-note">
        {t("Claude Code, the Claude app, Codex, Cursor and other agents reach your mail through the")} <code>regards</code>{" "}
        {t("command. Each has its own steps. Then choose above what it may do with each account.")}
      </p>
      <div className="agent-connect-actions">
        <button type="button" className="btn primary" onClick={() => setOpen(true)}>
          <Icon name="agent" size={15} />
          {t("Connect an agent…")}
        </button>
        <button
          type="button"
          className="btn"
          onClick={() => openUrl(GUIDE).catch((failure: unknown) => setError(String(failure)))}
        >
          <Icon name="help" size={15} />
          {t("Read the agent guide")}
        </button>
      </div>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      {open && <ConnectDialog onClose={() => setOpen(false)} />}
    </section>
  );
}
