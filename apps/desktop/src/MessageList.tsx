import { type DragEvent, type MouseEvent, type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Person } from "./accounts";
import { type Action, choose } from "./actions";
import { Avatar } from "./Avatar";
import { dateSection, laterDate, listDate, peopleShort, personName } from "./format";
import { Icon, type IconName } from "./icons";
import type { ListItem, MessageFeed } from "./mail";
import { type Anchor, anchorOf } from "./Menu";
import { MOD_KEY, useShortcuts } from "./shortcuts";
import { DRAG_TYPE, type DragPayload } from "./Sidebar";

/** "Someday" in the daemon: no time to come back (regards_store::marks::SOMEDAY). */
const SOMEDAY = 32_503_680_000_000;

/** When the list is this close to its end, the next page is fetched. */
const PREFETCH_PX = 800;

/** The top of a list: its name on two lines (with a menu when it has one), and buttons on the right. */
export function ListHeader({
  title,
  subtitle,
  onTitle,
  onBack,
  children,
}: {
  title: string;
  subtitle?: string;
  /** Opens a menu or chooser under the title. */
  onTitle?: (anchor: Anchor) => void;
  /** A group's own list: back to the inbox. */
  onBack?: () => void;
  children?: ReactNode;
}) {
  const heading = (
    <>
      <span className="list-name">
        {title}
        {onTitle && <Icon name="chevronDown" size={12} />}
      </span>
      {subtitle && <span className="list-sub">{subtitle}</span>}
    </>
  );
  return (
    <header className="list-head" data-tauri-drag-region>
      {onBack && (
        <button type="button" className="icon-btn list-back" title="Back to the inbox (Esc)" aria-label="Back to the inbox" onClick={onBack}>
          <Icon name="back" size={17} />
        </button>
      )}
      {onTitle ? (
        <button type="button" className="list-title" onClick={(event) => onTitle(anchorOf(event.currentTarget))}>
          {heading}
        </button>
      ) : (
        <h1 className="list-title">{heading}</h1>
      )}
      <div className="list-tools">{children}</div>
    </header>
  );
}

/** A row standing for a group of messages: who is in it and how many. Opens the group. */
export interface Bundle {
  key: string;
  label: string;
  icon: IconName;
  /** Total messages in the group, and whether any is unread. */
  count: number;
  unread: boolean;
  /** Its newest messages, for the senders' chips. */
  messages: { from: Person | null }[];
  onOpen: () => void;
}

/** The Smart Inbox around the People list: the priority card, new senders, and group rows. */
export interface SmartParts {
  priority: { items: ListItem[]; total: number; onAll: () => void } | null;
  newSenders: { items: ListItem[]; total: number; held: boolean; onAll: () => void } | null;
  bundles: Bundle[];
}

/** Accepting or blocking a new sender, from their row. */
export interface Screener {
  accept: (item: ListItem) => void;
  block: (item: ListItem) => void;
}

