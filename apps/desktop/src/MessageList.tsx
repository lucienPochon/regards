import { useCallback, useEffect, useState } from "react";
import { type MessageCursor, type MessageSummary, listMessages } from "./accounts";

export interface OpenMailbox {
  id: number;
  label: string;
  account: string;
}

export function MessageList({ mailbox }: { mailbox: OpenMailbox }) {
  const [messages, setMessages] = useState<MessageSummary[]>([]);
  const [next, setNext] = useState<MessageCursor | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  // The newest page; also refreshed every few seconds until the change feed (ADR-0007) lands.
  const loadFirst = useCallback(() => {
    listMessages(mailbox.id, null)
      .then((page) => {
        setMessages(page.messages);
        setNext(page.next);
        setError(null);
      })
      .catch((failure: unknown) => setError(String(failure)))
      .finally(() => setLoading(false));
  }, [mailbox.id]);

  useEffect(() => {
    setLoading(true);
    setMessages([]);
    loadFirst();
    const timer = window.setInterval(loadFirst, 5000);
    return () => window.clearInterval(timer);
  }, [loadFirst]);

  const loadMore = async () => {
    if (!next) {
      return;
    }
    try {
      const page = await listMessages(mailbox.id, next);
      setMessages((current) => [...current, ...page.messages]);
      setNext(page.next);
    } catch (failure) {
      setError(String(failure));
    }
  };

  return (
    <section className="card" aria-labelledby="messages-heading">
      <h2 id="messages-heading">
        {mailbox.label} <span className="muted small">{mailbox.account}</span>
      </h2>
      {error && <p className="error">{error}</p>}
      {loading && <p className="muted">Loading…</p>}
      {!loading && messages.length === 0 && !error && <p className="muted">No messages.</p>}
      <ul className="messages">
        {messages.map((message) => (
          <li key={message.id} className={message.seen ? undefined : "unread"}>
            <span className="from">{sender(message)}</span>
            <span className="subject">{message.subject || "(no subject)"}</span>
            <span className="date muted small">
              {message.flagged && <span aria-label="flagged">★ </span>}
              {formatDate(message.received_at)}
            </span>
          </li>
        ))}
      </ul>
      {next && (
        <button type="button" onClick={loadMore}>
          Load more
        </button>
      )}
    </section>
  );
}

function sender(message: MessageSummary): string {
  return message.from?.name || message.from?.address || "(unknown sender)";
}

function formatDate(ms: number): string {
  const date = new Date(ms);
  const today = new Date();
  return date.toDateString() === today.toDateString()
    ? date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })
    : date.toLocaleDateString();
}
