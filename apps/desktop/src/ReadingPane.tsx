import { type Ref, useCallback, useEffect, useMemo, useRef, useState } from "react";
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
import { type Action, type Target, actionKeys, actionTitle, choose } from "./actions";
import { AttachmentTiles } from "./AttachmentTiles";
import { Avatar } from "./Avatar";
import { useChanges } from "./changes";
import { fullDate, peopleFull, personName } from "./format";
import { Icon } from "./icons";
import type { ListItem } from "./mail";
import { type Anchor, type MenuEntry, anchorOf } from "./Menu";
import { MARK_READ_AFTER, STEP_DWELL, getPrefs, usePrefs } from "./prefs";
import { senderImages, setSenderImages } from "./smart";
import { asMarkdown, printMessage, textParts } from "./messageText";
import { openUrl } from "./background";
import { useDark } from "./theme";
import { SourceDialog } from "./SourceDialog";
import { MOD_KEY, useShortcuts } from "./shortcuts";

type Load = { state: "loading" } | { state: "ready"; detail: MessageDetail } | { state: "failed"; message: string };

const disposable = (role: Role | null | undefined) => role === "trash" || role === "junk";

/** How tall a message's frame may grow, against pages that grow with it (`100vh`). */
const MAX_FRAME_HEIGHT = 30_000;

/** The command palette for these actions, with any extra entries (a message's own view options). */
export type Palette = (actions: Action[], title: string, extra?: MenuEntry[]) => void;

/** The sender card for one of the thread's messages. */
export type SenderOpener = (anchor: Anchor, messageId: number, person: Person) => void;

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

/** Up to two names, then "+N": "Ada, Bob +3". */
function recipients(people: Person[]): string {
  if (people.length === 0) return "";
  const names = people.slice(0, 2).map((person) => personName(person));
  return people.length > 2 ? `${names.join(", ")} +${people.length - 2}` : names.join(", ");
}

/**
 * The open message and the rest of its conversation, oldest first, as cards.
 * The open one is expanded; the others are one line each until clicked.
 */