export function MessageList({
  title,
  subtitle,
  onTitle,
  onBack,
  onCompose,
  canCompose,
  feed,
  mode,
  openId,
  picked,
  onOpen,
  onPick,
  actionsFor,
  onPalette,
  empty,
  onSplit,
  own,
  smart,
  screener,
  screenAll = false,
}: {
  title: string;
  subtitle?: string;
  onTitle?: (anchor: Anchor) => void;
  onBack?: () => void;
  onCompose: () => void;
  canCompose: boolean;
  feed: MessageFeed;
  /** One line per message beside nothing ("single"), or three beside the reading pane ("split"). */
  mode: "single" | "split";
  openId: number | null;
  picked: ReadonlySet<number>;
  onOpen: (item: ListItem) => void;
  onPick: (ids: Set<number>) => void;
  actionsFor: (items: ListItem[]) => Action[];
  /** The command palette, for these actions (right-click and the ⋯ buttons); `about` is the one message. */
  onPalette: (actions: Action[], title: string, about?: ListItem) => void;
  empty: { title: string; body: string };
  /** Show the reading pane beside the list (the handle on the list's edge). */
  onSplit?: () => void;
  /** The person's own addresses, lowercase: their mail reads "to Lena". */
  own: ReadonlySet<string>;
  /** The inbox as the Smart Inbox: cards and groups around the People list. */
  smart?: SmartParts | null;
  screener?: Screener;
  /** Every row here is a new sender (the screener's own list): Accept and Block on each. */
  screenAll?: boolean;
}) {
  const [searching, setSearching] = useState(false);
  const [query, setQuery] = useState("");
  const [cursor, setCursor] = useState<number | null>(null);
  const hovered = useRef<number | null>(null);
  const scroller = useRef<HTMLDivElement>(null);
  // Where a Shift+click range starts.
  const anchor = useRef<number | null>(null);

  // The cards' messages come first, then the list, for the keyboard and for selecting.
  const showSmart = Boolean(smart) && !query;
  const cardItems = useMemo(
    () => (smart ? [...(smart.newSenders?.held ? [] : (smart.newSenders?.items ?? [])), ...(smart.priority?.items ?? [])] : []),
    [smart],
  );
  const items = useMemo(() => {
    const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
    if (words.length === 0) return feed.items;
    return feed.items.filter((item) => {
      const text = `${item.subject} ${item.from?.name ?? ""} ${item.from?.address ?? ""} ${item.preview ?? ""}`.toLowerCase();
      return words.every((word) => text.includes(word));
    });
  }, [feed.items, query]);
  const all = useMemo(() => (showSmart ? [...cardItems, ...items] : items), [showSmart, cardItems, items]);

  // In split view the keyboard follows the open message.
  const focus = mode === "split" ? (openId ?? cursor) : cursor;
  const pickedItems = all.filter((item) => picked.has(item.id));

  const toggle = (item: ListItem) => {
    const next = new Set(picked);
    if (next.has(item.id)) next.delete(item.id);
    else next.add(item.id);
    anchor.current = item.id;
    onPick(next);
  };

  const click = (event: MouseEvent, item: ListItem) => {
    const index = all.indexOf(item);
    if (event.shiftKey) {
      const start = all.findIndex((other) => other.id === (anchor.current ?? focus));
      if (start !== -1) {
        const next = new Set(picked);
        const [from, to] = start < index ? [start, index] : [index, start];
        for (const other of all.slice(from, to + 1)) next.add(other.id);
        onPick(next);
        return;
      }
    }
    if (event.metaKey || event.ctrlKey || picked.size > 0) {
      toggle(item);
      return;
    }
    anchor.current = item.id;
    setCursor(item.id);
    onOpen(item);
  };

  // Down and up: in split view they open the next message; otherwise they move the highlight.
  // The highlighted row also takes the keyboard focus, so Enter opens it.
  const stepped = useRef(false);
  const step = (by: number) => {
    const index = all.findIndex((item) => item.id === focus);
    const target = index === -1 ? (by > 0 ? 0 : all.length - 1) : index + by;
    const item = all[target];
    if (!item) return;
    anchor.current = item.id;
    stepped.current = true;
    setCursor(item.id);
    if (mode === "split") onOpen(item);
  };

  // What an action applies to: the selection when the row is in it, else the row.
  const targetsFor = (item: ListItem) =>
    picked.has(item.id) && picked.size > 1 ? all.filter((other) => picked.has(other.id)) : [item];
  const palette = (targets: ListItem[]) =>
    onPalette(
      actionsFor(targets),
      targets.length > 1 ? `${targets.length} messages` : targets[0].subject || "(no subject)",
      targets.length === 1 ? targets[0] : undefined,
    );

  // Shortcuts act on the selection, else the message under the pointer, else the
  // highlighted one (looked up when pressed).
  const current = () => all.find((item) => item.id === (hovered.current ?? focus));
  const sample = all[0] ? actionsFor([all[0]]) : [];
  const actionShortcuts: Record<string, () => void> = {};
  for (const action of sample) {
    for (const key of action.keys) {
      actionShortcuts[key] = () => {
        const item = current();
        const targets = pickedItems.length > 0 ? pickedItems : item ? [item] : [];
        if (targets.length === 0) return;
        const chosen = actionsFor(targets).find((other) => other.keys.includes(key));
        chosen?.run(null);
      };
    }
  }
  useShortcuts({
    ...actionShortcuts,
    j: () => step(1),
    ArrowDown: () => step(1),
    k: () => step(-1),
    ArrowUp: () => step(-1),
    x: () => {
      const item = current();
      if (item) toggle(item);
    },
    "Mod+a": () => onPick(new Set(all.map((item) => item.id))),
    "Mod+f": () => setSearching(true),
    ...(picked.size > 0 ? { Escape: () => onPick(new Set()) } : onBack ? { Escape: onBack } : {}),
  });

  useEffect(() => {
    const row = scroller.current?.querySelector<HTMLElement>(".row.cursor .row-main, .row.open .row-main");
    row?.scrollIntoView({ block: "nearest" });
    if (stepped.current) row?.focus({ preventScroll: true });
    stepped.current = false;
  }, [focus]);

  // Fetch more while the end is near: on scroll, and when a page did not fill the list.
  const { more, loadMore } = feed;
  const nearEnd = useCallback(() => {
    const element = scroller.current;
    if (!element || !more || query) return;
    if (element.scrollTop + element.clientHeight > element.scrollHeight - PREFETCH_PX) loadMore();
  }, [more, loadMore, query]);
  useEffect(nearEnd, [items.length, nearEnd]);

  // Rows drag onto the sidebar's folders.
  const dragStart = (event: DragEvent, item: ListItem) => {
    const targets = targetsFor(item);
    const payload: DragPayload = {
      ids: targets.map((target) => target.id),
      accounts: targets.map((target) => target.accountId),
      mailboxes: targets.map((target) => target.mailboxId),
    };
    event.dataTransfer.setData(DRAG_TYPE, JSON.stringify(payload));
    event.dataTransfer.effectAllowed = "move";
  };

  // Group by day: Today, Yesterday, This week … by place in the list, so what
  // came back to the inbox (a snooze that ended, a reminder) is under the day it did.
  const sections: { label: string; items: ListItem[] }[] = [];
  for (const item of items) {
    const label = dateSection(item.listed_at);
    const last = sections[sections.length - 1];
    if (last?.label === label) last.items.push(item);
    else sections.push({ label, items: [item] });
  }
  // Groups sit in the first section, as if they had come today.
  const bundles = showSmart ? (smart?.bundles ?? []).filter((bundle) => bundle.count > 0) : [];
  if (bundles.length > 0 && sections[0]?.label !== "Today") sections.unshift({ label: "Today", items: [] });

  const row = (item: ListItem, screen = false) => {
    const isPicked = picked.has(item.id);
    const actions = actionsFor([item]);
    const snoozedUntil = item.snoozed_until !== null && item.snoozed_until < SOMEDAY ? item.snoozed_until : null;
    const button = (action: Action, size: number, className: string) => (
      <button
        key={action.id}
        type="button"
        className={className}
        title={`${action.label} (${action.keys[0]})`}
        aria-label={action.label}
        onClick={(event) => action.run(anchorOf(event.currentTarget, "end"))}
      >
        <Icon name={action.icon} size={size} />
      </button>
    );
    return (
      <li
        key={`${item.mailboxId}:${item.id}`}
        className={[
          "row",
          item.seen ? "" : "unread",
          item.id === openId && picked.size === 0 ? "open" : "",
          item.id === focus ? "cursor" : "",
          isPicked ? "picked" : "",
          screen ? "screening" : "",
        ].join(" ")}
        onMouseEnter={() => {
          hovered.current = item.id;
        }}
        onMouseLeave={() => {
          if (hovered.current === item.id) hovered.current = null;
        }}
        onContextMenu={(event) => {
          event.preventDefault();
          palette(targetsFor(item));
        }}
      >
        {mode === "split" ? (
          // Beside the reader: Done, read/unread and delete, stacked in the margin.
          <span className="row-gutter">
            {isPicked ? (
              <button type="button" className="gutter-check" aria-label="Unselect" aria-pressed onClick={() => toggle(item)}>
                <Icon name="check" size={11} />
              </button>
            ) : (
              choose(actions, "archive", "read", "unread", "delete").map((action) =>
                // Done (archive) shows a check here, as in the reader's Back | Done.
                button(action.id === "archive" ? { ...action, icon: "check" } : action, 13, "gutter-btn"),
              )
            )}
          </span>
        ) : (
          <button
            type="button"
            className="row-check"
            aria-label={isPicked ? "Unselect" : "Select"}
            aria-pressed={isPicked}
            onClick={() => toggle(item)}
          >
            <Icon name="check" size={12} />
          </button>
        )}
        <button
          type="button"
          className="row-main"
          draggable
          onDragStart={(event) => dragStart(event, item)}
          onClick={(event) => click(event, item)}
        >
          <span className="dot" aria-label={item.seen ? undefined : "Unread"} />
          <span className="row-avatar">
            <Avatar person={correspondent(item, own)} size="sm" />
          </span>
          <span className="from">{who(item, own)}</span>
          <span className="subject">
            {item.pinned && <Icon name="pinFilled" size={13} className="pinned" />}
            {item.subject || "(no subject)"}
          </span>
          <span className="marks">
            {item.thread_size !== undefined && item.thread_size > 1 && (
              <span className="thread-count" title={`${item.thread_size} messages`}>
                {item.thread_size}
              </span>
            )}
            {item.has_attachments && <Icon name="paperclip" size={14} className="clip" />}
            {(item.reminded || item.remind_at !== null) && (
              <span
                className={item.reminded ? "reminder due" : "reminder"}
                title={item.reminded ? "No reply yet" : `Reminds you ${laterDate(item.remind_at ?? 0)} if nobody replies`}
              >
                <Icon name="bell" size={14} />
              </span>
            )}
            {item.answered && <Icon name="reply" size={14} className="answered" />}
            {item.flagged && <Icon name="flagFilled" size={14} className="flagged" />}
          </span>
          {snoozedUntil !== null ? (
            <span className="date snoozed" title="Comes back then">
              <Icon name="clock" size={13} />
              {laterDate(snoozedUntil)}
            </span>
          ) : (
            <span className="date">{listDate(item.received_at)}</span>
          )}
          {mode === "split" && item.preview && !screen && <span className="preview">{item.preview}</span>}
        </button>
        {screen && screener && (
          // A new sender: let them in (People), or block them (their mail goes to Junk).
          <span className="screen-actions">
            <button type="button" className="screen-btn accept" title="Accept: their mail goes to People" onClick={() => screener.accept(item)}>
              <Icon name="thumbUp" size={14} />
              Accept
            </button>
            <button type="button" className="screen-btn block" title="Block: their mail goes to Junk" onClick={() => screener.block(item)}>
              <Icon name="thumbDown" size={14} />
              Block
            </button>
          </span>
        )}
        {mode === "single" && !screen && (
          // Alone, the row's actions take the place of the date.
          <span className="row-actions">
            {choose(actions, "delete", "flag", "read", "unread", "pin", "aside", "snooze").map((action) =>
              button(action, 16, "icon-btn small"),
            )}
            <button
              type="button"
              className="icon-btn small"
              title={`More (${MOD_KEY}+K)`}
              aria-label="More"
              onClick={() => palette(targetsFor(item))}
            >
              <Icon name="more" size={16} />
            </button>
          </span>
        )}
      </li>
    );
  };

  const header =
    pickedItems.length > 0 ? (
      <SelectionBar count={pickedItems.length} actions={actionsFor(pickedItems)} onClear={() => onPick(new Set())} onMore={() => palette(pickedItems)} />
    ) : searching ? (
      <SearchBar
        query={query}
        onQuery={setQuery}
        onClose={() => {
          setSearching(false);
          setQuery("");
        }}
      />
    ) : (
      <ListHeader title={title} subtitle={subtitle} onTitle={onTitle} onBack={onBack}>
        <button type="button" className="icon-btn" title={`Search (${MOD_KEY}+F)`} aria-label="Search" onClick={() => setSearching(true)}>
          <Icon name="search" size={17} />
        </button>
        <button type="button" className="icon-btn" title="New message (c)" aria-label="New message" onClick={onCompose} disabled={!canCompose}>
          <Icon name="compose" size={17} />
        </button>
      </ListHeader>
    );

  const priority = showSmart ? smart?.priority : null;
  const newSenders = showSmart ? smart?.newSenders : null;
  const nothing =
    items.length === 0 && bundles.length === 0 && !(priority && priority.items.length > 0) && !(newSenders && newSenders.total > 0);

  return (
    <section className={`list-pane ${mode}`} aria-label={title}>
      {header}
      <div className="list-scroll" ref={scroller} onScroll={nearEnd}>
        {feed.error && (
          <p className="list-error" role="alert">
            <Icon name="alert" size={16} /> {feed.error}
          </p>
        )}
        {feed.loading ? (
          <Skeleton mode={mode} />
        ) : nothing && !feed.error ? (
          query ? (
            <div className="empty list-empty">
              <h2>Nothing matches “{query}”</h2>
              <p>This searches the messages already in this list, by sender, subject and preview.</p>
            </div>
          ) : (
            <div className="empty list-empty">
              <span className="zero-mark" aria-hidden="true">
                <Icon name="check" size={34} />
              </span>
              <h2>{empty.title}</h2>
              <p>{empty.body}</p>
            </div>
          )
        ) : (
          <div className={`rows ${mode === "single" ? "wide" : "tall"}${picked.size > 0 ? " picking" : ""}`}>
            {newSenders && newSenders.total > 0 && newSenders.held && (
              // Held before the inbox: one line that leads to them.
              <button type="button" className="screen-banner" onClick={newSenders.onAll}>
                <span className="smart-icon new">
                  <Icon name="people" size={15} />
                </span>
                <strong>New senders</strong>
                <span className="muted">{newSenders.total} waiting for you to accept or block</span>
                <Icon name="chevronRight" size={16} />
              </button>
            )}
            {newSenders && newSenders.total > 0 && !newSenders.held && (
              <section className="smart-card new" aria-label="New senders">
                <h3 className="smart-card-head">
                  <Icon name="people" size={15} />
                  New senders
                  <span className="smart-card-count">{newSenders.total}</span>
                </h3>
                <ul>{newSenders.items.map((item) => row(item, true))}</ul>
                {newSenders.total > newSenders.items.length && (
                  <CardFoot items={newSenders.items} total={newSenders.total} onClick={newSenders.onAll} />
                )}
              </section>
            )}
            {priority && priority.items.length > 0 && (
              <section className="smart-card priority" aria-label="Priority">
                <h3 className="smart-card-head">
                  <Icon name="boltFilled" size={15} />
                  Priority
                </h3>
                <ul>{priority.items.map((item) => row(item))}</ul>
                {priority.total > priority.items.length && (
                  <CardFoot items={priority.items} total={priority.total} onClick={priority.onAll} />
                )}
              </section>
            )}
            {sections.map((section, sectionIndex) => (
              <section key={section.label} className="day" aria-label={section.label}>
                <h3 className="day-head">{section.label}</h3>
                <ul>
                  {sectionIndex === 0 && bundles.map((bundle) => <BundleRow key={bundle.key} bundle={bundle} />)}
                  {section.items.map((item) => row(item, screenAll))}
                </ul>
              </section>
            ))}
            {feed.more && !query && (
              <div className="row-more" aria-hidden="true">
                <span className="spinner" />
              </div>
            )}
          </div>
        )}
      </div>
      {onSplit && (
        <button type="button" className="split-handle" title="Show messages beside the list" aria-label="Split view" onClick={onSplit}>
          <Icon name="chevronLeft" size={14} />
        </button>
      )}
    </section>
  );
}

