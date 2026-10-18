import { type Ref, useCallback, useEffect, useRef, useState } from "react";
import {
  type MailboxSummary,
  type MessageDetail,
  type Person,
  type Role,
  type ThreadEntry,
  getMessage,
  getThread,
  messageHtmlUrl,
  setFlags,
} from "./accounts";
import { type Action, type Target, actionKeys, choose } from "./actions";
import { Avatar } from "./Avatar";
import { useChanges } from "./changes";
import { fileKind, fileSize, fullDate, peopleFull, peopleShort, personName } from "./format";
import { Icon } from "./icons";
import type { ListItem } from "./mail";
import { type MenuEntry, type MenuRequest, anchorOf } from "./Menu";
import { useShortcuts } from "./shortcuts";
import { leafName } from "./views";

type Load = { state: "loading" } | { state: "ready"; detail: MessageDetail } | { state: "failed"; message: string };

const disposable = (role: Role | null | undefined) => role === "trash" || role === "junk";

/** One message with its body, kept current (the body is cached, so refreshing is cheap). */
function useDetail(id: number, enabled: boolean): Load {
  const [load, setLoad] = useState<Load>({ state: "loading" });
  useEffect(() => {
    if (!enabled) return;
    let current = true;
    setLoad({ state: "loading" });
    getMessage(id)
      .then((detail) => current && setLoad({ state: "ready", detail }))
      .catch((error: unknown) => current && setLoad({ state: "failed", message: String(error) }));
    return () => {
      current = false;
    };
  }, [id, enabled]);
  const refresh = useCallback(() => {
    if (!enabled) return;
    getMessage(id)
      .then((detail) => setLoad((now) => (now.state === "ready" ? { state: "ready", detail } : now)))
      .catch(() => {});
  }, [id, enabled]);
  useChanges(refresh);
  return load;
}

/**
 * The open message and the rest of its conversation, oldest first. The open
 * one is expanded; the others are a line each until clicked.
 */
