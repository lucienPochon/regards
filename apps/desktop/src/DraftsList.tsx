import { Avatar } from "./Avatar";
import { type Draft, deleteDraft } from "./compose";
import { listDate, peopleShort } from "./format";
import { Icon } from "./icons";

/** Messages being written, to pick up again. */
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
  if (drafts.length === 0) {
    return (
      <div className="list-scroll">
        <div className="empty list-empty">
          <span className="empty-mark">
            <Icon name="draft" size={26} />
          </span>
          <h2>No drafts</h2>
          <p>Messages you start writing are kept here until you send them.</p>
        </div>
      </div>
    );
  }
  return (
    <div className="list-scroll">
      <ul className="rows" aria-label="Drafts">
        {drafts.map((draft) => (
          <li key={draft.id} className={draft.id === open ? "row open" : "row"}>
            <span className="row-avatar">
              <Avatar person={draft.to[0] ?? null} />
            </span>
            <button type="button" className="row-main" onClick={() => onOpen(draft.id)}>
              <span className="row-line">
                <span className="from">
                  <span className="draft-mark">{draft.sending ? "Sending" : "Draft"}</span>
                  {draft.to.length > 0 ? peopleShort(draft.to) : "No recipient yet"}
                </span>
                <span className="date">{listDate(draft.updated_at)}</span>
              </span>
              <span className="row-line">
                <span className="subject">{draft.subject || "(no subject)"}</span>
                <span className="marks">
                  {draft.attachments.length > 0 && <Icon name="paperclip" size={14} />}
                </span>
              </span>
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
    </div>
  );
}