export function ReadingPane({
  item,
  folders,
  mode,
  actionsFor,
  onPalette,
  onSender,
  onAssistant,
  assistant,
  onClose,
  onStep,
  stepped = false,
}: {
  item: ListItem;
  /** The folders of the message's account. */
  folders: MailboxSummary[];
  /** "single": the message is shown in place of the list, with a way back. */
  mode: "single" | "split";
  actionsFor: (target: Target) => Action[];
  onPalette: Palette;
  onSender?: SenderOpener;
  /** Ask the assistant about a message; absent while it is off. */
  onAssistant?: (messageId: number, kind: "summary" | "reply") => void;
  /** The assistant's button at the bar's end, named for its provider ("Claude"), with its menu. */
  assistant?: { name: string; onMenu: (anchor: Anchor) => void };
  onClose: () => void;
  /** The next (1) or previous (-1) message of the list, when the list is not beside it. */
  onStep?: (by: number) => void;
  /** Opened by stepping through the list with the keyboard: read only if one stays. */
  stepped?: boolean;
}) {
  const load = useDetail(item.id, true);
  const [thread, setThread] = useState<ThreadEntry[]>([]);
  const [expanded, setExpanded] = useState<Set<number>>(() => new Set([item.id]));
  const prefs = usePrefs();
  const currentCard = useRef<HTMLLIElement>(null);
  const markedRead = useRef(false);

  // An opened message is marked read as Settings › General says: at once, after
  // a few seconds, or never; one only passed with the keyboard stays unread.
  useEffect(() => {
    if (load.state !== "ready" || markedRead.current || load.detail.message.seen) return;
    const after = MARK_READ_AFTER[prefs.markRead];
    if (after === null) return;
    const timer = window.setTimeout(
      () => {
        markedRead.current = true;
        setFlags([item.id], { seen: true }).catch(() => {});
      },
      stepped ? Math.max(after, STEP_DWELL) : after,
    );
    return () => window.clearTimeout(timer);
  }, [load, item.id, prefs.markRead, stepped]);

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
    reminding: message.remind_at !== null,
    reminded: message.reminded,
    done: item.done === true,
  };
  const actions = actionsFor(target);
  const done = choose(actions, "archive")[0];
  const subject = message.subject || "(no subject)";
  const standard = prefs.keySet === "standard";
  useShortcuts({
    Escape: onClose,
    ...(onStep ? { j: () => onStep(1), k: () => onStep(-1) } : {}),
    ...(onStep && standard ? { ArrowRight: () => onStep(1), ArrowLeft: () => onStep(-1) } : {}),
    ...(load.state === "ready" ? actionKeys(actions) : {}),
    ...(load.state === "ready" && standard ? { p: () => printMessage(load.detail) } : {}),
  });

  const entries: ThreadEntry[] =
    thread.length > 0 && thread.some((entry) => entry.message.id === item.id)
      ? thread
      : [{ message, mailbox_id: mailboxId, mailbox_name: folder?.name ?? "", mailbox_role: folder?.role ?? null }];

  // Settings › General: older messages folded to one line, or all open.
  const allIds = entries.map((entry) => entry.message.id).join(",");
  useEffect(() => {
    if (!prefs.collapseOlder && allIds) setExpanded(new Set(allIds.split(",").map(Number)));
  }, [prefs.collapseOlder, allIds]);

  // Every message of the conversation open, or the older ones folded again (Shift+O, O).
  const conversation =
    entries.length > 1
      ? {
          openAll: () => setExpanded(new Set(entries.map((entry) => entry.message.id))),
          foldOlder: () => setExpanded(new Set([item.id])),
        }
      : undefined;
  useShortcuts(conversation ? { "Shift+O": conversation.openAll, o: conversation.foldOlder } : {});

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
      className="icon-btn"
      title={actionTitle(action)}
      aria-label={action.label}
      aria-pressed={action.id === "flag" ? message.flagged : action.id === "pin" ? message.pinned : undefined}
      disabled={load.state !== "ready"}
      onClick={(event) => action.run(anchorOf(event.currentTarget, "end"))}
    >
      <Icon name={action.icon} size={17} />
    </button>
  );

  return (
    <section className={`reader ${mode}`} aria-label="Message">
      <header className="reader-bar" role="toolbar" aria-label="Message actions" data-tauri-drag-region>
        <div className="segmented">
          <button
            type="button"
            title={mode === "single" ? "Back to the list (Esc)" : "Close (Esc)"}
            aria-label={mode === "single" ? "Back to the list" : "Close"}
            onClick={onClose}
          >
            <Icon name={mode === "single" ? "back" : "close"} size={14} />
          </button>
          {done && (
            <button
              type="button"
              title={`Done: ${actionTitle(done)}`}
              aria-label={done.label}
              disabled={load.state !== "ready"}
              onClick={() => done.run(null)}
            >
              <Icon name="check" size={14} />
            </button>
          )}
        </div>
        <div className="reader-tools">
          {choose(actions, "delete", "flag", "unread", "read", "pin", "aside", "snooze").map(tool)}
          <button
            type="button"
            className="icon-btn"
            title={`More (${MOD_KEY}+K)`}
            aria-label="More"
            disabled={load.state !== "ready"}
            onClick={() => onPalette(actions, subject)}
          >
            <Icon name="more" size={17} />
          </button>
        </div>
        {assistant && (
          <button
            type="button"
            className="reader-ask"
            title={`Ask ${assistant.name} about this conversation`}
            aria-label={`Ask ${assistant.name}`}
            aria-haspopup="true"
            disabled={load.state !== "ready"}
            onClick={(event) => assistant.onMenu(anchorOf(event.currentTarget, "end"))}
          >
            <Icon name="agent" size={15} />
            <span className="reader-ask-name">{assistant.name}</span>
            <Icon name="chevronDown" size={12} />
          </button>
        )}
      </header>
      <div className="reader-scroll">
        <h1 className="reader-subject">{subject}</h1>
        {load.state === "failed" ? (
          <p className="error reader-error" role="alert">{load.message}</p>
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
                  onPalette={onPalette}
                  onSender={onSender}
                  conversation={conversation}
                  onAssistant={onAssistant}
                />
              );
            })}
          </ol>
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
  onPalette,
  onSender,
  conversation,
  onAssistant,
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
  onPalette: Palette;
  onSender?: SenderOpener;
  /** A conversation of several messages: open them all, or fold the older ones. */
  conversation?: { openAll: () => void; foldOlder: () => void };
  onAssistant?: (messageId: number, kind: "summary" | "reply") => void;
}) {
  const own = useDetail(entry.message.id, given === null && expanded);
  const load = given ?? own;
  const [showText, setShowText] = useState(false);
  const dark = useDark();
  // Settings › General: images from the web load in every message, or only when asked.
  const [allowRemote, setAllowRemote] = useState(() => getPrefs().remoteImages);
  const [details, setDetails] = useState(false);
  const [source, setSource] = useState(false);
  // The HTML frame is as tall as the message: its own script says how tall (mail_frame.rs).
  const frame = useRef<HTMLIFrameElement>(null);
  const [frameHeight, setFrameHeight] = useState<number | null>(null);
  useEffect(() => {
    const heard = (event: MessageEvent) => {
      if (!frame.current || event.source !== frame.current.contentWindow) return;
      const height = (event.data as { regardsMailHeight?: unknown } | null)?.regardsMailHeight;
      if (typeof height === "number" && Number.isFinite(height)) {
        setFrameHeight(Math.min(Math.max(Math.ceil(height), 40), MAX_FRAME_HEIGHT));
      }
    };
    window.addEventListener("message", heard);
    return () => window.removeEventListener("message", heard);
  }, []);
  const { message } = entry;
  const ready = load.state === "ready" ? load.detail : null;
  const when = fullDate(message.sent_at ?? message.received_at);
  // This sender's (or its domain's) images load: the person said so once, for all their mail.
  const [fromSender, setFromSender] = useState<boolean | null>(null);
  const trusted = fromSender ?? ready?.images_from_sender === true;
  const [imagesError, setImagesError] = useState<string | null>(null);
  const address = message.from?.address ?? null;
  const domain = address?.includes("@") ? `@${address.split("@")[1].toLowerCase()}` : null;
  const alwaysFrom = (who: string) => {
    setImagesError(null);
    setSenderImages(accountId, who, true).then(
      () => {
        setFromSender(true);
        setAllowRemote(true);
      },
      (failure: unknown) => setImagesError(String(failure)),
    );
  };
  const stopFrom = () => {
    setImagesError(null);
    // Allowed by address, by domain, or both: block every way it was allowed.
    senderImages(accountId)
      .then((allowed) =>
        Promise.all(
          [address?.toLowerCase(), domain]
            .filter((who): who is string => !!who && allowed.includes(who))
            .map((who) => setSenderImages(accountId, who, false)),
        ),
      )
      .then(
        () => {
          setFromSender(false);
          setAllowRemote(getPrefs().remoteImages);
        },
        (failure: unknown) => setImagesError(String(failure)),
      );
  };

  if (!expanded) {
    return (
      <li className="mail-card collapsed" ref={ref}>
        <button type="button" className="mail-line" aria-expanded={false} onClick={onToggle ?? undefined}>
          <Avatar person={message.from} size="sm" />
          <span className={message.seen ? "who" : "who unread"}>{personName(message.from)}</span>
          {otherFolder && entry.mailbox_name && <span className="chip">{entry.mailbox_name}</span>}
          <span className="when">{when}</span>
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
    reminding: (ready?.message.remind_at ?? message.remind_at) !== null,
    reminded: ready?.message.reminded ?? message.reminded,
  });
  // The message's own view options join its actions in the palette.
  const more = () => {
    const extra: MenuEntry[] = [];
    if (ready?.has_html) {
      extra.push({ label: showText ? "Show formatted" : "Show plain text", icon: "text", onSelect: () => setShowText(!showText) });
    }
    if (ready?.has_html && ready.remote_images > 0 && !allowRemote && !trusted) {
      extra.push({ label: "Load remote images", icon: "image", onSelect: () => setAllowRemote(true) });
    }
    if (ready?.has_html && ready.remote_images > 0 && address && !trusted) {
      extra.push({ label: `Always load images from ${address}`, icon: "image", onSelect: () => alwaysFrom(address) });
      if (domain) extra.push({ label: `Always load images from anyone at ${domain.slice(1)}`, icon: "image", onSelect: () => alwaysFrom(domain) });
    }
    if (ready?.has_html && trusted) {
      extra.push({ label: "Stop loading images from this sender", icon: "shield", onSelect: stopFrom });
    }
    if (conversation) {
      extra.push(
        { label: "Open every message", icon: "expand", keys: "Shift+O", onSelect: conversation.openAll },
        { label: "Fold the older messages", icon: "shrink", keys: "o", onSelect: conversation.foldOlder },
      );
    }
    if (ready) {
      extra.push(
        {
          label: "Copy as Markdown",
          icon: "copy",
          onSelect: () => void navigator.clipboard?.writeText(asMarkdown(ready)).catch(() => {}),
        },
        { label: "Print…", icon: "text", onSelect: () => printMessage(ready) },
        { label: "See the original message", icon: "quote", onSelect: () => setSource(true) },
      );
    }
    if (onAssistant) {
      extra.push(
        { label: "Summarise with the assistant…", icon: "bolt", onSelect: () => onAssistant(message.id, "summary") },
        { label: "Draft a reply with the assistant…", icon: "bolt", onSelect: () => onAssistant(message.id, "reply") },
      );
    }
    extra.push({ label: details ? "Hide details" : "Show details", icon: "info", onSelect: () => setDetails(!details) });
    const from = message.from;
    if (onSender && from) {
      extra.push({
        label: "The sender, and why this is here…",
        icon: "people",
        onSelect: () => onSender({ x: window.innerWidth / 2 - 160, y: window.innerHeight / 4 }, message.id, from),
      });
    }
    onPalette(actions, message.subject || "(no subject)", extra);
  };
  const to = ready?.to ?? [];
  const cc = ready?.cc ?? [];

  return (
    <li className="mail-card" ref={ref}>
      <div className="mail-head">
        {onSender && message.from ? (
          <button
            type="button"
            className="mail-avatar"
            title="About the sender"
            aria-label={`About ${personName(message.from)}`}
            onClick={(event) => message.from && onSender(anchorOf(event.currentTarget), message.id, message.from)}
          >
            <Avatar person={message.from} size="sm" />
          </button>
        ) : (
          <Avatar person={message.from} size="sm" />
        )}
        <button
          type="button"
          className="mail-who"
          aria-expanded={onToggle ? true : details}
          onClick={onToggle ?? (() => setDetails(!details))}
          title={message.from?.address ?? undefined}
        >
          <strong>{personName(message.from)}</strong>
          {ready && (to.length > 0 || cc.length > 0) && (
            <span className="mail-to">
              to <span className="people">{recipients([...to, ...cc])}</span>
            </span>
          )}
        </button>
        <time className="when">{when}</time>
        <button type="button" className="icon-btn small" title={`More (${MOD_KEY}+K)`} aria-label="More" aria-haspopup="dialog" onClick={more}>
          <Icon name="more" size={15} />
        </button>
      </div>
      {details && ready && <Details detail={ready} />}
      {source && (
        <SourceDialog messageId={message.id} subject={message.subject || "(no subject)"} onClose={() => setSource(false)} />
      )}
      <div className="mail-body">
        {load.state === "loading" && <p className="muted">Opening…</p>}
        {load.state === "failed" && <p className="error" role="alert">{load.message}</p>}
        {ready && (
          <>
            {ready.has_html && !showText && ready.remote_images > 0 && !allowRemote && !trusted && (
              <div className="notice images-notice">
                <Icon name="shield" size={16} />
                <span>
                  {ready.remote_images} remote {ready.remote_images === 1 ? "image is" : "images are"} blocked. They
                  can tell the sender you opened this message.
                </span>
                <button type="button" className="btn small" onClick={() => setAllowRemote(true)}>
                  Load once
                </button>
                {address && (
                  <button type="button" className="btn small" title={`Images from ${address} load from now on`} onClick={() => alwaysFrom(address)}>
                    Always from this sender
                  </button>
                )}
              </div>
            )}
            {imagesError && <p className="error" role="alert">{imagesError}</p>}
            {ready.has_html && !showText ? (
              // Only our script, which says how tall the message is (its page's CSP allows no
              // other); no same-origin access, no forms, popups or navigation.
              <iframe
                // Loaded again when the sender's images are allowed or blocked, or the theme changes.
                key={`${fromSender}:${dark}`}
                ref={frame}
                className={dark ? "html-body dark" : "html-body"}
                style={frameHeight === null ? undefined : { height: frameHeight }}
                sandbox="allow-scripts"
                referrerPolicy="no-referrer"
                title="Message content"
                src={messageHtmlUrl(message.id, allowRemote || fromSender === true, dark)}
              />
            ) : (
              <TextBody text={ready.text.trim()} onFailed={setImagesError} />
            )}
            <AttachmentTiles messageId={message.id} attachments={ready.attachments} />
          </>
        )}
      </div>
      {ready && (
        <div className="mail-foot">
          {choose(actions, "reply", "forward").map((action) => (
            <button
              key={action.id}
              type="button"
              className="icon-btn small"
              title={actionTitle(action)}
              aria-label={action.label}
              onClick={() => action.run(null)}
            >
              <Icon name={action.icon} size={16} />
            </button>
          ))}
        </div>
      )}
    </li>
  );
}

/** A message's plain text, its web addresses as links that open in the browser. */
function TextBody({ text, onFailed }: { text: string; onFailed: (problem: string) => void }) {
  const parts = useMemo(() => textParts(text), [text]);
  if (!text) return <pre className="text-body">(no text)</pre>;
  return (
    <pre className="text-body">
      {parts.map((part, at) =>
        part.url ? (
          <a
            key={at}
            href={part.url}
            title={part.url}
            rel="noreferrer"
            onClick={(event) => {
              // Never in the app's own window: in the browser.
              event.preventDefault();
              openUrl(part.url).catch((failure: unknown) => onFailed(String(failure)));
            }}
            onAuxClick={(event) => event.preventDefault()}
          >
            {part.text}
          </a>
        ) : (
          part.text
        ),
      )}
    </pre>
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
