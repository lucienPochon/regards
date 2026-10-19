import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { type AgentActivity as Entry, type AgentDecision, listAgentActivity } from "./agents";
import { fullDate } from "./format";
import { Icon } from "./icons";

/** How often the list looks for new entries while it is shown (recording wakes no client). */
const POLL_MS = 5000;
/** Entries a session shows before "Show all". */
const FOLDED = 5;

/** What agents did, in plain words; other methods show as named. */
const ACTIONS: Record<string, string> = {
  "account.list": "Listed accounts",
  "mailbox.list": "Listed folders",
  "message.list": "Listed messages",
  "message.view": "Listed pinned, set-aside or snoozed mail",
  "message.get": "Read a message",
  "message.html": "Read a message",
  "thread.get": "Read a conversation",
  "message.explain": "Asked why a message is where it is",
  "smart.overview": "Counted the Smart Inbox",
  "search.query": "Searched",
  "search.server": "Searched on the server",
  "draft.list": "Listed drafts",
  "draft.get": "Read a draft",
  "draft.create": "Started a message",
  "draft.reply": "Started a reply",
  "draft.forward": "Started a forward",
  "draft.update": "Changed a draft",
  "draft.send": "Sent a message",
  "message.archive": "Archived",
  "message.trash": "Deleted",
  "message.delete": "Deleted for good",
  "message.move": "Moved",
  "message.set_flags": "Marked read, unread or flagged",
  "message.pin": "Pinned",
  "message.snooze": "Snoozed",
  "message.set_aside": "Set aside",
  "message.remind": "Set a reminder",
  "sender.set": "Chose where a sender's mail goes",
  "sender.block": "Blocked a sender",
  "account.set_agent_access": "Tried to change what agents may do",
  "agent.activity": "Tried to read this list",
};

const DECISIONS: Partial<Record<AgentDecision, { label: string; tone: string }>> = {
  denied: { label: "Refused", tone: "refused" },
  held: { label: "Waiting for you", tone: "held" },
  approved: { label: "Approved", tone: "approved" },
  rejected: { label: "Rejected", tone: "refused" },
};

/** The action in words, with what was searched for when it was a search. */
function describe(entry: Entry): string {
  const action = ACTIONS[entry.method] ?? entry.method;
  const args = entry.args as { query?: unknown } | null;
  if (entry.method.startsWith("search.") && args && typeof args.query === "string" && args.query) {
    return `${action} “${args.query}”`;
  }
  return action;
}

const time = (at: number) => new Date(at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });

/** Requests made together (one agent connection, one after another) under one heading. */
function bySession(entries: Entry[]): { session: string; client: string; entries: Entry[] }[] {
  const groups: { session: string; client: string; entries: Entry[] }[] = [];
  for (const entry of entries) {
    const last = groups[groups.length - 1];
    if (last && last.session === entry.session) last.entries.push(entry);
    else groups.push({ session: entry.session, client: entry.client, entries: [entry] });
  }
  return groups;
}

/**
 * Settings › Agents: every request agents made, allowed or refused, newest
 * first and grouped by session; kept 90 days by the daemon.
 */
