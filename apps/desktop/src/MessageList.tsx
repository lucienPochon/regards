import {
  type DragEvent,
  type MouseEvent,
  type ReactNode,
  type RefObject,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type { Person } from "./accounts";
import { type Action, choose } from "./actions";
import { Avatar } from "./Avatar";
import { dateSection, laterDate, listDate, peopleShort, personName } from "./format";
import { Icon, type IconName } from "./icons";
import type { ListItem, MessageFeed } from "./mail";
import { type Anchor, anchorOf } from "./Menu";
import { accountColour, usePrefs } from "./prefs";
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
        <button type="button" className="list-title" aria-haspopup="true" onClick={(event) => onTitle(anchorOf(event.currentTarget))}>
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
  /** When its newest message came: where the group sits when groups follow the dates. */
  newest?: number;
  onOpen: () => void;
}

/** Searching all mail (Ctrl/⌘+F), in place of the header. */
export interface SearchControls {
  /** What is typed; null when not searching. */
  query: string | null;
  onQuery: (query: string | null) => void;
  /** With several accounts: all of them (null), or one. */
  scope: { accounts: { id: string; label: string }[]; accountId: string | null; onScope: (accountId: string | null) => void } | null;
  recents: string[];
  onRemember: (query: string) => void;
  onForget: (query: string) => void;
  /** The best few matches, shown on their own when the results run long. */
  top: ListItem[];
  /** Changes not indexed yet: recent mail may be missing. */
  indexing: number;
  onRetry: () => void;
  /** When nothing was found: a mended search that finds something, offered, never run unasked. */
  suggestion: string | null;
  /** Asking the servers, for words in mail not opened here. */
  server: ServerSearch;
}

export interface ServerSearch {
  state: "idle" | "loading" | "done" | "failed";
  /** Matches the search here had not found. */
  added: number;
  /** Matches not on this device yet. */
  notHere: number;
  /** Addresses of the accounts whose server could not be reached. */
  unreachable: string[];
  /** The servers have older matches: ask again for them. */
  more: boolean;
  error: string | null;
  /** More than one account is searched: "the servers". */
  many: boolean;
  /** The first page, the next one, or again after a failure. */
  onSearch: () => void;
}

/** Results beyond which the best matches get a block of their own. */
const TOP_AFTER = 6;

/** The Smart Inbox around the People list: the priority card, new senders, and group rows. */
export interface SmartParts {
  priority: { items: ListItem[]; total: number; onAll: () => void } | null;
  newSenders: { items: ListItem[]; total: number; held: boolean; onAll: () => void } | null;
  bundles: Bundle[];
}

/**
 * One card of the card view: a kind of unread mail (or pinned mail), its
 * newest few, and the way to all of it.
 */
export interface InboxCard {
  key: string;
  label: string;
  icon: IconName;
  /** Priority keeps its warm card; new senders their own. */
  tone: "priority" | "new" | "plain";
  items: ListItem[];
  /** How many there are in all. */
  total: number;
  onAll: () => void;
  /** The round check: mark the card's mail read. */
  onRead?: () => void;
  /** New senders: Accept and Block on each row. */
  screen?: boolean;
}

/** Accepting or blocking a new sender, from their row. */
export interface Screener {
  accept: (item: ListItem) => void;
  block: (item: ListItem) => void;
}

/** A row's marks in words, for screen readers: the dot, pin, flag and the rest are pictures. */
function rowState(item: ListItem, snoozedUntil: number | null): string {
  const marks = [
    item.seen ? null : "Unread",
    item.pinned ? "pinned" : null,
    item.flagged ? "flagged" : null,
    item.has_attachments ? "attachment" : null,
    item.answered ? "answered" : null,
    item.reminded ? "no reply yet" : item.remind_at !== null ? "reminder set" : null,
    item.thread_size !== undefined && item.thread_size > 1 ? `${item.thread_size} messages` : null,
    snoozedUntil !== null ? `snoozed until ${laterDate(snoozedUntil)}` : null,
  ].filter(Boolean);
  return marks.length > 0 ? `${marks.join(", ")}: ` : "";
}