/** A card's footer: a few faces, and how many more there are. */
function CardFoot({ items, total, onClick }: { items: ListItem[]; total: number; onClick: () => void }) {
  return (
    <button type="button" className="smart-card-foot" onClick={onClick}>
      <span className="faces">
        {items.slice(0, 3).map((item) => (
          <Avatar key={item.id} person={item.from} size="xs" />
        ))}
      </span>
      View all ({total})
    </button>
  );
}

/** A group's row: its icon, name and count, then who is in it. */
function BundleRow({ bundle }: { bundle: Bundle }) {
  const senders: { person: Person | null; count: number }[] = [];
  for (const message of bundle.messages) {
    const name = personName(message.from);
    const known = senders.find((sender) => personName(sender.person) === name);
    if (known) known.count++;
    else senders.push({ person: message.from, count: 1 });
  }
  return (
    <li className={bundle.unread ? "row bundle unread" : "row bundle"}>
      <button type="button" className="row-main" onClick={bundle.onOpen}>
        <span className="dot" />
        <span className="row-avatar">
          <span className={`smart-icon ${bundle.key}`}>
            <Icon name={bundle.icon} size={14} />
          </span>
        </span>
        <span className="from">
          {bundle.label} <span className="bundle-count">{bundle.count}</span>
        </span>
        <span className="subject chips">
          {senders.slice(0, 5).map((sender) => (
            <span key={personName(sender.person)} className="sender-chip">
              <Avatar person={sender.person} size="xs" />
              {personName(sender.person)}
              {sender.count > 1 && <span className="bundle-count">{sender.count}</span>}
            </span>
          ))}
        </span>
        <span className="marks" />
        <span className="date">
          <Icon name="chevronRight" size={16} />
        </span>
      </button>
    </li>
  );
}

