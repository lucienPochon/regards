import { Avatar } from "./Avatar";
import { type OutboxEntry, dismissOutbox, reschedule, sendAgain, undoSend } from "./compose";
import { fullDate, peopleShort } from "./format";
import { Icon } from "./icons";

/**
 * What has not gone out yet: waiting, scheduled, held, failed, or perhaps
 * sent. Each entry says what happened and what can be done.
 */
export function OutboxList({
  entries,
  onEdit,
  onFailed,
  onSignIn,
}: {
  entries: OutboxEntry[];
  onEdit: (draftId: number) => void;
  onFailed: (error: unknown) => void;
  /** Where to sign an account in again (Settings › Accounts). */
  onSignIn?: () => void;
}) {
  if (entries.length === 0) {
    return (
      <div className="list-scroll">
        <div className="empty list-empty">
          <span className="zero-mark" aria-hidden="true">
            <Icon name="send" size={30} />
          </span>
          <h2>Everything has gone out</h2>
          <p>Messages waiting to be sent, or that could not be, show here.</p>
        </div>
      </div>
    );
  }
  const act = (action: () => Promise<unknown>) => {
    action().catch(onFailed);
  };
  const edit = (entry: OutboxEntry) =>
    act(async () => {
      const { draft_id } = await undoSend(entry.id);
      if (draft_id !== null) onEdit(draft_id);
    });

  return (
    <div className="list-scroll">
      <div className="rows tall">
        <ul aria-label="Outbox">
          {entries.map((entry) => {
            const trouble = entry.state === "failed" || entry.state === "uncertain" || entry.state === "held";
            const waiting = entry.state === "held" || scheduled(entry);
            return (
              <li key={entry.id} className="row outbox-row">
                <div className="row-main">
                  <span className="dot" />
                  <span className="row-avatar">
                    <Avatar person={entry.recipients[0] ?? null} size="sm" />
                  </span>
                  <span className="from">to {peopleShort(entry.recipients)}</span>
                  <span className="subject">{entry.subject || "(no subject)"}</span>
                  <span className="row-extra">
                    <span className={trouble ? "outbox-state bad" : "outbox-state"}>{describe(entry)}</span>
                    <span className="outbox-actions">
                      {waiting && (
                        <button type="button" className="btn small" onClick={() => act(() => reschedule(entry.id, null))}>
                          Send now
                        </button>
                      )}
                      {(waiting || entry.state === "failed" || (entry.state === "queued" && entry.attempts > 0)) && (
                        <button type="button" className="btn small" onClick={() => edit(entry)}>
                          Edit
                        </button>
                      )}
                      {signedOut(entry) && onSignIn && (
                        <button type="button" className="btn small primary" onClick={onSignIn}>
                          Sign in again
                        </button>
                      )}
                      {entry.state === "failed" && (
                        <button type="button" className="btn small" onClick={() => act(() => dismissOutbox(entry.id))}>
                          Discard
                        </button>
                      )}
                      {entry.state === "uncertain" && (
                        <>
                          <button type="button" className="btn small" onClick={() => act(() => sendAgain(entry.id))}>
                            Send again
                          </button>
                          <button type="button" className="btn small" onClick={() => act(() => dismissOutbox(entry.id))}>
                            It was sent
                          </button>
                        </>
                      )}
                    </span>
                  </span>
                </div>
              </li>
            );
          })}
        </ul>
      </div>
    </div>
  );
}

/** Waiting for its time, not yet tried. */
function scheduled(entry: OutboxEntry): boolean {
  return entry.state === "queued" && entry.attempts === 0 && entry.send_at !== null && entry.send_at > Date.now();
}

/** Waiting for its account to be signed in again (Microsoft or Google): it goes as soon as it is. */
function signedOut(entry: OutboxEntry): boolean {
  return entry.state === "queued" && /sign in again/i.test(entry.last_error ?? "");
}

function describe(entry: OutboxEntry): string {
  switch (entry.state) {
    case "sending":
      return "Sending…";
    case "failed":
      return `Not sent: ${entry.last_error ?? "the server refused it"}`;
    case "uncertain":
      return `It may or may not have been sent: ${entry.last_error ?? ""} Check the recipient or your Sent folder before sending it again.`;
    case "held":
      return `Not sent: it was due ${fullDate(entry.send_at ?? entry.next_attempt_at)}, while this computer was off or asleep. Send it now, or edit it.`;
    case "handed_off":
      // From 1.1: a remote executor holds it.
      return `Sends ${fullDate(entry.send_at ?? entry.next_attempt_at)}, even with this computer off.`;
    case "queued":
      if (signedOut(entry)) return "Waits for you to sign in again to its account, then goes at once.";
      if (scheduled(entry)) {
        // Sent from this computer (every send in 1.0), it must be on then (ADR-0019).
        return entry.needs_this_computer === false
          ? `Sends ${fullDate(entry.send_at!)}.`
          : `Sends ${fullDate(entry.send_at!)}. This computer must be on then.`;
      }
      if (entry.attempts === 0) return "Sending soon…";
      return `Not sent yet, trying again at ${new Date(entry.next_attempt_at).toLocaleTimeString()}${
        entry.last_error ? `: ${entry.last_error}` : ""
      }`;
  }
}
