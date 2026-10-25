import { useCallback, useEffect, useRef, useState } from "react";
import { applyUndo } from "./accounts";
import { type Hold, approveHold, listHolds, rejectHold } from "./agents";
import { useChanges } from "./changes";
import { type Draft, getDraft, undoSend } from "./compose";
import { fileSize, fullDate, peopleFull } from "./format";
import { Icon } from "./icons";
import { useDialogFocus } from "./dialogFocus";
import { VOICE_MAX } from "./identities";
import { useShortcuts } from "./shortcuts";
import { t } from "./i18n";

/** The draft's text shown in full up to this, then cut. */
const TEXT_SHOWN = 1500;

/** The text an agent wrote for a voice or a note hold: what is decided on. */
function heldText(hold: Hold): string | null {
  if (hold.reason !== "voice" && hold.reason !== "note") return null;
  const text = (hold.params as { text?: unknown } | null)?.text;
  return typeof text === "string" ? text : null;
}

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
  return hours >= 1 ? t("in {hours} h", { hours }) : t("within the hour");
};

/**
 * Settings › Agents: what agents asked for and is waiting for the person:
 * every send, changes to more than 50 messages, deleting for good. Each can
 * be approved (it runs as asked) or rejected; a send shows its draft first.
 */
export function AgentHolds({
  accounts,
  focus = null,
}: {
  accounts: { id: string; email: string }[];
  /** One a link asked to see: shown and scrolled to, even once decided. */
  focus?: number | null;
}) {
  const pending = usePendingHolds();
  // Decided here: shown with how it went until the page closes.
  const [decided, setDecided] = useState<Hold[]>([]);
  const waiting = pending.filter((hold) => !decided.some((done) => done.id === hold.id));
  // The hold a link named, when it no longer waits: how it went, or that it is gone.
  const [looked, setLooked] = useState<Hold | "gone" | null>(null);
  const focusWaits = focus !== null && pending.some((hold) => hold.id === focus);
  const focusDecidedHere = focus !== null && decided.some((hold) => hold.id === focus);
  useEffect(() => {
    setLooked(null);
    if (focus === null || focusWaits || focusDecidedHere) return;
    listHolds(false).then(
      (all) => setLooked(all.find((hold) => hold.id === focus) ?? "gone"),
      () => setLooked("gone"),
    );
  }, [focus, focusWaits, focusDecidedHere]);
  if (waiting.length === 0 && decided.length === 0 && looked === null) return null;
  const emailOf = (id: string) => accounts.find((account) => account.id === id)?.email ?? null;
  return (
    <section className="agent-holds" aria-label={t("Waiting for you")}>
      <h4>
        {t("Waiting for you")}
        {waiting.length > 0 && <span className="smart-card-count">{waiting.length}</span>}
      </h4>
      <ul>
        {looked === "gone" && (
          <li className="hold-card decided focused">
            <p className="muted">{t("That request is no longer here: requests are kept for a while after they are decided.")}</p>
          </li>
        )}
        {looked && looked !== "gone" && (
          <Outcome hold={looked} focused onDismiss={() => setLooked(null)} />
        )}
        {waiting.map((hold) => (
          <HoldCard
            key={hold.id}
            focused={hold.id === focus}
            hold={hold}
            email={accounts.length > 1 ? emailOf(hold.account_id) : null}
            onDecided={(result) => setDecided((now) => [result, ...now.filter((done) => done.id !== result.id)])}
          />
        ))}
        {decided.map((hold) => (
          <Outcome
            key={hold.id}
            hold={hold}
            focused={hold.id === focus}
            onDismiss={() => setDecided((now) => now.filter((done) => done.id !== hold.id))}
          />
        ))}
      </ul>
    </section>
  );
}

/** A card a link asked to see: brought into view once. */
function useFocused(focused: boolean) {
  const card = useRef<HTMLLIElement>(null);
  useEffect(() => {
    if (focused) card.current?.scrollIntoView({ block: "center" });
  }, [focused]);
  return card;
}