/** In place of the header while messages are selected: how many, and what to do with them. */
function SelectionBar({
  count,
  actions,
  onClear,
  onMore,
}: {
  count: number;
  actions: Action[];
  onClear: () => void;
  onMore: () => void;
}) {
  return (
    <header className="list-head selection" role="toolbar" aria-label="Selected messages">
      <button type="button" className="icon-btn" title="Clear the selection (Esc)" aria-label="Clear the selection" onClick={onClear}>
        <Icon name="close" size={16} />
      </button>
      <span className="selection-count">
        <Icon name="mail" size={16} />
        {count} selected
      </span>
      <div className="list-tools">
        {choose(actions, "aside", "flag", "archive", "read", "unread", "pin", "delete", "snooze").map((action) => (
          <button
            key={action.id}
            type="button"
            className={action.danger ? "icon-btn danger" : "icon-btn"}
            title={`${action.label} (${action.keys[0]})`}
            aria-label={action.label}
            onClick={(event) => action.run(anchorOf(event.currentTarget, "end"))}
          >
            <Icon name={action.icon} size={17} />
          </button>
        ))}
        <button type="button" className="icon-btn" title={`More (${MOD_KEY}+K)`} aria-label="More" onClick={onMore}>
          <Icon name="more" size={17} />
        </button>
      </div>
    </header>
  );
}

