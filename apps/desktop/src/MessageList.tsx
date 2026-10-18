import { type MouseEvent, type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { MessageSummary, Person } from "./accounts";
import { type Action, choose } from "./actions";
import { Avatar } from "./Avatar";
import { dateSection, laterDate, listDate, personName } from "./format";
import { Icon, type IconName } from "./icons";
import type { ListItem, MessageFeed } from "./mail";
import { type Anchor, type MenuEntry, type MenuRequest, anchorOf } from "./Menu";
import { MOD_KEY, useShortcuts } from "./shortcuts";

/** "Someday" in the daemon: no time to come back (regards_store::marks::SOMEDAY). */
const SOMEDAY = 32_503_680_000_000;

/** When the list is this close to its end, the next page is fetched. */
const PREFETCH_PX = 800;

/** The top of a list: its name on two lines (with a menu when it has one), and buttons on the right. */
export function ListHeader({
  title,
  subtitle,
  onTitle,
  children,
}: {
  title: string;
  subtitle?: string;
  /** Opens a menu under the title (to choose an account, say). */
  onTitle?: (anchor: Anchor) => void;
  children?: ReactNode;
}) {
  const heading = (
    <>
      <span className="list-name">
        {title}
        {onTitle && <Icon name="chevronDown" size={14} />}
      </span>
      {subtitle && <span className="list-sub">{subtitle}</span>}
    </>
  );
  return (
    <header className="list-head">
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

/** A row that stands for a group of messages (the pinned ones, say): who is in it, and how many. */
export interface Bundle {
  label: string;
  icon: IconName;
  messages: MessageSummary[];
  onOpen: () => void;
}

export function MessageList({
  title,
  subtitle,
  onTitle,
  onCompose,
  canCompose,
  feed,
  mode,
  openId,
  picked,
  onOpen,
  onPick,
  actionsFor,
  onMenu,
  empty,
  bundle,
  onSplit,
}: {
  title: string;
  subtitle?: string;
  onTitle?: (anchor: Anchor) => void;
  onCompose: () => void;
  canCompose: boolean;
  feed: MessageFeed;
  /** One line per message beside nothing ("single"), or two lines beside the reading pane ("split"). */
  mode: "single" | "split";
  openId: number | null;
  picked: ReadonlySet<number>;
  onOpen: (item: ListItem) => void;
  onPick: (ids: Set<number>) => void;
  actionsFor: (items: ListItem[]) => Action[];
  onMenu: (request: MenuRequest) => void;
  empty: { title: string; body: string };
  /** A group row at the top (the inbox's pinned messages). */
  bundle?: Bundle | null;
  /** Show the reading pane beside the list (the handle on the list's edge). */
  onSplit?: () => void;
}) {
  const [searching, setSearching] = useState(false);
  const [query, setQuery] = useState("");
  const [cursor, setCursor] = useState<number | null>(null);
  const hovered = useRef<number | null>(null);
  const scroller = useRef<HTMLDivElement>(null);
  // Where a Shift+click range starts.
  const anchor = useRef<number | null>(null);

  const items = useMemo(() => {
    const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
    if (words.length === 0) return feed.items;
    return feed.items.filter((item) => {
      const text = `${item.subject} ${item.from?.name ?? ""} ${item.from?.address ?? ""}`.toLowerCase();
      return words.every((word) => text.includes(word));
    });
  }, [feed.items, query]);

  // In split view the keyboard follows the open message.
  const focus = mode === "split" ? (openId ?? cursor) : cursor;
  const pickedItems = items.filter((item) => picked.has(item.id));

  const toggle = (item: ListItem) => {
    const next = new Set(picked);
    if (next.has(item.id)) next.delete(item.id);
    else next.add(item.id);
    anchor.current = item.id;
    onPick(next);
  };

  const click = (event: MouseEvent, item: ListItem, index: number) => {
    if (event.shiftKey) {
      const start = items.findIndex((other) => other.id === (anchor.current ?? focus));
      if (start !== -1) {
        const next = new Set(picked);
        const [from, to] = start < index ? [start, index] : [index, start];
        for (const other of items.slice(from, to + 1)) next.add(other.id);
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
    const index = items.findIndex((item) => item.id === focus);
    const target = index === -1 ? (by > 0 ? 0 : items.length - 1) : index + by;
    const item = items[target];
    if (!item) return;
    anchor.current = item.id;
    stepped.current = true;
    setCursor(item.id);
    if (mode === "split") onOpen(item);
  };

  // Shortcuts act on the selection, else the message under the pointer, else the
  // highlighted one (looked up when pressed).
  const current = () => items.find((item) => item.id === (hovered.current ?? focus));
  const sample = items[0] ? actionsFor([items[0]]) : [];
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
    "Mod+a": () => onPick(new Set(items.map((item) => item.id))),
    "Mod+f": () => setSearching(true),
    ...(picked.size > 0 ? { Escape: () => onPick(new Set()) } : {}),
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

  const menuEntries = (actions: Action[], at: Anchor | null): MenuEntry[] => {
    const groups = [
      choose(actions, "reply", "replyAll", "forward"),
      choose(actions, "read", "unread", "flag"),
      choose(actions, "pin", "snooze", "remind", "aside"),
      choose(actions, "archive", "move"),
      choose(actions, "delete"),
    ].filter((group) => group.length > 0);
    return groups.flatMap((group, index) => [
      ...(index > 0 ? (["separator"] as const) : []),
      ...group.map((action) => ({
        label: action.label,
        icon: action.icon,
        hint: action.keys[0],
        danger: action.danger,
        onSelect: () => action.run(at),
      })),
    ]);
  };

  const contextMenu = (event: MouseEvent, item: ListItem) => {
    event.preventDefault();
    const targets = picked.has(item.id) && picked.size > 1 ? items.filter((other) => picked.has(other.id)) : [item];
    const at = { x: event.clientX, y: event.clientY };
    onMenu({ anchor: at, entries: menuEntries(actionsFor(targets), at), label: targets.length > 1 ? `${targets.length} messages` : "Message" });
  };

  // Group by day: Today, Yesterday, this week … by place in the list, so what
  // came back to the inbox (a snooze that ended, a reminder) is under the day it did.
  const sections: { label: string; items: ListItem[] }[] = [];
  for (const item of items) {
    const label = dateSection(item.listed_at);
    const last = sections[sections.length - 1];
    if (last?.label === label) last.items.push(item);
    else sections.push({ label, items: [item] });
  }
  const showBundle = bundle && bundle.messages.length > 0 && !query;
  if (showBundle && sections[0]?.label !== "Today") sections.unshift({ label: "Today", items: [] });

  const header =
    pickedItems.length > 0 ? (
      <SelectionBar
        count={pickedItems.length}
        actions={actionsFor(pickedItems)}
        onClear={() => onPick(new Set())}
        onMore={(at) => onMenu({ anchor: at, entries: menuEntries(actionsFor(pickedItems), at), label: "Selected messages" })}
      />
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
      <ListHeader title={title} subtitle={subtitle} onTitle={onTitle}>
        <button type="button" className="icon-btn" title={`Search (${MOD_KEY}+F)`} aria-label="Search" onClick={() => setSearching(true)}>
          <Icon name="search" />
        </button>
        <button type="button" className="icon-btn" title="New message (c)" aria-label="New message" onClick={onCompose} disabled={!canCompose}>
          <Icon name="compose" />
        </button>
      </ListHeader>
    );

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
        ) : items.length === 0 && !showBundle && !feed.error ? (
          query ? (
            <div className="empty list-empty">
              <h2>Nothing matches “{query}”</h2>
              <p>This searches the messages already in this list, by sender and subject.</p>
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
            {sections.map((section, sectionIndex) => (
              <section key={section.label} className="day" aria-label={section.label}>
                <h3 className="day-head">{section.label}</h3>
                <ul>
                  {sectionIndex === 0 && showBundle && <BundleRow bundle={bundle} />}
                  {section.items.map((item) => {
                    const index = items.indexOf(item);
                    const isPicked = picked.has(item.id);
                    // Beside the reader the row is narrow: fewer buttons, the rest under More.
                    const hover =
                      mode === "single"
                        ? choose(actionsFor([item]), "archive", "delete", "flag", "read", "unread", "pin", "aside", "snooze")
                        : choose(actionsFor([item]), "archive", "delete", "read", "unread", "snooze");
                    const snoozedUntil =
                      item.snoozed_until !== null && item.snoozed_until < SOMEDAY ? item.snoozed_until : null;
                    return (
                      <li
                        key={`${item.mailboxId}:${item.id}`}
                        className={[
                          "row",
                          item.seen ? "" : "unread",
                          mode === "split" && item.id === openId && picked.size === 0 ? "open" : "",
                          item.id === focus ? "cursor" : "",
                          isPicked ? "picked" : "",
                        ].join(" ")}
                        onMouseEnter={() => {
                          hovered.current = item.id;
                        }}
                        onMouseLeave={() => {
                          if (hovered.current === item.id) hovered.current = null;
                        }}
                        onContextMenu={(event) => contextMenu(event, item)}
                      >
                        <button
                          type="button"
                          className="row-check"
                          aria-label={isPicked ? "Unselect" : "Select"}
                          aria-pressed={isPicked}
                          onClick={() => toggle(item)}
                        >
                          <Icon name="check" size={12} />
                        </button>
                        <button type="button" className="row-main" onClick={(event) => click(event, item, index)}>
                          <span className="dot" aria-label={item.seen ? undefined : "Unread"} />
                          <span className="row-avatar">
                            <Avatar person={item.from} size={mode === "single" ? "sm" : "md"} ring />
                          </span>
                          <span className="from">{personName(item.from)}</span>
                          <span className="subject">
                            {item.pinned && <Icon name="pinFilled" size={13} className="pinned" />}
                            {item.subject || "(no subject)"}
                          </span>
                          <span className="marks">
                            {(item.reminded || item.remind_at !== null) && (
                              <span
                                className={item.reminded ? "reminder due" : "reminder"}
                                title={
                                  item.reminded
                                    ? "No reply yet"
                                    : `Reminds you ${laterDate(item.remind_at ?? 0)} if nobody replies`
                                }
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
                        </button>
                        <span className="row-actions">
                          {hover.map((action) => (
                            <button
                              key={action.id}
                              type="button"
                              className="icon-btn small"
                              title={`${action.label} (${action.keys[0]})`}
                              aria-label={action.label}
                              onClick={(event) => action.run(anchorOf(event.currentTarget, "end"))}
                            >
                              <Icon name={action.icon} size={16} />
                            </button>
                          ))}
                          <button
                            type="button"
                            className="icon-btn small"
                            title="More"
                            aria-label="More"
                            onClick={(event) => {
                              const at = anchorOf(event.currentTarget, "end");
                              onMenu({ anchor: at, entries: menuEntries(actionsFor([item]), at), label: "Message" });
                            }}
                          >
                            <Icon name="more" size={16} />
                          </button>
                        </span>
                      </li>
                    );
                  })}
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

/** The group row: an icon, the name and count, then who is in it. */
function BundleRow({ bundle }: { bundle: Bundle }) {
  const senders: { person: Person | null; count: number }[] = [];
  for (const message of bundle.messages) {
    const name = personName(message.from);
    const known = senders.find((sender) => personName(sender.person) === name);
    if (known) known.count++;
    else senders.push({ person: message.from, count: 1 });
  }
  const unread = bundle.messages.some((message) => !message.seen);
  return (
    <li className={unread ? "row bundle unread" : "row bundle"}>
      <button type="button" className="row-main" onClick={bundle.onOpen}>
        <span className="dot" />
        <span className="row-avatar">
          <span className="bundle-icon">
            <Icon name={bundle.icon} size={15} />
          </span>
        </span>
        <span className="from">
          {bundle.label} <span className="bundle-count">{bundle.messages.length}</span>
        </span>
        <span className="subject chips">
          {senders.slice(0, 6).map((sender) => (
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
  onMore: (anchor: Anchor) => void;
}) {
  return (
    <header className="list-head selection" role="toolbar" aria-label="Selected messages">
      <button type="button" className="icon-btn" title="Clear the selection (Esc)" aria-label="Clear the selection" onClick={onClear}>
        <Icon name="close" />
      </button>
      <span className="selection-count">{count} selected</span>
      <div className="list-tools">
        {choose(actions, "aside", "flag", "archive", "read", "unread", "pin", "move", "delete", "snooze", "remind").map((action) => (
          <button
            key={action.id}
            type="button"
            className={action.danger ? "icon-btn danger" : "icon-btn"}
            title={`${action.label} (${action.keys[0]})`}
            aria-label={action.label}
            onClick={(event) => action.run(anchorOf(event.currentTarget, "end"))}
          >
            <Icon name={action.icon} />
          </button>
        ))}
        <button type="button" className="icon-btn" title="More" aria-label="More" onClick={(event) => onMore(anchorOf(event.currentTarget, "end"))}>
          <Icon name="more" />
        </button>
      </div>
    </header>
  );
}

function SearchBar({ query, onQuery, onClose }: { query: string; onQuery: (query: string) => void; onClose: () => void }) {
  return (
    <header className="list-head searching">
      <label className="search-field">
        <Icon name="search" size={17} />
        <input
          autoFocus
          value={query}
          placeholder="Search this list by sender or subject"
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
          <Icon name="close" size={13} />
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
