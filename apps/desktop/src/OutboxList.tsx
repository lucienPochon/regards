import { useEffect, useState } from "react";
import { Avatar } from "./Avatar";
import { type OutboxEntry, dismissOutbox, reschedule, sendAgain, undoSend } from "./compose";
import { fullDate, peopleShort } from "./format";
import { Icon } from "./icons";
import { appLocale, t } from "./i18n";

/**
 * What has not gone out yet: waiting, scheduled, held, failed, or perhaps
 * sent. Each entry says what happened and what can be done.
 */
export function OutboxList({
  entries,
  onEdit,
  onFailed,
  onSignIn,
  onPickTime,
}: {
  entries: OutboxEntry[];
  onEdit: (draftId: number) => void;
  onFailed: (error: unknown) => void;
  /** Where to sign an account in again (Settings › Accounts). */
  onSignIn?: () => void;
  /** Choose another time to send, as Send later does; `null` if none was chosen. */
  onPickTime?: () => Promise<number | null>;
}) {
  // While a message can still be called back, count down to when it goes.
  const [now, setNow] = useState(() => Date.now());
  const counting = entries.some((entry) => entry.state === "queued" && entry.undo_until > now);
  useEffect(() => {
    if (!counting) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [counting]);

  if (entries.length === 0) {
    return (
      <div className="list-scroll">
        <div className="empty list-empty">
          <span className="zero-mark" aria-hidden="true">
            <Icon name="send" size={30} />
          </span>
          <h2>{t("Everything has gone out")}</h2>
          <p>{t("Messages waiting to be sent, or that could not be, show here.")}</p>
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
        <ul aria-label={t("Outbox")}>
          {entries.map((entry) => {
            const trouble = entry.state === "failed" || entry.state === "uncertain" || entry.state === "held";
            const waiting = entry.state === "held" || scheduled(entry);
            // Within its undo window (an agent's send waits there to be called back): take it back to Drafts.
            const callable = entry.state === "queued" && entry.undo_until > now && !scheduled(entry);
            return (
              <li key={entry.id} className="row outbox-row">
                <div className="row-main">
                  <span className="dot" />
                  <span className="row-avatar">
                    <Avatar person={entry.recipients[0] ?? null} size="sm" />
                  </span>
                  <span className="from">to {peopleShort(entry.recipients)}</span>
                  <span className="subject">{entry.subject || t("(no subject)")}</span>
                  <span className="row-extra">
                    {entry.by_agent && (
                      <span className="chip agent-chip">
                        <Icon name="agent" size={11} />
                        {t("An agent asked to send this")}
                      </span>
                    )}
                    <span className={trouble ? "outbox-state bad" : "outbox-state"}>{describe(entry, now)}</span>
                    <span className="outbox-actions">
                      {waiting && (
                        <button type="button" className="btn small" onClick={() => act(() => reschedule(entry.id, null))}>
                          {t("Send now")}
                        </button>
                      )}
                      {waiting && onPickTime && (
                        <button
                          type="button"
                          className="btn small"
                          onClick={() =>
                            act(async () => {
                              const time = await onPickTime();
                              if (time !== null) await reschedule(entry.id, time);
                            })
                          }
                        >
                          {t("Change time…")}
                        </button>
                      )}
                      {(waiting || entry.state === "failed" || (entry.state === "queued" && entry.attempts > 0)) && (
                        <button type="button" className="btn small" onClick={() => edit(entry)}>
                          {t("Edit")}
                        </button>
                      )}
                      {callable && (
                        <button type="button" className="btn small primary" onClick={() => act(() => undoSend(entry.id))}>
                          {t("Call it back")}
                        </button>
                      )}
                      {signedOut(entry) && onSignIn && (
                        <button type="button" className="btn small primary" onClick={onSignIn}>
                          {t("Sign in again")}
                        </button>
                      )}
                      {entry.state === "failed" && (
                        <button type="button" className="btn small" onClick={() => act(() => dismissOutbox(entry.id))}>
                          {t("Discard")}
                        </button>
                      )}
                      {entry.state === "uncertain" && (
                        <>
                          <button type="button" className="btn small" onClick={() => act(() => sendAgain(entry.id))}>
                            {t("Send again")}
                          </button>
                          <button type="button" className="btn small" onClick={() => act(() => dismissOutbox(entry.id))}>
                            {t("It was sent")}
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
  return entry.state === "queued" && entry.waiting_for_sign_in === true;
}

function describe(entry: OutboxEntry, now: number): string {
  switch (entry.state) {
    case "sending":
      return t("Sending…");
    case "failed":
      return entry.last_error ? t("Not sent: {problem}", { problem: entry.last_error }) : t("Not sent: the server refused it");
    case "uncertain":
      return t("It may or may not have been sent: {problem} Check the recipient or your Sent folder before sending it again.", {
        problem: entry.last_error ?? "",
      });
    case "held":
      return t("Not sent: it was due {when}, while this computer was off or asleep. Send it now, or edit it.", {
        when: fullDate(entry.send_at ?? entry.next_attempt_at),
      });
    case "handed_off":
      // From 1.1: a remote executor holds it.
      return t("Sends {when}, even with this computer off.", { when: fullDate(entry.send_at ?? entry.next_attempt_at) });
    case "queued":
      if (signedOut(entry)) return t("Waits for you to sign in again to its account, then goes at once.");
      if (scheduled(entry)) {
        // Sent from this computer (every send in 1.0), it must be on then (ADR-0019).
        return entry.needs_this_computer === false
          ? t("Sends {when}.", { when: fullDate(entry.send_at!) })
          : t("Sends {when}. This computer must be on then.", { when: fullDate(entry.send_at!) });
      }
      if (entry.attempts === 0) {
        const left = Math.ceil((entry.undo_until - now) / 1000);
        return left > 0 ? t("Goes in {seconds} s unless you call it back.", { seconds: left }) : t("Sending soon…");
      }
      const at = new Date(entry.next_attempt_at).toLocaleTimeString(appLocale());
      return entry.last_error
        ? t("Not sent yet, trying again at {time}: {problem}", { time: at, problem: entry.last_error })
        : t("Not sent yet, trying again at {time}", { time: at });
  }
}
