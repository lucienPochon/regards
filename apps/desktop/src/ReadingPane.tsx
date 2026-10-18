import { useCallback, useEffect, useState } from "react";
import {
  type MailboxSummary,
  type MessageDetail,
  type MoveResult,
  type Person,
  type ThreadEntry,
  type UndoInfo,
  archiveMessages,
  deleteForever,
  getMessage,
  getThread,
  listMailboxes,
  messageHtmlUrl,
  moveMessages,
  setFlags,
  trashMessages,
} from "./accounts";
import { useChanges } from "./changes";

type Load = { state: "loading" } | { state: "ready"; detail: MessageDetail } | { state: "failed"; message: string };

export function ReadingPane({
  messageId,
  accountId,
  onOpenMessage,
  onMoved,
}: {
  messageId: number;
  accountId: string;
  onOpenMessage: (id: number) => void;
  onMoved: (undo: UndoInfo | null) => void;
}) {
  const [folders, setFolders] = useState<MailboxSummary[]>([]);
  const [load, setLoad] = useState<Load>({ state: "loading" });
  const [thread, setThread] = useState<ThreadEntry[]>([]);
  const [showText, setShowText] = useState(false);
  const [allowRemote, setAllowRemote] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [confirmingDelete, setConfirmingDelete] = useState(false);

  useEffect(() => {
    let current = true;
    setLoad({ state: "loading" });
    setShowText(false);
    setAllowRemote(false);
    setActionError(null);
    setConfirmingDelete(false);
    getMessage(messageId)
      .then((detail) => {
        if (!current) return;
        setLoad({ state: "ready", detail });
        // Opening a message marks it read (to become a setting).
        if (!detail.message.seen) {
          setFlags([messageId], { seen: true }).catch((error: unknown) => setActionError(String(error)));
        }
      })
      .catch((error: unknown) => current && setLoad({ state: "failed", message: String(error) }));
    return () => {
      current = false;
    };
  }, [messageId]);

  // The rest of the conversation is a nicety: without it, the message still shows.
  const loadThread = useCallback(() => {
    getThread(messageId)
      .then((result) => setThread(result.messages))
      .catch(() => setThread([]));
  }, [messageId]);
  useEffect(() => {
    setThread([]);
    loadThread();
  }, [loadThread]);

  // Keep the flags shown here current; the body is cached, so this is cheap.
  const refresh = useCallback(() => {
    getMessage(messageId)
      .then((detail) => setLoad((current) => (current.state === "ready" ? { state: "ready", detail } : current)))
      .catch(() => {});
    loadThread();
  }, [messageId, loadThread]);
  useChanges(refresh);

  useEffect(() => {
    listMailboxes(accountId)
      .then((result) => setFolders(result.mailboxes.filter((m) => m.selectable)))
      .catch(() => setFolders([]));
  }, [accountId]);

  const move = (action: () => Promise<MoveResult>) => {
    setActionError(null);
    action()
      .then((result) => onMoved(result.undo))
      .catch((error: unknown) => setActionError(String(error)));
  };

  const act = (flags: { seen?: boolean; flagged?: boolean }) => {
    setActionError(null);
    setFlags([messageId], flags).catch((error: unknown) => setActionError(String(error)));
  };

  const deleteForGood = () => {
    setActionError(null);
    deleteForever([messageId])
      .then(() => onMoved(null))
      .catch((error: unknown) => {
        setConfirmingDelete(false);
        setActionError(String(error));
      });
  };

  if (load.state === "loading") {
    return (
      <section className="card">
        <p className="muted">Opening…</p>
      </section>
    );
  }
  if (load.state === "failed") {
    return (
      <section className="card">
        <p className="error">{load.message}</p>
      </section>
    );
  }
  const { detail } = load;
  const { message } = detail;
  const attachments = detail.attachments.filter((a) => !a.inline);
  // In Trash or Junk, deleting means for good.
  const shownIn = folders.find((folder) => folder.id === detail.mailbox_id);
  const disposable = shownIn?.role === "trash" || shownIn?.role === "junk";
  return (
    <article className="card reading" aria-labelledby="reading-subject">
      <h2 id="reading-subject">{message.subject || "(no subject)"}</h2>
      <div className="actions-bar">
        <button type="button" onClick={() => act({ seen: !message.seen })}>
          {message.seen ? "Mark unread" : "Mark read"}
        </button>
        <button type="button" onClick={() => act({ flagged: !message.flagged })} aria-pressed={message.flagged}>
          {message.flagged ? "Unflag" : "Flag"}
        </button>
        <button type="button" onClick={() => move(() => archiveMessages([messageId]))}>
          Archive
        </button>
        {disposable ? (
          <button type="button" onClick={() => setConfirmingDelete(true)} disabled={confirmingDelete}>
            Delete forever
          </button>
        ) : (
          <button type="button" onClick={() => move(() => trashMessages([messageId]))}>
            Delete
          </button>
        )}
        <select
          aria-label="Move to folder"
          value=""
          onChange={(event) => {
            const target = Number(event.target.value);
            if (target) move(() => moveMessages([messageId], target));
          }}
        >
          <option value="">Move to…</option>
          {folders.map((folder) => (
            <option key={folder.id} value={folder.id}>
              {folder.name}
            </option>
          ))}
        </select>
        {actionError && <span className="error small">{actionError}</span>}
      </div>
      {confirmingDelete && (
        <div className="confirm small" role="alertdialog" aria-labelledby="confirm-delete">
          <span id="confirm-delete">Delete this message for good? This cannot be undone.</span>
          <button type="button" className="danger" onClick={deleteForGood}>
            Delete forever
          </button>
          <button type="button" onClick={() => setConfirmingDelete(false)}>
            Cancel
          </button>
        </div>
      )}
      <dl className="headers small">
        <dt>From</dt>
        <dd>{people(message.from ? [message.from] : [])}</dd>
        {detail.to.length > 0 && (
          <>
            <dt>To</dt>
            <dd>{people(detail.to)}</dd>
          </>
        )}
        {detail.cc.length > 0 && (
          <>
            <dt>Cc</dt>
            <dd>{people(detail.cc)}</dd>
          </>
        )}
        <dt>Date</dt>
        <dd>{new Date(message.sent_at ?? message.received_at).toLocaleString()}</dd>
      </dl>
      {thread.length > 1 && (
        <nav className="conversation small" aria-label="Conversation">
          <h3>Conversation · {thread.length} messages</h3>
          <ol>
            {thread.map((entry) => {
              const current = entry.message.id === messageId;
              const when = new Date(entry.message.sent_at ?? entry.message.received_at).toLocaleString();
              return (
                <li key={entry.message.id}>
                  <button
                    type="button"
                    className={current ? "current" : undefined}
                    aria-current={current ? "true" : undefined}
                    disabled={current}
                    onClick={() => onOpenMessage(entry.message.id)}
                  >
                    <span className={entry.message.seen ? "who" : "who unread"}>
                      {entry.message.from?.name ?? entry.message.from?.address ?? "(unknown sender)"}
                    </span>
                    <span className="muted"> · {when}</span>
                    <span className="folder">{entry.mailbox_name}</span>
                  </button>
                </li>
              );
            })}
          </ol>
        </nav>
      )}
      {detail.has_html && (
        <div className="view-options small">
          <button type="button" onClick={() => setShowText((text) => !text)}>
            {showText ? "Show formatted" : "Show plain text"}
          </button>
          {!showText && detail.remote_images > 0 && !allowRemote && (
            <>
              <span className="muted">
                {detail.remote_images} remote {detail.remote_images === 1 ? "image" : "images"} blocked. They can
                tell the sender you opened this message.
              </span>
              <button type="button" onClick={() => setAllowRemote(true)}>
                Load images
              </button>
            </>
          )}
        </div>
      )}
      {detail.has_html && !showText ? (
        // No scripts, no same-origin access, no popups or navigation: an empty sandbox.
        <iframe
          className="html-body"
          sandbox=""
          referrerPolicy="no-referrer"
          title="Message content"
          src={messageHtmlUrl(message.id, allowRemote)}
        />
      ) : (
        <pre className="body">{detail.text.trim() || "(no text)"}</pre>
      )}
      {attachments.length > 0 && (
        <>
          <h3 className="small">Attachments</h3>
          <ul className="attachments small">
            {attachments.map((attachment, index) => (
              <li key={`${index}-${attachment.name ?? ""}`}>
                {attachment.name ?? "(unnamed)"} <span className="muted">· {attachment.mime} · {size(attachment.size)}</span>
              </li>
            ))}
          </ul>
        </>
      )}
    </article>
  );
}

function people(list: Person[]): string {
  return list.map((p) => (p.name && p.address ? `${p.name} <${p.address}>` : (p.name ?? p.address ?? ""))).join(", ");
}

function size(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
