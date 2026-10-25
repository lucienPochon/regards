import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { applyUndo } from "./accounts";
import { type ActivityKind, type AgentActivity as Entry, type AgentDecision, activityKind, listAgentActivity } from "./agents";
import { fullDate } from "./format";
import { Icon } from "./icons";
import { radioKeys, radioTab } from "./radios";
import { t } from "./i18n";

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
  "hold.get": "Checked on what it asked for",
};

/** Which requests the list shows: all, those of one kind, or the refused ones. */
type Filter = "all" | ActivityKind | "refused";
const FILTERS: { id: Filter; label: string; none: string }[] = [
  { id: "all", label: t("Everything"), none: t("No agent has used Regards yet. What they do will be listed here.") },
  { id: "reading", label: t("Reading"), none: t("No agent has read any mail.") },
  { id: "sorting", label: t("Sorting"), none: t("No agent has moved or marked any mail.") },
  { id: "sending", label: t("Sending"), none: t("No agent has written or sent anything.") },
  { id: "refused", label: t("Refused"), none: t("No agent has been refused.") },
];

const DECISIONS: Partial<Record<AgentDecision, { label: string; tone: string }>> = {
  denied: { label: t("Refused"), tone: "refused" },
  held: { label: t("Waiting for you"), tone: "held" },
  approved: { label: t("Approved"), tone: "approved" },
  rejected: { label: t("Rejected"), tone: "refused" },
};

/** The daemon's answer to an agent going faster than its pace (RATE_LIMITED). */
const RATE_LIMITED = -32015;
const slowed = (entry: Entry) => entry.decision === "denied" && entry.error_code === RATE_LIMITED;

/** How a request was answered, for its chip: refused, or only slowed down. */
function decisionOf(entry: Entry): { label: string; tone: string } | undefined {
  if (slowed(entry)) {
    // Too many at once, or too many already waiting for the person.
    return /wait/i.test(entry.error ?? "") && !/try again/i.test(entry.error ?? "")
      ? { label: t("Too many waiting"), tone: "held" }
      : { label: t("Too fast"), tone: "held" };
  }
  return DECISIONS[entry.decision];
}

/** The action in words: what was searched for, which flags, how many messages. */
function describe(entry: Entry): string {
  const args = (entry.args ?? {}) as { query?: unknown; ids?: unknown; seen?: unknown; flagged?: unknown };
  let action = ACTIONS[entry.method] ?? entry.method;
  if (entry.method === "message.set_flags") {
    const parts = [
      args.seen === true ? t("Marked read") : args.seen === false ? t("Marked unread") : null,
      args.flagged === true ? "flagged" : args.flagged === false ? "unflagged" : null,
    ].filter(Boolean) as string[];
    if (parts.length > 0) action = parts.length === 2 ? `${parts[0]} and ${parts[1]}` : parts[0].replace(/^f/, "F").replace(/^u/, "U");
  }
  if (entry.method.startsWith("search.") && typeof args.query === "string" && args.query) {
    return `${action} “${args.query}”`;
  }
  const count = Array.isArray(args.ids) ? args.ids.length : 0;
  return count > 1 ? `${action}: ${count} messages` : action;
}

/** How one thing an agent changed stood before: a message, a draft or a sender. */
function wasText(item: Record<string, unknown>, folderName: (id: number) => string | null): string {
  if (typeof item.address === "string") {
    const policy = typeof item.policy === "string" ? `always ${item.policy}` : "sorted automatically";
    return [item.address, policy, item.priority ? "priority" : null, item.blocked ? "blocked" : null].filter(Boolean).join(", ");
  }
  const subject = typeof item.subject === "string" && item.subject ? `“${item.subject}”` : "(no subject)";
  if ("sending" in item || "bcc" in item) return t("The draft {subject}", { subject });
  const folder = typeof item.mailbox_id === "number" ? folderName(item.mailbox_id) : null;
  return [
    subject,
    folder ? `in ${folder}` : null,
    item.seen === false ? "unread" : item.seen === true ? "read" : null,
    item.flagged ? "flagged" : null,
    item.pinned ? "pinned" : null,
    item.aside ? "set aside" : null,
    item.snoozed_until ? "snoozed" : null,
    item.remind_at ? "with a reminder" : null,
  ]
    .filter(Boolean)
    .join(", ");
}