function SearchBar({ query, onQuery, onClose }: { query: string; onQuery: (query: string) => void; onClose: () => void }) {
  return (
    <header className="list-head searching" data-tauri-drag-region>
      <label className="search-field">
        <Icon name="search" size={16} />
        <input
          autoFocus
          value={query}
          placeholder="Search this list"
          aria-label="Search this list"
          onChange={(event) => onQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.preventDefault();
              onClose();
            }
          }}
        />
        <button type="button" className="search-clear" title="Close the search (Esc)" aria-label="Close the search" onClick={onClose}>
          <Icon name="close" size={12} />
        </button>
      </label>
    </header>
  );
}

function Skeleton({ mode }: { mode: "single" | "split" }) {
  return (
    <div className={`rows ${mode === "single" ? "wide" : "tall"}`} aria-label="Loading messages" aria-busy="true">
      <ul>
        {Array.from({ length: 10 }, (_, index) => (
          <li key={index} className="row skeleton">
            <span className="row-main">
              <span className="dot" />
              <span className="row-avatar">
                <span className="bone avatar-bone" />
              </span>
              <span className="from">
                <span className="bone line-a" />
              </span>
              <span className="subject">
                <span className="bone line-b" />
              </span>
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}

/** Mail the person sent: from the Sent folder, or from one of their addresses. */
function mine(item: ListItem, own: ReadonlySet<string>): boolean {
  const from = item.from?.address?.toLowerCase();
  return item.role === "sent" || (from !== undefined && own.has(from));
}

/** Who a row is about: the sender, or for mail one sent, its first recipient. */
function correspondent(item: ListItem, own: ReadonlySet<string>) {
  return mine(item, own) && item.to.length > 0 ? item.to[0] : item.from;
}

/** "Lena Okafor", or for mail one sent, "to Lena Okafor, Bob" (summaries carry the first three). */
function who(item: ListItem, own: ReadonlySet<string>): string {
  if (!mine(item, own) || item.to.length === 0) return personName(item.from);
  const named = peopleShort(item.to.slice(0, 2));
  return `to ${named}${item.to.length > 2 ? " and others" : ""}`;
}