export function AgentActivity({ accounts }: { accounts: { id: string; email: string }[] }) {
  const [accountId, setAccountId] = useState<string | null>(null);
  const [refusedOnly, setRefusedOnly] = useState(false);
  const [entries, setEntries] = useState<Entry[] | null>(null);
  const [next, setNext] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loadingOlder, setLoadingOlder] = useState(false);
  const scope = useRef(accountId);
  scope.current = accountId;
  // The first page is in: later ones only add what is new.
  const loaded = useRef(false);

  // The newest page; new entries join the top of what is shown.
  const refresh = useCallback(() => {
    const asked = accountId;
    listAgentActivity({ accountId: asked }).then(
      (page) => {
        if (scope.current !== asked) return;
        setError(null);
        if (!loaded.current) {
          loaded.current = true;
          setEntries(page.entries);
          setNext(page.next);
          return;
        }
        setEntries((now) => {
          if (now === null) return page.entries;
          const known = new Set(now.map((entry) => entry.id));
          const fresh = page.entries.filter((entry) => !known.has(entry.id));
          return fresh.length > 0 ? [...fresh, ...now] : now;
        });
      },
      (failure: unknown) => setError(String(failure)),
    );
  }, [accountId]);

  useEffect(() => {
    loaded.current = false;
    setEntries(null);
    setNext(null);
    refresh();
    const timer = window.setInterval(() => {
      if (document.visibilityState === "visible") refresh();
    }, POLL_MS);
    window.addEventListener("focus", refresh);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener("focus", refresh);
    };
  }, [refresh]);

  const older = () => {
    if (next === null) return;
    const asked = accountId;
    setLoadingOlder(true);
    listAgentActivity({ accountId: asked, before: next })
      .then(
        (page) => {
          if (scope.current !== asked) return;
          setEntries((now) => [...(now ?? []), ...page.entries]);
          setNext(page.next);
        },
        (failure: unknown) => setError(String(failure)),
      )
      .finally(() => setLoadingOlder(false));
  };

  const shown = useMemo(
    () => (entries ?? []).filter((entry) => !refusedOnly || entry.decision === "denied" || entry.decision === "rejected"),
    [entries, refusedOnly],
  );
  const emailOf = (id: string | null) => accounts.find((account) => account.id === id)?.email ?? null;

  return (
    <section className="agent-activity" aria-label="Agent activity">
      <div className="activity-head">
        <h4>Activity</h4>
        <span className="choice" role="radiogroup" aria-label="Which requests">
          {[
            { id: false, label: "Everything" },
            { id: true, label: "Refused" },
          ].map((option) => (
            <button
              key={option.label}
              type="button"
              role="radio"
              aria-checked={refusedOnly === option.id}
              className={refusedOnly === option.id ? "chosen" : undefined}
              onClick={() => setRefusedOnly(option.id)}
            >
              {option.label}
            </button>
          ))}
        </span>
        {accounts.length > 1 && (
          <select
            className="agent-level"
            aria-label="Which account"
            value={accountId ?? ""}
            onChange={(event) => setAccountId(event.target.value || null)}
          >
            <option value="">All accounts</option>
            {accounts.map((account) => (
              <option key={account.id} value={account.id}>
                {account.email}
              </option>
            ))}
          </select>
        )}
      </div>
      {error && <p className="error">{error}</p>}
      {entries === null ? (
        !error && <p className="muted activity-empty">Loading…</p>
      ) : shown.length === 0 ? (
        <p className="muted activity-empty">
          {refusedOnly ? "No agent has been refused." : "No agent has used Regards yet. What they do will be listed here."}
        </p>
      ) : (
        <ol className="activity-sessions">
          {bySession(shown).map((group) => (
            <Session
              key={`${group.session}:${group.entries[group.entries.length - 1].id}`}
              client={group.client}
              entries={group.entries}
              emailOf={accounts.length > 1 ? emailOf : () => null}
            />
          ))}
        </ol>
      )}
      {next !== null && entries !== null && (
        <button type="button" className="btn small activity-older" onClick={older} disabled={loadingOlder}>
          {loadingOlder ? "Loading…" : "Show older"}
        </button>
      )}
    </section>
  );
}

function Session({
  client,
  entries,
  emailOf,
}: {
  client: string;
  entries: Entry[];
  emailOf: (id: string | null) => string | null;
}) {
  const [open, setOpen] = useState(false);
  const newest = entries[0];
  const oldest = entries[entries.length - 1];
  const refused = entries.filter((entry) => entry.decision === "denied").length;
  const shown = open ? entries : entries.slice(0, FOLDED);
  return (
    <li className="activity-session">
      <div className="activity-session-head">
        <Icon name={client === "mcp" ? "agent" : "keyboard"} size={15} />
        <strong>{client === "mcp" ? "An assistant (MCP)" : "The command line"}</strong>
        <span className="muted">
          {fullDate(oldest.at)}
          {newest.at - oldest.at > 60_000 ? ` – ${time(newest.at)}` : ""} · {entries.length}{" "}
          {entries.length === 1 ? "request" : "requests"}
          {refused > 0 && ` · ${refused} refused`}
        </span>
      </div>
      <ul className="activity-entries">
        {shown.map((entry) => (
          <Row key={entry.id} entry={entry} email={emailOf(entry.account_id)} />
        ))}
      </ul>
      {entries.length > FOLDED && (
        <button type="button" className="activity-more" onClick={() => setOpen(!open)}>
          {open ? "Show fewer" : `Show all ${entries.length}`}
        </button>
      )}
    </li>
  );
}

function Row({ entry, email }: { entry: Entry; email: string | null }) {
  const decision = DECISIONS[entry.decision];
  return (
    <li className="activity-entry">
      <details>
        <summary>
          <span className="activity-when">{time(entry.at)}</span>
          <span className="activity-what">{describe(entry)}</span>
          {email && <span className="activity-account muted">{email}</span>}
          {decision && <span className={`chip activity-decision ${decision.tone}`}>{decision.label}</span>}
          {!decision && entry.error && <span className="chip activity-decision refused">Failed</span>}
        </summary>
        <div className="activity-details">
          {entry.error && <p className={entry.decision === "denied" ? "activity-error" : "error"}>{entry.error}</p>}
          <dl>
            <dt>Request</dt>
            <dd>
              <code>{entry.method}</code> · {entry.duration_ms} ms
              {entry.access && ` · account allowed ${entry.access}`}
            </dd>
          </dl>
          {entry.args !== null && entry.args !== undefined && <pre>{JSON.stringify(entry.args, null, 2)}</pre>}
        </div>
      </details>
    </li>
  );
}