const time = (at: number) => new Date(at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });

/**
 * Requests made through one agent connection under one heading, the
 * sessions ordered by their newest request. Two agents at once interleave
 * in the log, so a session is gathered from everything loaded, not from
 * runs of neighbours.
 */
function bySession(entries: Entry[]): { session: string; client: string; entries: Entry[] }[] {
  const groups = new Map<string, { session: string; client: string; entries: Entry[] }>();
  for (const entry of entries) {
    const group = groups.get(entry.session);
    if (group) group.entries.push(entry);
    else groups.set(entry.session, { session: entry.session, client: entry.client, entries: [entry] });
  }
  return [...groups.values()];
}

/**
 * Settings › Agents: every request agents made, allowed or refused, newest
 * first and grouped by session; kept 90 days by the daemon.
 */
export function AgentActivity({
  accounts,
  folderName,
}: {
  accounts: { id: string; email: string }[];
  /** A folder's name, for "Was: in Archive". */
  folderName: (mailboxId: number) => string | null;
}) {
  const [accountId, setAccountId] = useState<string | null>(null);
  const [only, setOnly] = useState<Filter>("all");
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
          const latest = new Map(page.entries.map((entry) => [entry.id, entry]));
          const fresh = page.entries.filter((entry) => !known.has(entry.id));
          return [...fresh, ...now.map((entry) => latest.get(entry.id) ?? entry)];
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
    () =>
      (entries ?? []).filter((entry) =>
        only === "all"
          ? true
          : only === "refused"
            ? entry.decision === "denied" || entry.decision === "rejected"
            : activityKind(entry.method) === only,
      ),
    [entries, only],
  );
  const emailOf = (id: string | null) => accounts.find((account) => account.id === id)?.email ?? null;

  return (
    <section className="agent-activity" aria-label={t("Agent activity")}>
      <div className="activity-head">
        <h4>{t("Activity")}</h4>
        <span className="choice" role="radiogroup" aria-label={t("Which requests")} onKeyDown={(event) => radioKeys(event)}>
          {FILTERS.map((option, index) => (
            <button
              key={option.label}
              type="button"
              role="radio"
              aria-checked={only === option.id}
              tabIndex={radioTab(only === option.id, index, true)}
              className={only === option.id ? "chosen" : undefined}
              onClick={() => setOnly(option.id)}
            >
              {option.label}
            </button>
          ))}
        </span>
        {accounts.length > 1 && (
          <select
            className="agent-level"
            aria-label={t("Which account")}
            value={accountId ?? ""}
            onChange={(event) => setAccountId(event.target.value || null)}
          >
            <option value="">{t("All accounts")}</option>
            {accounts.map((account) => (
              <option key={account.id} value={account.id}>
                {account.email}
              </option>
            ))}
          </select>
        )}
      </div>
      {error && <p className="error" role="alert">{error}</p>}
      {entries === null ? (
        !error && <p className="muted activity-empty">{t("Loading…")}</p>
      ) : shown.length === 0 ? (
        <p className="muted activity-empty">
          {FILTERS.find((option) => option.id === only)?.none}
        </p>
      ) : (
        <ol className="activity-sessions">
          {bySession(shown).map((group) => (
            <Session
              key={group.session}
              client={group.client}
              entries={group.entries}
              emailOf={accounts.length > 1 ? emailOf : () => null}
              folderName={folderName}
              onChanged={refresh}
            />
          ))}
        </ol>
      )}
      {next !== null && entries !== null && (
        <button type="button" className="btn small activity-older" onClick={older} disabled={loadingOlder}>
          {loadingOlder ? t("Loading…") : t("Show older")}
        </button>
      )}
    </section>
  );
}

