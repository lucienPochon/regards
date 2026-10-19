import { useCallback, useEffect, useState } from "react";
import type { MessageSummary } from "./accounts";
import { Avatar } from "./Avatar";
import { useChanges } from "./changes";
import { type Draft, adoptDraft, deleteDraft, listDraftsElsewhere } from "./compose";
import { listDate, peopleShort } from "./format";
import { Icon } from "./icons";

/**
 * Messages being written, to pick up again: those written here, then those
 * written on another device or in webmail (in the server's Drafts folder),
 * which become drafts here when opened.
 */
export function DraftsList({
  drafts,
  open,
  onOpen,
  onFailed,
}: {
  drafts: Draft[];
  open: number | null;
  onOpen: (draftId: number) => void;
  onFailed: (error: unknown) => void;
}) {
  const [elsewhere, setElsewhere] = useState<MessageSummary[]>([]);
  const load = useCallback(() => {
    listDraftsElsewhere().then(setElsewhere, () => setElsewhere([]));
  }, []);
  useEffect(load, [load]);
  useChanges(load);
  const pickUp = (message: MessageSummary) =>
    adoptDraft(message.id).then((draft) => onOpen(draft.id), onFailed);

  if (drafts.length === 0 && elsewhere.length === 0) {
    return (
      <div className="list-scroll">
        <div className="empty list-empty">
          <span className="zero-mark" aria-hidden="true">
            <Icon name="draft" size={30} />
          </span>
          <h2>No drafts</h2>
          <p>Messages you start writing are kept here until you send them.</p>
        </div>
      </div>
    );
  }
  return (
    <div className="list-scroll">
      <div className="rows tall">
        <ul aria-label="Drafts">
          {drafts.map((draft) => (
            <li key={draft.id} className={draft.id === open ? "row open" : "row"}>
              <button type="button" className="row-main" onClick={() => onOpen(draft.id)}>
                <span className="dot" />
                <span className="row-avatar">
                  <Avatar person={draft.to[0] ?? null} size="md" ring />
                </span>
                <span className="from">
                  <span className="draft-mark">{draft.sending ? "Sending" : "Draft"}</span>
                  {draft.to.length > 0 ? peopleShort(draft.to) : "No recipient yet"}
                </span>
                <span className="subject">{draft.subject || "(no subject)"}</span>
                <span className="marks">{draft.attachments.length > 0 && <Icon name="paperclip" size={14} />}</span>
                <span className="date">{listDate(draft.updated_at)}</span>
              </button>
              {!draft.sending && (
                <span className="row-actions">
                  <button
                    type="button"
                    className="icon-btn small"
                    title="Discard draft"
                    aria-label="Discard draft"
                    onClick={() => deleteDraft(draft.id).catch(onFailed)}
                  >
                    <Icon name="trash" size={16} />
                  </button>
                </span>
              )}
            </li>
          ))}
        </ul>
        {elsewhere.length > 0 && (
          <section className="day" aria-label="On other devices">
            <h3 className="day-head">On other devices</h3>
            <ul aria-label="Drafts written elsewhere">
              {elsewhere.map((message) => (
                <li key={message.id} className="row">
                  <button type="button" className="row-main" onClick={() => void pickUp(message)}>
                    <span className="dot" />
                    <span className="row-avatar">
                      <Avatar person={message.to[0] ?? null} size="md" ring />
                    </span>
                    <span className="from">
                      <span className="draft-mark">Draft</span>
                      {message.to.length > 0 ? peopleShort(message.to) : "No recipient yet"}
                    </span>
                    <span className="subject">{message.subject || "(no subject)"}</span>
                    <span className="marks">{message.has_attachments && <Icon name="paperclip" size={14} />}</span>
                    <span className="date">{listDate(message.received_at)}</span>
                  </button>
                </li>
              ))}
            </ul>
          </section>
        )}
      </div>
    </div>
  );
}