export function MessageList({
  title,
  subtitle,
  onTitle,
  onBack,
  onCompose,
  canCompose,
  onMarkAllRead,
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
  cards,
  search,
  screener,
  screenAll = false,
}: {
  title: string;
  subtitle?: string;
  onTitle?: (anchor: Anchor) => void;
  onBack?: () => void;
  onCompose: () => void;
  canCompose: boolean;
  /** Every unread message of this folder or section, read (with Undo); absent where it means nothing. */
  onMarkAllRead?: () => void;
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
  /** The inbox by category: unread mail in a card per category, then the list (`feed`) as the card of mail seen. */
  cards?: InboxCard[] | null;
  search: SearchControls;
  screener?: Screener;
  /** Every row here is a new sender (the screener's own list): Accept and Block on each. */
  screenAll?: boolean;
}) {
  const prefs = usePrefs();
  const searching = search.query !== null;
  const query = search.query?.trim() ?? "";
  const field = useRef<HTMLInputElement>(null);
  const [cursor, setCursor] = useState<number | null>(null);
  const hovered = useRef<number | null>(null);
  const scroller = useRef<HTMLDivElement>(null);
  // Where a Shift+click range starts.
  const anchor = useRef<number | null>(null);

  // The cards' messages come first, then the list, for the keyboard and for selecting.
  const showSmart = Boolean(smart) && !searching;
  const showCards = Boolean(cards) && !searching;
  const unreadCards = useMemo(() => (cards ?? []).filter((card) => card.items.length > 0), [cards]);
  const cardItems = useMemo(
    () =>
      cards
        ? unreadCards.flatMap((card) => card.items)
        : smart
          ? [...(smart.newSenders?.held ? [] : (smart.newSenders?.items ?? [])), ...(smart.priority?.items ?? [])]
          : [],
    [cards, unreadCards, smart],
  );
  const items = feed.items;
  const all = useMemo(
    () => (showSmart || showCards ? [...cardItems, ...items] : items),
    [showSmart, showCards, cardItems, items],
  );

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
    "Mod+f": () => {
      search.onQuery(search.query ?? "");
      field.current?.focus();
    },
    ...(picked.size > 0
      ? { Escape: () => onPick(new Set()) }
      : searching
        ? { Escape: () => search.onQuery(null) }
        : onBack
          ? { Escape: onBack }
          : {}),
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
    if (!element || !more) return;
    if (element.scrollTop + element.clientHeight > element.scrollHeight - PREFETCH_PX) loadMore();
  }, [more, loadMore]);
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
  // Groups sit with today (or the first day), or, as the person chose, at their newest message's day.
  const bundles = showSmart ? (smart?.bundles ?? []).filter((bundle) => bundle.count > 0) : [];
  const bundlesIn = new Map<string, Bundle[]>();
  for (const bundle of bundles) {
    let label = "Today";
    if (prefs.groupsPosition === "chronological" && bundle.newest !== undefined) {
      label = dateSection(bundle.newest);
      if (!sections.some((section) => section.label === label)) {
        // A day of its own, in date order among the others.
        const at = sections.findIndex((section) => (section.items[0]?.listed_at ?? Number.MAX_SAFE_INTEGER) < (bundle.newest ?? 0));
        sections.splice(at === -1 ? sections.length : at, 0, { label, items: [] });
      }
    } else if (sections[0]?.label !== "Today") {
      if (!sections.some((section) => section.label === "Today")) sections.unshift({ label: "Today", items: [] });
    }
    bundlesIn.set(label, [...(bundlesIn.get(label) ?? []), bundle]);
  }

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
          <span className="sr-only">{rowState(item, snoozedUntil)}</span>
          <span className="dot" aria-hidden="true" />
          <span className="row-avatar">
            <Avatar person={correspondent(item, own)} size="sm" colour={accountColour(prefs, item.accountId)} />
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
        field={field}
        query={search.query ?? ""}
        onQuery={search.onQuery}
        scope={search.scope}
        onEnter={() => {
          if (!query) return;
          search.onRemember(query);
          step(1);
        }}
        onDown={() => step(1)}
        onClose={() => search.onQuery(null)}
      />
    ) : (
      <ListHeader title={title} subtitle={subtitle} onTitle={onTitle} onBack={onBack}>
        <button type="button" className="icon-btn" title={`Search (${MOD_KEY}+F)`} aria-label="Search" onClick={() => search.onQuery("")}>
          <Icon name="search" size={17} />
        </button>
        {onMarkAllRead && (
          <button type="button" className="icon-btn" title="Mark everything here read" aria-label="Mark everything here read" onClick={onMarkAllRead}>
            <Icon name="mailOpen" size={17} />
          </button>
        )}
        <button type="button" className="icon-btn" title="New message (c)" aria-label="New message" onClick={onCompose} disabled={!canCompose}>
          <Icon name="compose" size={17} />
        </button>
      </ListHeader>
    );

  const priority = showSmart ? smart?.priority : null;
  const newSenders = showSmart ? smart?.newSenders : null;
  const nothing =
    items.length === 0 &&
    bundles.length === 0 &&
    !(priority && priority.items.length > 0) &&
    !(newSenders && newSenders.total > 0) &&
    !(showCards && unreadCards.length > 0);

  return (
    <section className={`list-pane ${mode}`} aria-label={title}>
      {header}
      <div className="list-scroll" ref={scroller} onScroll={nearEnd}>
        {feed.error && (
          <p className="list-error" role="alert">
            <Icon name="alert" size={16} /> {feed.error}
          </p>
        )}
        {searching && !query ? (
          <SearchTips
            recents={search.recents}
            onRecent={(recent) => {
              search.onQuery(recent);
              search.onRemember(recent);
            }}
            onForget={search.onForget}
            onTerm={(term) => {
              const now = search.query ?? "";
              search.onQuery(now === "" || now.endsWith(" ") ? now + term : `${now} ${term}`);
              field.current?.focus();
            }}
          />
        ) : feed.loading ? (
          <Skeleton mode={mode} />
        ) : nothing && !feed.error ? (
          searching ? (
            <div className="empty list-empty" role="status">
              <h2>Nothing found for “{query}”</h2>
              {search.suggestion && (
                <p className="search-suggestion">
                  Did you mean{" "}
                  <button
                    type="button"
                    onClick={() => {
                      search.onQuery(search.suggestion);
                      field.current?.focus();
                    }}
                  >
                    {search.suggestion}
                  </button>
                  ?
                </p>
              )}
              <p>
                {search.indexing > 0
                  ? "Recent mail is still being indexed and may be missing. Try again in a moment."
                  : "Search looks at subjects, people and text. Try fewer words, or in:anywhere to include Trash and Junk."}
              </p>
              {search.indexing > 0 && (
                <button type="button" className="btn small" onClick={search.onRetry}>
                  Search again
                </button>
              )}
              <p className="server-search alone">
                <ServerRow server={search.server} />
              </p>
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
            {searching && (
              <p className="search-summary" role="status">
                <span>
                  {items.length}
                  {feed.more ? "+" : ""} {items.length === 1 && !feed.more ? "result" : "results"}
                </span>
                {search.indexing > 0 && (
                  <span className="search-indexing">
                    Still indexing: recent mail may be missing.
                    <button type="button" onClick={search.onRetry}>
                      Search again
                    </button>
                  </span>
                )}
                <ServerRow server={search.server} />
              </p>
            )}
            {searching && search.top.length > 0 && items.length > TOP_AFTER && (
              <section className="day top-results" aria-label="Top results">
                <h3 className="day-head">Top results</h3>
                <ul>{search.top.map((item) => row(item))}</ul>
              </section>
            )}
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
            {showCards && (
              <>
                {unreadCards.length === 0 && (
                  <section className="smart-card plain caught-up" aria-label="Unread">
                    <span className="zero-mark" aria-hidden="true">
                      <Icon name="check" size={18} />
                    </span>
                    <p>
                      <strong>You're all caught up.</strong> Nothing unread.
                    </p>
                  </section>
                )}
                {unreadCards.map((card) => (
                  <section key={card.key} className={`smart-card ${card.tone}`} aria-label={card.label}>
                    <div className="smart-card-top">
                      <h3 className="smart-card-head">
                        <Icon name={card.icon} size={15} />
                        {card.label}
                        <span className="smart-card-count">{card.total}</span>
                      </h3>
                      {card.onRead && (
                        <button
                          type="button"
                          className="card-read"
                          title="Mark all read"
                          aria-label={`Mark all ${card.label} read`}
                          onClick={card.onRead}
                        >
                          <Icon name="check" size={14} />
                        </button>
                      )}
                    </div>
                    <ul>{card.items.map((item) => row(item, card.screen))}</ul>
                    {card.total > card.items.length && <CardFoot items={card.items} total={card.total} onClick={card.onAll} />}
                  </section>
                ))}
                {items.length > 0 && (
                  <section className="smart-card plain" aria-label="Seen">
                    <div className="smart-card-top">
                      <h3 className="smart-card-head">
                        <Icon name="mailOpen" size={15} />
                        Seen
                      </h3>
                    </div>
                    <ul>{items.map((item) => row(item))}</ul>
                  </section>
                )}
              </>
            )}
            {!showCards && sections.map((section) => (
              <section key={section.label} className="day" aria-label={section.label}>
                <h3 className="day-head">{section.label}</h3>
                <ul>
                  {(bundlesIn.get(section.label) ?? []).map((bundle) => (
                    <BundleRow key={bundle.key} bundle={bundle} />
                  ))}
                  {section.items.map((item) => row(item, screenAll))}
                </ul>
              </section>
            ))}
            {searching && search.server.more && !feed.more && (
              <div className="server-more">
                <button type="button" className="btn small" onClick={search.server.onSearch}>
                  <Icon name="search" size={14} />
                  More from {search.server.many ? "the servers" : "the server"}
                </button>
              </div>
            )}
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

function SearchBar({
  field,
  query,
  onQuery,
  scope,
  onEnter,
  onDown,
  onClose,
}: {
  field: RefObject<HTMLInputElement | null>;
  query: string;
  onQuery: (query: string) => void;
  scope: SearchControls["scope"];
  /** Enter: remember the search and go to the first result. */
  onEnter: () => void;
  onDown: () => void;
  onClose: () => void;
}) {
  const one = scope?.accounts.find((account) => account.id === scope.accountId);
  return (
    <header className="list-head searching" data-tauri-drag-region>
      <label className="search-field">
        <Icon name="search" size={16} />
        <input
          ref={field}
          autoFocus
          value={query}
          placeholder={one ? `Search ${one.label}` : "Search all mail"}
          aria-label="Search mail"
          spellCheck={false}
          onChange={(event) => onQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.preventDefault();
              event.stopPropagation();
              onClose();
            } else if (event.key === "Enter") {
              event.preventDefault();
              onEnter();
            } else if (event.key === "ArrowDown") {
              event.preventDefault();
              onDown();
            }
          }}
        />
        {scope && (
          <select
            className="search-scope"
            aria-label="Which accounts to search"
            value={scope.accountId ?? ""}
            onChange={(event) => scope.onScope(event.target.value || null)}
          >
            <option value="">All accounts</option>
            {scope.accounts.map((account) => (
              <option key={account.id} value={account.id}>
                {account.label}
              </option>
            ))}
          </select>
        )}
        <button
          type="button"
          className="search-clear"
          title={query ? "Clear" : "Close the search (Esc)"}
          aria-label={query ? "Clear the search" : "Close the search"}
          onClick={() => {
            if (query) {
              onQuery("");
              field.current?.focus();
            } else onClose();
          }}
        >
          <Icon name="close" size={12} />
        </button>
      </label>
    </header>
  );
}

/**
 * Searching the servers: offered, under way, then what it found (and what is
 * not on this device yet, or could not be reached), or why it failed.
 */
function ServerRow({ server }: { server: ServerSearch }) {
  const where = server.many ? "the servers" : "the server";
  if (server.state === "idle") {
    return (
      <button type="button" className="server-ask" title="Finds words in mail not opened on this device" onClick={server.onSearch}>
        <Icon name="search" size={13} />
        Search on {where}
      </button>
    );
  }
  if (server.state === "loading") {
    return (
      <span className="server-status">
        <span className="spinner small" aria-hidden="true" />
        Asking {where}…
      </span>
    );
  }
  if (server.state === "failed") {
    return (
      <span className="server-status failed">
        <span className="server-words">
          {server.many ? "The servers" : "The server"} could not be searched: {server.error}{" "}
          <button type="button" onClick={server.onSearch}>
            Try again
          </button>
        </span>
      </span>
    );
  }
  const parts = [server.added > 0 ? `${server.added} more from ${where}` : `Nothing more on ${where}`];
  if (server.notHere > 0) parts.push(`${server.notHere} more on ${where}, not on this device yet`);
  return (
    <span className="server-status">
      <Icon name="check" size={13} />
      <span className="server-words">
        {parts.join(" · ")}
        {server.unreachable.length > 0 && (
          <span className="server-unreachable">Couldn't reach {server.unreachable.join(", ")}</span>
        )}
      </span>
    </span>
  );
}

/** What search understands, as examples to click into the field. */
const TIPS: { icon: IconName; term: string; example: string; text: string }[] = [
  { icon: "user", term: "from:", example: "from:lena", text: "From someone, by name or address" },
  { icon: "send", term: "to:", example: "to:marco", text: "To someone" },
  { icon: "text", term: "subject:", example: "subject:invoice", text: "Words in the subject" },
  { icon: "folder", term: "in:", example: "in:sent", text: "In one folder; in:anywhere adds Trash and Junk" },
  { icon: "paperclip", term: "has:attachment ", example: "has:attachment", text: "With files attached" },
  { icon: "mail", term: "is:unread ", example: "is:unread", text: "Unread, read or flagged" },
  { icon: "calendar", term: "after:", example: "after:2026-09-01", text: "After or before a day" },
  { icon: "quote", term: '"', example: '"exact words" -word', text: "A phrase as written; leave a word out" },
];

/** Under the field before anything is typed: tips, then recent searches. */
function SearchTips({
  recents,
  onRecent,
  onForget,
  onTerm,
}: {
  recents: string[];
  onRecent: (query: string) => void;
  onForget: (query: string) => void;
  onTerm: (term: string) => void;
}) {
  return (
    <div className="search-start">
      <h3 className="day-head">Tips</h3>
      <ul className="search-tips">
        {TIPS.map((tip) => (
          <li key={tip.example}>
            <button type="button" className="search-tip" onClick={() => onTerm(tip.term)}>
              <Icon name={tip.icon} size={17} />
              <span>
                <strong>{tip.example}</strong>
                <span className="muted">{tip.text}</span>
              </span>
            </button>
          </li>
        ))}
      </ul>
      <p className="search-hint muted">
        Combine them, as in <code>from:lena has:attachment after:2026-09-01</code>. Accents need not be typed.
      </p>
      {recents.length > 0 && (
        <>
          <h3 className="day-head">Recent searches</h3>
          <ul className="search-recents">
            {recents.map((recent) => (
              <li key={recent}>
                <button type="button" className="search-recent" onClick={() => onRecent(recent)}>
                  <Icon name="clock" size={15} />
                  <span>{recent}</span>
                </button>
                <button
                  type="button"
                  className="icon-btn small search-forget"
                  title="Remove from recent searches"
                  aria-label={`Remove “${recent}” from recent searches`}
                  onClick={() => onForget(recent)}
                >
                  <Icon name="close" size={12} />
                </button>
              </li>
            ))}
          </ul>
        </>
      )}
    </div>
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