function Session({
  client,
  entries,
  emailOf,
  folderName,
  onChanged,
}: {
  client: string;
  entries: Entry[];
  emailOf: (id: string | null) => string | null;
  folderName: (mailboxId: number) => string | null;
  onChanged: () => void;
}) {
  const [open, setOpen] = useState(false);
  const newest = entries[0];
  const oldest = entries[entries.length - 1];
  const refused = entries.filter((entry) => entry.decision === "denied" && !slowed(entry)).length;
  const tooFast = entries.filter(slowed).length;
  const shown = open ? entries : entries.slice(0, FOLDED);
  return (
    <li className="activity-session">
      <div className="activity-session-head">
        <Icon name={client === "mcp" ? "agent" : "keyboard"} size={15} />
        <strong>{client === "mcp" ? t("An assistant (MCP)") : t("The command line")}</strong>
        <span className="muted">
          {fullDate(oldest.at)}
          {newest.at - oldest.at > 60_000 ? ` – ${time(newest.at)}` : ""} · {entries.length}{" "}
          {entries.length === 1 ? "request" : "requests"}
          {refused > 0 && ` · ${refused} refused`}
          {tooFast > 0 && ` · ${t("{count} slowed down", { count: tooFast })}`}
        </span>
      </div>
      <ul className="activity-entries">
        {shown.map((entry) => (
          <Row key={entry.id} entry={entry} email={emailOf(entry.account_id)} folderName={folderName} onChanged={onChanged} />
        ))}
      </ul>
      {entries.length > FOLDED && (
        <button type="button" className="activity-more" onClick={() => setOpen(!open)}>
          {open ? t("Show fewer") : t("Show all {count}", { count: entries.length })}
        </button>
      )}
    </li>
  );
}

function Row({
  entry,
  email,
  folderName,
  onChanged,
}: {
  entry: Entry;
  email: string | null;
  folderName: (mailboxId: number) => string | null;
  onChanged: () => void;
}) {
  const decision = decisionOf(entry);
  // What was done about it here: undone, or why that failed.
  const [done, setDone] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const before = Array.isArray(entry.before)
    ? (entry.before as Record<string, unknown>[])
    : entry.before && typeof entry.before === "object"
      ? [entry.before as Record<string, unknown>]
      : [];

  const act = (label: string, work: () => Promise<unknown>) => (event: { preventDefault: () => void; stopPropagation: () => void }) => {
    // Inside the row's summary: act, without opening the details.
    event.preventDefault();
    event.stopPropagation();
    setBusy(true);
    setProblem(null);
    work()
      .then(
        () => {
          setDone(label);
          onChanged();
        },
        (failure: unknown) => setProblem(String(failure)),
      )
      .finally(() => setBusy(false));
  };
  const undo = act("Undone", () => applyUndo(entry.undo_id ?? 0));

  return (
    <li className="activity-entry">
      <details>
        <summary>
          <span className="activity-when">{time(entry.at)}</span>
          <span className="activity-what">{describe(entry)}</span>
          {email && <span className="activity-account muted">{email}</span>}
          {decision && <span className={`chip activity-decision ${decision.tone}`}>{decision.label}</span>}
          {!decision && entry.error && <span className="chip activity-decision refused">{t("Failed")}</span>}
          {done ? (
            <span className="chip activity-decision approved">{done}</span>
          ) : entry.undoable && entry.undo_id !== null ? (
            <button type="button" className="activity-undo" onClick={undo} disabled={busy}>
              {t("Undo")}
            </button>
          ) : null}
        </summary>
        {problem && <p className="error activity-problem" role="alert">{problem}</p>}
        <div className="activity-details">
          {entry.error && <p className={entry.decision === "denied" ? "activity-error" : "error"}>{entry.error}</p>}
          <dl>
            <dt>{t("Request")}</dt>
            <dd>
              <code>{entry.method}</code> · {entry.duration_ms} ms
              {entry.access && ` · ${t("account allowed {level}", { level: entry.access })}`}
            </dd>
          </dl>
          {before.length > 0 && (
            <dl className="activity-before">
              <dt>{t("Was")}</dt>
              <dd>
                <ul>
                  {before.slice(0, 10).map((item, index) => (
                    <li key={index}>{wasText(item, folderName)}</li>
                  ))}
                  {before.length > 10 && <li className="muted">and {before.length - 10} more</li>}
                </ul>
              </dd>
            </dl>
          )}
          {entry.args !== null && entry.args !== undefined && <pre>{JSON.stringify(entry.args, null, 2)}</pre>}
        </div>
      </details>
    </li>
  );
}
