import { useCallback, useEffect, useState } from "react";
import {
  type MailboxSummary,
  type MessageDetail,
  type MoveResult,
  type Person,
  type UndoInfo,
  archiveMessages,
  getMessage,
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
  onMoved,
}: {
  messageId: number;
  accountId: string;
  onMoved: (undo: UndoInfo | null) => void;
}) {
  const [folders, setFolders] = useState<MailboxSummary[]>([]);
  const [load, setLoad] = useState<Load>({ state: "loading" });
  const [showText, setShowText] = useState(false);
  const [allowRemote, setAllowRemote] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  useEffect(() => {
    let current = true;
    setLoad({ state: "loading" });
    setShowText(false);
    setAllowRemote(false);
    setActionError(null);
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

  // Keep the flags shown here current; the body is cached, so this is cheap.
  const refresh = useCallback(() => {
    getMessage(messageId)
      .then((detail) => setLoad((current) => (current.state === "ready" ? { state: "ready", detail } : current)))
      .catch(() => {});
  }, [messageId]);
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
        <button type="button" onClick={() => move(() => trashMessages([messageId]))}>
          Delete
        </button>
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
