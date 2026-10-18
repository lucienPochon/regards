import { useCallback, useEffect, useRef, useState } from "react";
import { type MessageCursor, type MessageSummary, listMessages } from "./accounts";
import { useChanges } from "./changes";
import { useShortcuts } from "./shortcuts";

export interface OpenMailbox {
  id: number;
  label: string;
  account: string;
  accountId: string;
}

const PAGE = 50;
const MAX_PAGE = 500;

export function MessageList({
  mailbox,
  selected,
  onOpen,
}: {
  mailbox: OpenMailbox;
  selected: number | null;
  onOpen: (messageId: number) => void;
}) {
  const [messages, setMessages] = useState<MessageSummary[]>([]);
  const [next, setNext] = useState<MessageCursor | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const shown = useRef(0);
  shown.current = messages.length;

  // Re-read the newest messages, as many as are on screen (so "Load more" pages stay).
  const refresh = useCallback(() => {
    const limit = Math.min(MAX_PAGE, Math.max(PAGE, shown.current));
    listMessages(mailbox.id, null, limit)
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
    shown.current = 0;
    refresh();
  }, [refresh]);

  useChanges(refresh);

  // Next and previous; from nothing open, "next" opens the newest.
  const step = (by: number) => {
    const index = messages.findIndex((message) => message.id === selected);
    const target = index === -1 ? (by > 0 ? 0 : -1) : index + by;
    const message = messages[target];
    if (message) onOpen(message.id);
  };
  useShortcuts({
    j: () => step(1),
    ArrowDown: () => step(1),
    k: () => step(-1),
    ArrowUp: () => step(-1),
  });

  const list = useRef<HTMLUListElement>(null);
  useEffect(() => {
    list.current?.querySelector(".message.selected")?.scrollIntoView({ block: "nearest" });
  }, [selected]);

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
      <ul className="messages" ref={list}>
        {messages.map((message) => (
          <li key={message.id}>
            <button
              type="button"
              className={[
                "message",
                message.seen ? "" : "unread",
                message.id === selected ? "selected" : "",
              ].join(" ")}
              onClick={() => onOpen(message.id)}
            >
              <span className="from">{sender(message)}</span>
              <span className="subject">{message.subject || "(no subject)"}</span>
              <span className="date muted small">
                {message.flagged && <span aria-label="flagged">★ </span>}
                {formatDate(message.received_at)}
              </span>
            </button>
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
