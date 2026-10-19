import { useCallback, useEffect, useState } from "react";
import { applyUndo } from "./accounts";
import { type Hold, approveHold, listHolds, rejectHold } from "./agents";
import { useChanges } from "./changes";
import { type Draft, getDraft, undoSend } from "./compose";
import { fileSize, fullDate, peopleFull } from "./format";
import { Icon } from "./icons";

/** The draft's text shown in full up to this, then cut. */
const TEXT_SHOWN = 1500;

/** The holds still waiting, kept current (holding and deciding are ordinary changes). */
export function usePendingHolds(): Hold[] {
  const [holds, setHolds] = useState<Hold[]>([]);
  const refresh = useCallback(() => {
    listHolds(true).then(setHolds, () => setHolds([]));
  }, []);
  useEffect(refresh, [refresh]);
  useChanges(refresh);
  return holds;
}

const until = (at: number) => {
  const hours = Math.round((at - Date.now()) / 3_600_000);
  return hours >= 1 ? `in ${hours} h` : "within the hour";
};

/**
 * Settings › Agents: what agents asked for and is waiting for the person:
 * every send, changes to more than 50 messages, deleting for good. Each can
 * be approved (it runs as asked) or rejected; a send shows its draft first.
 */
export function AgentHolds({ accounts }: { accounts: { id: string; email: string }[] }) {
  const pending = usePendingHolds();
  // Decided here: shown with how it went until the page closes.
  const [decided, setDecided] = useState<Hold[]>([]);
  const waiting = pending.filter((hold) => !decided.some((done) => done.id === hold.id));
  if (waiting.length === 0 && decided.length === 0) return null;
  const emailOf = (id: string) => accounts.find((account) => account.id === id)?.email ?? null;
  return (
    <section className="agent-holds" aria-label="Waiting for you">
      <h4>
        Waiting for you
        {waiting.length > 0 && <span className="smart-card-count">{waiting.length}</span>}
      </h4>
      <ul>
        {waiting.map((hold) => (
          <HoldCard
            key={hold.id}
            hold={hold}
            email={accounts.length > 1 ? emailOf(hold.account_id) : null}
            onDecided={(result) => setDecided((now) => [result, ...now.filter((done) => done.id !== result.id)])}
          />
        ))}
        {decided.map((hold) => (
          <Outcome key={hold.id} hold={hold} onDismiss={() => setDecided((now) => now.filter((done) => done.id !== hold.id))} />
        ))}
      </ul>
    </section>
  );
}

function HoldCard({ hold, email, onDecided }: { hold: Hold; email: string | null; onDecided: (hold: Hold) => void }) {
  const [draft, setDraft] = useState<Draft | "gone" | null>(null);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  useEffect(() => {
    if (hold.draft_id === null) return;
    getDraft(hold.draft_id).then(setDraft, () => setDraft("gone"));
  }, [hold.draft_id]);

  const decide = (call: (id: number) => Promise<Hold>) => () => {
    setBusy(true);
    setProblem(null);
    call(hold.id)
      .then(onDecided, (failure: unknown) => setProblem(String(failure)))
      .finally(() => setBusy(false));
  };
  const approve =
    hold.reason === "sending" ? "Send" : hold.reason === "delete" ? "Delete for good" : "Approve";

  return (
    <li className={`hold-card ${hold.reason}`}>
      <div className="hold-head">
        <Icon name={hold.reason === "sending" ? "send" : hold.reason === "delete" ? "trash" : "agent"} size={16} />
        <span className="hold-what">
          <strong>{hold.description}</strong>
          <span className="muted">
            {hold.client === "mcp" ? "An assistant (MCP)" : "The command line"} asked {fullDate(hold.created_at)}
            {email && ` · ${email}`} · expires {until(hold.expires_at)}
          </span>
        </span>
      </div>
      {hold.reason === "bulk" && (
        <p className="hold-why muted">More than 50 messages at once: an agent needs you for that.</p>
      )}
      {hold.reason === "delete" && (
        <p className="hold-why muted">Deleted for good, these can&apos;t be brought back.</p>
      )}
      {hold.draft_id !== null &&
        (draft === null ? (
          <p className="muted hold-why">Loading the message…</p>
        ) : draft === "gone" ? (
          <p className="error hold-why">The draft is gone: approving won&apos;t send anything.</p>
        ) : (
          <div className="hold-draft">
            <dl>
              <dt>From</dt>
              <dd>{peopleFull([draft.from])}</dd>
              <dt>To</dt>
              <dd>{peopleFull(draft.to) || "nobody"}</dd>
              {draft.cc.length > 0 && (
                <>
                  <dt>Cc</dt>
                  <dd>{peopleFull(draft.cc)}</dd>
                </>
              )}
              {draft.bcc.length > 0 && (
                <>
                  <dt>Bcc</dt>
                  <dd>{peopleFull(draft.bcc)}</dd>
                </>
              )}
            </dl>
            <p className="hold-subject">{draft.subject || "(no subject)"}</p>
            <pre className="hold-text">
              {draft.body.length > TEXT_SHOWN ? `${draft.body.slice(0, TEXT_SHOWN)}…` : draft.body}
            </pre>
            {draft.attachments.length > 0 && (
              <ul className="hold-files">
                {draft.attachments.map((file) => (
                  <li key={file.id}>
                    <Icon name="paperclip" size={13} />
                    {file.name} <span className="muted">{fileSize(file.size)}</span>
                  </li>
                ))}
              </ul>
            )}
          </div>
        ))}
      {problem && <p className="error hold-why">{problem}</p>}
      <div className="hold-buttons">
        <button type="button" className="btn small" onClick={decide(rejectHold)} disabled={busy}>
          Reject
        </button>
        <button
          type="button"
          className={hold.reason === "delete" ? "btn small danger-fill" : "btn small primary"}
          onClick={decide(approveHold)}
          disabled={busy || draft === "gone"}
        >
          {approve}
        </button>
      </div>
    </li>
  );
}