function HoldCard({
  hold,
  email,
  focused,
  onDecided,
}: {
  hold: Hold;
  email: string | null;
  focused: boolean;
  onDecided: (hold: Hold) => void;
}) {
  const card = useFocused(focused);
  const [draft, setDraft] = useState<Draft | "gone" | null>(null);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const suggested = heldText(hold);
  // A suggested voice, as the person edits it before approving.
  const [voice, setVoice] = useState<string | null>(null);
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
    hold.reason === "sending"
      ? t("Send")
      : hold.reason === "delete"
        ? t("Delete for good")
        : hold.reason === "note"
          ? t("Save anyway")
          : t("Approve");
  // Approved as edited, when it was.
  const approveAsEdited = (id: number) =>
    approveHold(id, voice !== null && voice.trim() !== (suggested ?? "").trim() ? voice.trim() : undefined);
  // A note that waits looks risky: rejecting it is the safe, main choice.
  const rejectFirst = hold.reason === "note";

  return (
    <li ref={card} className={`hold-card ${hold.reason}${focused ? " focused" : ""}`}>
      <div className="hold-head">
        <Icon name={hold.reason === "sending" ? "send" : hold.reason === "delete" ? "trash" : "agent"} size={16} />
        <span className="hold-what">
          <strong>{hold.description}</strong>
          <span className="muted">
            {hold.client === "mcp"
              ? t("An assistant (MCP) asked {when}", { when: fullDate(hold.created_at) })
              : t("The command line asked {when}", { when: fullDate(hold.created_at) })}
            {email && ` · ${email}`} · {t("expires {when}", { when: until(hold.expires_at) })}
          </span>
        </span>
      </div>
      {hold.reason === "bulk" && (
        <p className="hold-why muted">{t("More than 50 messages at once: an agent needs you for that.")}</p>
      )}
      {hold.reason === "delete" && (
        <p className="hold-why muted">{t("Deleted for good, these can't be brought back.")}</p>
      )}
      {hold.reason === "voice" &&
        suggested !== null &&
        (voice === null ? (
          <p className="hold-said">{suggested}</p>
        ) : (
          <div className="hold-edit">
            <textarea
              aria-label={t("Suggested voice")}
              rows={7}
              maxLength={VOICE_MAX}
              value={voice}
              onChange={(event) => setVoice(event.target.value)}
              autoFocus
            />
            <span className="muted small">
              {voice.length} / {VOICE_MAX.toLocaleString()}
            </span>
          </div>
        ))}
      {hold.reason === "voice" && (
        <p className="hold-why muted">{t("It describes how you write, not what you wrote about. Nothing changes until you approve.")}</p>
      )}
      {hold.reason === "note" && suggested !== null && <p className="hold-said">{suggested}</p>}
      {hold.draft_id !== null &&
        (draft === null ? (
          <p className="muted hold-why">{t("Loading the message…")}</p>
        ) : draft === "gone" ? (
          <p className="error hold-why" role="alert">{t("The draft is gone: approving won't send anything.")}</p>
        ) : (
          <div className="hold-draft">
            <dl>
              <dt>{t("From")}</dt>
              <dd>{peopleFull([draft.from])}</dd>
              <dt>{t("To")}</dt>
              <dd>{peopleFull(draft.to) || "nobody"}</dd>
              {draft.cc.length > 0 && (
                <>
                  <dt>{t("Cc")}</dt>
                  <dd>{peopleFull(draft.cc)}</dd>
                </>
              )}
              {draft.bcc.length > 0 && (
                <>
                  <dt>{t("Bcc")}</dt>
                  <dd>{peopleFull(draft.bcc)}</dd>
                </>
              )}
            </dl>
            <p className="hold-subject">{draft.subject || t("(no subject)")}</p>
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
      {problem && <p className="error hold-why" role="alert">{problem}</p>}
      <div className="hold-buttons">
        {rejectFirst ? (
          // The safe choice is the main one, at the end.
          <>
            <button type="button" className="btn small" onClick={decide(approveHold)} disabled={busy}>
              {approve}
            </button>
            <button type="button" className="btn small primary" onClick={decide(rejectHold)} disabled={busy}>
              {t("Reject")}
            </button>
          </>
        ) : (
          <>
            <button type="button" className="btn small" onClick={decide(rejectHold)} disabled={busy}>
              {t("Reject")}
            </button>
            {hold.reason === "voice" && suggested !== null && voice === null && (
              <button type="button" className="btn small" onClick={() => setVoice(suggested)} disabled={busy}>
                {t("Edit")}
              </button>
            )}
            <button
              type="button"
              className={hold.reason === "delete" ? "btn small danger-fill" : "btn small primary"}
              onClick={decide(hold.reason === "voice" ? approveAsEdited : approveHold)}
              disabled={busy || draft === "gone" || (voice !== null && voice.trim() === "")}
            >
              {approve}
            </button>
          </>
        )}
      </div>
    </li>
  );
}

/**
 * How a hold decided here went, with a way back while there is one: a
 * send's undo while it lasts (the agent may have set none), a scheduled
 * send's "take back" until it goes, or a change's undo.
 */
function Outcome({ hold, focused = false, onDismiss }: { hold: Hold; focused?: boolean; onDismiss: () => void }) {
  const card = useFocused(focused);
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
    const work = sendUndo !== null ? undoSend(sendUndo).then(() => t("Taken back: the message is in Drafts.")) : applyUndo(changeUndo ?? 0).then(() => t("Undone."));
    work.then(setUndone, (failure: unknown) => setProblem(String(failure)));
  };
  const said =
    hold.status === "approved"
      ? hold.reason === "sending"
        ? sendAt !== null && sendAt > Date.now()
          ? t("Scheduled for {when}.", { when: fullDate(sendAt) })
          : t("Sent to the Outbox.")
        : hold.reason === "voice"
          ? t("Voice saved. Agents use it from now on.")
          : hold.reason === "note"
            ? t("Saved on their card, marked as an agent's note.")
            : "Done."
      : hold.status === "rejected"
        ? hold.reason === "voice"
          ? t("Rejected: your voice did not change.")
          : t("Rejected: nothing was done.")
        : hold.status === "failed"
          ? hold.error
            ? t("Not done: {problem}", { problem: hold.error })
            : t("Not done: it could not run")
          : hold.status === "expired"
            ? t("It waited too long and expired.")
            : hold.status === "pending"
              ? t("Still waiting.")
              : "";
  return (
    <li ref={card} className={`hold-card decided ${hold.status}${focused ? " focused" : ""}`}>
      <div className="hold-head">
        <Icon name={hold.status === "approved" ? "check" : hold.status === "failed" ? "alert" : "close"} size={16} />
        <span className="hold-what">
          <strong>{hold.description}</strong>
          <span className={hold.status === "failed" ? "error" : "muted"}>{undone ?? said}</span>
          {problem && <span className="error" role="alert">{problem}</span>}
        </span>
        {!undone && hold.status === "approved" && (sendUndo !== null || changeUndo !== null) && (
          <button type="button" className="activity-undo" onClick={undo}>
            {sendUndo !== null ? (scheduled ? t("Take back") : t("Undo send")) : t("Undo")}
          </button>
        )}
        <button type="button" className="icon-btn small" title={t("Dismiss")} aria-label={t("Dismiss")} onClick={onDismiss}>
          <Icon name="close" size={13} />
        </button>
      </div>
    </li>
  );
}

/**
 * What agents wait for, over the mail (from "Agents are waiting for you"):
 * each request decided here as in Settings › Agents, which the foot opens.
 */
export function HoldsDialog({
  accounts,
  onSettings,
  onClose,
}: {
  accounts: { id: string; email: string }[];
  /** Settings › Agents: what each agent may do, and their activity. */
  onSettings: () => void;
  onClose: () => void;
}) {
  const box = useRef<HTMLElement>(null);
  useDialogFocus(box);
  useShortcuts({ Escape: onClose }, { modal: true });
  const waiting = usePendingHolds();
  return (
    <div className="backdrop" onMouseDown={onClose}>
      <section
        ref={box}
        tabIndex={-1}
        className="dialog holds-dialog"
        role="dialog"
        aria-modal="true"
        aria-label={t("What agents are waiting for")}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <AgentHolds accounts={accounts} />
        {waiting.length === 0 && <p className="muted">{t("Nothing more is waiting for you.")}</p>}
        <div className="dialog-actions">
          <button type="button" className="btn small" onClick={onSettings}>
            {t("Agent settings…")}
          </button>
          <span className="spacer" />
          <button type="button" className="btn small primary" onClick={onClose}>
            {t("Done")}
          </button>
        </div>
      </section>
    </div>
  );
}
