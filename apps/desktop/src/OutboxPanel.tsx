import { useCallback, useEffect, useState } from "react";
import type { Person } from "./accounts";
import { useChanges } from "./changes";
import { type OutboxEntry, dismissOutbox, listOutbox, sendAgain, undoSend } from "./compose";

/**
 * What has not gone out yet: waiting, failed, or perhaps sent. Shown only
 * when there is something; each entry says what happened and what can be done.
 */
export function OutboxPanel({ onEdit }: { onEdit: (draftId: number) => void }) {
  const [entries, setEntries] = useState<OutboxEntry[]>([]);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    listOutbox()
      .then((result) => setEntries(result.entries))
      .catch(() => {});
  }, []);
  useEffect(refresh, [refresh]);
  useChanges(refresh);

  const act = (action: () => Promise<unknown>) => {
    setError(null);
    action()
      .then(refresh)
      .catch((failure: unknown) => setError(String(failure)));
  };
  const edit = (entry: OutboxEntry) =>
    act(async () => {
      const { draft_id } = await undoSend(entry.id);
      if (draft_id !== null) onEdit(draft_id);
    });

  if (entries.length === 0) return null;
  return (
    <section className="card outbox" aria-labelledby="outbox-heading">
      <h2 id="outbox-heading">Outbox</h2>
      <ul>
        {entries.map((entry) => (
          <li key={entry.id}>
            <div>
              <strong>{entry.subject || "(no subject)"}</strong>
              <span className="muted small"> · to {names(entry.recipients)}</span>
            </div>
            <p className={entry.state === "failed" || entry.state === "uncertain" ? "error small" : "muted small"}>
              {describe(entry)}
            </p>
            <div className="actions-bar small">
              {(entry.state === "failed" || (entry.state === "queued" && entry.attempts > 0)) && (
                <button type="button" onClick={() => edit(entry)}>
                  Edit
                </button>
              )}
              {entry.state === "failed" && (
                <button type="button" onClick={() => act(() => dismissOutbox(entry.id))}>
                  Discard
                </button>
              )}
              {entry.state === "uncertain" && (
                <>
                  <button type="button" onClick={() => act(() => sendAgain(entry.id))}>
                    Send again
                  </button>
                  <button type="button" onClick={() => act(() => dismissOutbox(entry.id))}>
                    It was sent
                  </button>
                </>
              )}
            </div>
          </li>
        ))}
      </ul>
      {error && <p className="error small">{error}</p>}
    </section>
  );
}

function describe(entry: OutboxEntry): string {
  switch (entry.state) {
    case "sending":
      return "Sending…";
    case "failed":
      return `Not sent: ${entry.last_error ?? "the server refused it"}`;
    case "uncertain":
      return `It may or may not have been sent: ${entry.last_error ?? ""} Check the recipient or your Sent folder before sending it again.`;
    case "queued":
      if (entry.attempts === 0) return "Sending soon…";
      return `Not sent yet, trying again at ${new Date(entry.next_attempt_at).toLocaleTimeString()}${
        entry.last_error ? `: ${entry.last_error}` : ""
      }`;
  }
}

function names(people: Person[]): string {
  if (people.length === 0) return "nobody";
  const first = people[0].name ?? people[0].address ?? "";
  return people.length === 1 ? first : `${first} and ${people.length - 1} more`;
}
