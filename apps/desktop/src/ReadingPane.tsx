import { useEffect, useState } from "react";
import { type MessageDetail, type Person, getMessage, messageHtmlUrl } from "./accounts";

type Load = { state: "loading" } | { state: "ready"; detail: MessageDetail } | { state: "failed"; message: string };

export function ReadingPane({ messageId }: { messageId: number }) {
  const [load, setLoad] = useState<Load>({ state: "loading" });
  const [showText, setShowText] = useState(false);
  const [allowRemote, setAllowRemote] = useState(false);

  useEffect(() => {
    let current = true;
    setLoad({ state: "loading" });
    setShowText(false);
    setAllowRemote(false);
    getMessage(messageId)
      .then((detail) => current && setLoad({ state: "ready", detail }))
      .catch((error: unknown) => current && setLoad({ state: "failed", message: String(error) }));
    return () => {
      current = false;
    };
  }, [messageId]);

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
