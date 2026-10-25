import { useEffect, useState } from "react";
import { personName } from "./format";
import { Icon } from "./icons";
import { type Invitation, type InvitationAnswer, answerInvitation, getInvitation, whenText } from "./invitations";

const ANSWERS: { answer: InvitationAnswer; label: string; done: string }[] = [
  { answer: "accepted", label: "Accept", done: "You accepted" },
  { answer: "tentative", label: "Maybe", done: "You said maybe" },
  { answer: "declined", label: "Decline", done: "You declined" },
];

/**
 * The invitation a message carries, over its text: what, when (in local
 * time), where and from whom, and its answers. Answering sends the
 * organizer a reply at once (iMIP); it can be changed later, which sends
 * another. A cancellation only says so.
 */
export function InvitationCard({ messageId, onFailed }: { messageId: number; onFailed: (problem: string) => void }) {
  const [invitation, setInvitation] = useState<Invitation | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let current = true;
    setInvitation(null);
    getInvitation(messageId).then(
      (found) => current && setInvitation(found),
      () => {},
    );
    return () => {
      current = false;
    };
  }, [messageId]);
  if (!invitation) return null;

  const cancelled = invitation.method === "cancel";
  const answerable = invitation.method === "request" && invitation.organizer !== null;
  const chosen = ANSWERS.find((option) => option.answer === invitation.answered);
  const answer = (choice: InvitationAnswer) => {
    setBusy(true);
    answerInvitation(messageId, choice)
      .then(setInvitation, (failure: unknown) => onFailed(String(failure)))
      .finally(() => setBusy(false));
  };
  const organizer = invitation.organizer ? personName(invitation.organizer) : null;
  const others = Math.max(0, invitation.attendees - 1);

  return (
    <section className={cancelled ? "invitation cancelled" : "invitation"} aria-label="Invitation">
      <span className="invitation-icon" aria-hidden="true">
        <Icon name="calendar" size={20} />
      </span>
      <div className="invitation-what">
        <strong>{invitation.summary || "(no title)"}</strong>
        <span>
          {cancelled ? "Cancelled · " : ""}
          {whenText(invitation.start, invitation.end)}
          {invitation.repeats ? " · repeats" : ""}
        </span>
        {invitation.location && <span className="muted">{invitation.location}</span>}
        <span className="muted small">
          {organizer && `From ${organizer}`}
          {others > 0 && ` · ${others} ${others === 1 ? "other" : "others"} invited`}
        </span>
      </div>
      {answerable && (
        <div className="invitation-answers" role="group" aria-label="Your answer">
          {chosen && (
            <span className="invitation-answered" role="status">
              {chosen.done}
            </span>
          )}
          {ANSWERS.map((option) => (
            <button
              key={option.answer}
              type="button"
              className={option.answer === invitation.answered ? "btn small primary" : "btn small"}
              aria-pressed={option.answer === invitation.answered}
              disabled={busy}
              title={`${option.label}: a reply goes to ${organizer ?? "the organizer"}`}
              onClick={() => answer(option.answer)}
            >
              {option.label}
            </button>
          ))}
        </div>
      )}
    </section>
  );
}