export function ReadingPane({
  item,
  folders,
  actionsFor,
  onMenu,
  onClose,
}: {
  item: ListItem;
  /** The folders of the message's account. */
  folders: MailboxSummary[];
  actionsFor: (target: Target) => Action[];
  onMenu: (request: MenuRequest) => void;
  onClose: () => void;
}) {
  const load = useDetail(item.id, true);
  const [thread, setThread] = useState<ThreadEntry[]>([]);
  const [expanded, setExpanded] = useState<Set<number>>(() => new Set([item.id]));
  const currentCard = useRef<HTMLLIElement>(null);
  const markedRead = useRef(false);

  // Opening a message marks it read (to become a setting).
  useEffect(() => {
    if (load.state !== "ready" || markedRead.current) return;
    markedRead.current = true;
    if (!load.detail.message.seen) setFlags([item.id], { seen: true }).catch(() => {});
  }, [load, item.id]);

  // The rest of the conversation is a nicety: without it, the message still shows.
  const loadThread = useCallback(() => {
    getThread(item.id)
      .then((result) => setThread(result.messages))
      .catch(() => setThread([]));
  }, [item.id]);
  useEffect(loadThread, [loadThread]);
  useChanges(loadThread);

  useEffect(() => {
    if (thread.length > 1) currentCard.current?.scrollIntoView({ block: "nearest" });
  }, [thread.length]);

  const detail = load.state === "ready" ? load.detail : null;
  const message = detail?.message ?? item;
  const mailboxId = detail?.mailbox_id ?? item.mailboxId;
  const folder = folders.find((f) => f.id === mailboxId);
  const target: Target = {
    ids: [item.id],
    single: item.id,
    accountId: item.accountId,
    mailboxId,
    seen: message.seen,
    flagged: message.flagged,
    disposable: disposable(folder?.role ?? item.role),
    pinned: message.pinned,
    aside: message.aside,
    snoozed: message.snoozed_until !== null,
  };
  const actions = actionsFor(target);
  useShortcuts({ Escape: onClose, ...(load.state === "ready" ? actionKeys(actions) : {}) });

  const entries: ThreadEntry[] =
    thread.length > 0 && thread.some((entry) => entry.message.id === item.id)
      ? thread
      : [{ message, mailbox_id: mailboxId, mailbox_name: folder?.name ?? "", mailbox_role: folder?.role ?? null }];

  const toggle = (id: number) =>
    setExpanded((now) => {
      const next = new Set(now);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const tool = (action: Action) => (
    <button
      key={action.id}
      type="button"
      className={action.danger ? "icon-btn danger" : "icon-btn"}
      title={`${action.label} (${action.keys[0]})`}
      aria-label={action.label}
      aria-pressed={action.id === "flag" ? message.flagged : action.id === "pin" ? message.pinned : undefined}
      disabled={load.state !== "ready"}
      onClick={(event) => action.run(anchorOf(event.currentTarget))}
    >
      <Icon name={action.icon} />
    </button>
  );

  return (
    <section className="reader" aria-label="Message">
      <header className="reader-bar" role="toolbar" aria-label="Message actions">
        {choose(actions, "archive", "snooze", "move", "delete").map(tool)}
        <span className="sep" />
        {choose(actions, "pin", "aside", "unread", "read", "flag").map(tool)}
        <span className="spacer" />
        {choose(actions, "reply", "replyAll", "forward").map(tool)}
        <span className="sep" />
        <button type="button" className="icon-btn" title="Close (Esc)" aria-label="Close" onClick={onClose}>
          <Icon name="close" />
        </button>
      </header>
      <div className="reader-scroll">
        <h1 className="reader-subject">{message.subject || "(no subject)"}</h1>
        <div className="reader-meta">
          {folder && (
            <span className="chip">
              <Icon name="folder" size={13} />
              {leafName(folder)}
            </span>
          )}
          {entries.length > 1 && <span className="chip">{entries.length} messages</span>}
        </div>
        {load.state === "failed" ? (
          <p className="error">{load.message}</p>
        ) : (
          <ol className="thread">
            {entries.map((entry) => {
              const current = entry.message.id === item.id;
              return (
                <ThreadMessage
                  key={entry.message.id}
                  ref={current ? currentCard : undefined}
                  entry={entry}
                  accountId={item.accountId}
                  detail={current ? load : null}
                  expanded={expanded.has(entry.message.id)}
                  otherFolder={entry.mailbox_id !== mailboxId}
                  onToggle={entries.length > 1 ? () => toggle(entry.message.id) : null}
                  actionsFor={actionsFor}
                  onMenu={onMenu}
                />
              );
            })}
          </ol>
        )}
        {load.state === "ready" && (
          <div className="reply-row">
            {choose(actions, "reply", "replyAll", "forward").map((action) => (
              <button key={action.id} type="button" className="pill" onClick={() => action.run(null)}>
                <Icon name={action.icon} size={16} />
                {action.label}
              </button>
            ))}
          </div>
        )}
      </div>
    </section>
  );
}

function ThreadMessage({
  ref,
  entry,
  accountId,
  detail: given,
  expanded,
  otherFolder,
  onToggle,
  actionsFor,
  onMenu,
}: {
  ref?: Ref<HTMLLIElement>;
  entry: ThreadEntry;
  accountId: string;
  /** The open message's body, loaded by the pane; null for the others, which load their own. */
  detail: Load | null;
  expanded: boolean;
  /** In another folder than the open message (a reply in Sent, say). */
  otherFolder: boolean;
  /** Expand or collapse; null when it is the only message. */
  onToggle: (() => void) | null;
  actionsFor: (target: Target) => Action[];
  onMenu: (request: MenuRequest) => void;
}) {
  const own = useDetail(entry.message.id, given === null && expanded);
  const load = given ?? own;
  const [showText, setShowText] = useState(false);
  const [allowRemote, setAllowRemote] = useState(false);
  const [details, setDetails] = useState(false);
  const { message } = entry;
  const ready = load.state === "ready" ? load.detail : null;

  if (!expanded) {
    return (
      <li className="mail-card collapsed" ref={ref}>
        <button type="button" className="mail-line" onClick={onToggle ?? undefined}>
          <Avatar person={message.from} size="sm" />
          <span className={message.seen ? "who" : "who unread"}>{personName(message.from)}</span>
          {otherFolder && entry.mailbox_name && <span className="chip">{entry.mailbox_name}</span>}
          <span className="when">{fullDate(message.sent_at ?? message.received_at)}</span>
        </button>
      </li>
    );
  }

  const actions = actionsFor({
    ids: [message.id],
    single: message.id,
    accountId,
    mailboxId: ready?.mailbox_id ?? entry.mailbox_id,
    seen: ready?.message.seen ?? message.seen,
    flagged: ready?.message.flagged ?? message.flagged,
    disposable: disposable(entry.mailbox_role),
    pinned: ready?.message.pinned ?? message.pinned,
    aside: ready?.message.aside ?? message.aside,
    snoozed: (ready?.message.snoozed_until ?? message.snoozed_until) !== null,
  });
  const more = (button: HTMLElement) => {
    const entries: MenuEntry[] = [
      ...choose(actions, "replyAll", "forward").map((action) => ({
        label: action.label,
        icon: action.icon,
        onSelect: () => action.run(null),
      })),
      "separator",
    ];
    if (ready?.has_html) {
      entries.push({
        label: showText ? "Show formatted" : "Show plain text",
        icon: "text",
        onSelect: () => setShowText(!showText),
      });
    }
    entries.push({ label: details ? "Hide details" : "Show details", icon: "info", onSelect: () => setDetails(!details) });
    for (const action of choose(actions, "unread", "flag", "pin")) {
      entries.push({ label: action.label, icon: action.icon, onSelect: () => action.run(null) });
    }
    onMenu({ anchor: anchorOf(button, "end"), entries, label: "More" });
  };
  const reply = choose(actions, "reply")[0];
  const to = ready?.to ?? [];
  const cc = ready?.cc ?? [];
  const attachments = ready?.attachments.filter((a) => !a.inline) ?? [];

  return (
    <li className="mail-card" ref={ref}>
      <div className="mail-head">
        <Avatar person={message.from} size="lg" />
        <button
          type="button"
          className="mail-who"
          onClick={onToggle ?? (() => setDetails(!details))}
          aria-expanded={onToggle ? true : details}
        >
          <span className="mail-from">
            <strong>{personName(message.from)}</strong>
            {message.from?.name && message.from.address && <span className="addr">{message.from.address}</span>}
          </span>
          <span className="mail-to">
            {to.length > 0 ? `to ${peopleShort(to)}` : ready ? "to undisclosed recipients" : " "}
            {cc.length > 0 && `, cc ${peopleShort(cc)}`}
          </span>
        </button>
        <time className="when">{fullDate(message.sent_at ?? message.received_at)}</time>
        {reply && (
          <button type="button" className="icon-btn" title="Reply" aria-label="Reply" onClick={() => reply.run(null)}>
            <Icon name="reply" />
          </button>
        )}
        <button
          type="button"
          className="icon-btn"
          title="More"
          aria-label="More"
          onClick={(event) => more(event.currentTarget)}
        >
          <Icon name="more" />
        </button>
      </div>
      {details && ready && <Details detail={ready} />}
      <div className="mail-body">
        {load.state === "loading" && <p className="muted">Opening…</p>}
        {load.state === "failed" && <p className="error">{load.message}</p>}
        {ready && (
          <>
            {ready.has_html && !showText && ready.remote_images > 0 && !allowRemote && (
              <div className="notice">
                <Icon name="shield" size={16} />
                <span>
                  {ready.remote_images} remote {ready.remote_images === 1 ? "image is" : "images are"} blocked. They
                  can tell the sender you opened this message.
                </span>
                <button type="button" className="btn small" onClick={() => setAllowRemote(true)}>
                  Load images
                </button>
              </div>
            )}
            {ready.has_html && !showText ? (
              // No scripts, no same-origin access, no popups or navigation: an empty sandbox.
              <iframe
                className="html-body"
                sandbox=""
                referrerPolicy="no-referrer"
                title="Message content"
                src={messageHtmlUrl(message.id, allowRemote)}
              />
            ) : (
              <pre className="text-body">{ready.text.trim() || "(no text)"}</pre>
            )}
            {attachments.length > 0 && (
              <ul className="attachments" aria-label="Attachments">
                {attachments.map((attachment, index) => (
                  <li key={`${index}-${attachment.name ?? ""}`} className="attachment">
                    <span className="file-kind">{fileKind(attachment.name, attachment.mime)}</span>
                    <span className="file-text">
                      <span className="file-name">{attachment.name ?? "(unnamed)"}</span>
                      <span className="file-size">{fileSize(attachment.size)}</span>
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </>
        )}
      </div>
    </li>
  );
}

function Details({ detail }: { detail: MessageDetail }) {
  const row = (label: string, people: Person[]) =>
    people.length > 0 && (
      <>
        <dt>{label}</dt>
        <dd>{peopleFull(people)}</dd>
      </>
    );
  const { message } = detail;
  return (
    <dl className="mail-details">
      {row("From", message.from ? [message.from] : [])}
      {row("To", detail.to)}
      {row("Cc", detail.cc)}
      <dt>Date</dt>
      <dd>{new Date(message.sent_at ?? message.received_at).toLocaleString()}</dd>
    </dl>
  );
}