/**
 * How a hold decided here went, with a way back while there is one: a
 * send's undo while it lasts (the agent may have set none), a scheduled
 * send's "take back" until it goes, or a change's undo.
 */
function Outcome({ hold, onDismiss }: { hold: Hold; onDismiss: () => void }) {
  const [undone, setUndone] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const result = (hold.result ?? {}) as {
    outbox_id?: unknown;
    undo_until?: unknown;
    send_at?: unknown;
    undo?: { id?: unknown } | null;
  };
  const outbox = typeof result.outbox_id === "number" ? result.outbox_id : null;
  const undoUntil = typeof result.undo_until === "number" ? result.undo_until : 0;
  const sendAt = typeof result.send_at === "number" ? result.send_at : null;
  const scheduled = outbox !== null && sendAt !== null && sendAt > now;
  const sendUndo = outbox !== null && (scheduled || undoUntil > now) ? outbox : null;
  const changeUndo = typeof result.undo?.id === "number" ? result.undo.id : null;
  // Look again when the way back ends, so no dead button stays.
  useEffect(() => {
    const ends = scheduled ? (sendAt ?? 0) : undoUntil;
    if (outbox === null || ends <= now) return;
    const timer = window.setTimeout(() => setNow(Date.now()), Math.min(ends - now + 50, 2 ** 31 - 1));
    return () => window.clearTimeout(timer);
  }, [outbox, scheduled, sendAt, undoUntil, now]);
  const undo = () => {
    setProblem(null);
    const work = sendUndo !== null ? undoSend(sendUndo).then(() => "Taken back: the message is in Drafts.") : applyUndo(changeUndo ?? 0).then(() => "Undone.");
    work.then(setUndone, (failure: unknown) => setProblem(String(failure)));
  };
  const said =
    hold.status === "approved"
      ? hold.reason === "sending"
        ? sendAt !== null && sendAt > Date.now()
          ? `Scheduled for ${fullDate(sendAt)}.`
          : "Sent to the Outbox."
        : "Done."
      : hold.status === "rejected"
        ? "Rejected: nothing was done."
        : hold.status === "failed"
          ? `Not done: ${hold.error ?? "it could not run"}`
          : hold.status === "expired"
            ? "It waited too long and expired."
            : "";
  return (
    <li className={`hold-card decided ${hold.status}`}>
      <div className="hold-head">
        <Icon name={hold.status === "approved" ? "check" : hold.status === "failed" ? "alert" : "close"} size={16} />
        <span className="hold-what">
          <strong>{hold.description}</strong>
          <span className={hold.status === "failed" ? "error" : "muted"}>{undone ?? said}</span>
          {problem && <span className="error">{problem}</span>}
        </span>
        {!undone && hold.status === "approved" && (sendUndo !== null || changeUndo !== null) && (
          <button type="button" className="activity-undo" onClick={undo}>
            {sendUndo !== null ? (scheduled ? "Take back" : "Undo send") : "Undo"}
          </button>
        )}
        <button type="button" className="icon-btn small" title="Dismiss" aria-label="Dismiss" onClick={onDismiss}>
          <Icon name="close" size={13} />
        </button>
      </div>
    </li>
  );
}
