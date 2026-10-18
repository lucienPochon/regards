import { type MouseEvent, type ReactNode, useCallback, useEffect, useRef } from "react";
import { type Action, choose } from "./actions";
import { Avatar } from "./Avatar";
import { laterDate, listDate, personName } from "./format";
import { Icon } from "./icons";
import type { ListItem, MessageFeed } from "./mail";
import type { MenuEntry, MenuRequest } from "./Menu";
import { useShortcuts } from "./shortcuts";

/** "Someday" in the daemon: no time to come back (regards_store::marks::SOMEDAY). */
const SOMEDAY = 32_503_680_000_000;

/** The top of the message column: the folder's name, and buttons on the right. */
export function ListHeader({
  title,
  subtitle,
  rail,
  onToggleRail,
  children,
}: {
  title: string;
  subtitle?: string;
  rail: boolean;
  onToggleRail: () => void;
  children?: ReactNode;
}) {
  return (
    <header className="list-head">
      <button
        type="button"
        className="icon-btn"
        aria-label={rail ? "Show folder names" : "Hide folder names"}
        title={rail ? "Show folder names" : "Hide folder names"}
        aria-pressed={!rail}
        onClick={onToggleRail}
      >
        <Icon name="sidebar" />
      </button>
      <div className="list-title">
        <h1>{title}</h1>
        {subtitle && <p>{subtitle}</p>}
      </div>
      {children}
    </header>
  );
}

/** When the list is this close to its end, the next page is fetched. */
const PREFETCH_PX = 800;

export function MessageList({
  feed,
  openId,
  picked,
  onOpen,
  onPick,
  actionsFor,
  onMenu,
  empty,
}: {
  feed: MessageFeed;
  openId: number | null;
  picked: ReadonlySet<number>;
  onOpen: (item: ListItem) => void;
  onPick: (ids: Set<number>) => void;
  actionsFor: (items: ListItem[]) => Action[];
  onMenu: (request: MenuRequest) => void;
  empty: { title: string; body: string };
}) {
  const { items } = feed;
  const scroller = useRef<HTMLDivElement>(null);
  // Where a Shift+click range starts.
  const anchor = useRef<number | null>(null);

  const toggle = (item: ListItem) => {
    const next = new Set(picked);
    if (next.size === 0 && openId !== null && openId !== item.id) next.add(openId);
    if (next.has(item.id)) next.delete(item.id);
    else next.add(item.id);
    anchor.current = item.id;
    onPick(next);
  };

  const click = (event: MouseEvent, item: ListItem, index: number) => {
    if (event.shiftKey) {
      const start = items.findIndex((other) => other.id === (anchor.current ?? openId));
      if (start !== -1) {
        const next = new Set(picked);
        if (openId !== null) next.add(openId);
        const [from, to] = start < index ? [start, index] : [index, start];
        for (const other of items.slice(from, to + 1)) next.add(other.id);
        onPick(next);
        return;
      }
    }
    if (event.metaKey || event.ctrlKey) {
      toggle(item);
      return;
    }
    anchor.current = item.id;
    onPick(new Set());
    onOpen(item);
  };

  // Next and previous; from nothing open, "next" opens the newest.
  const step = (by: number) => {
    const index = items.findIndex((item) => item.id === openId);
    const target = index === -1 ? (by > 0 ? 0 : -1) : index + by;
    const item = items[target];
    if (!item) return;
    anchor.current = item.id;
    onPick(new Set());
    onOpen(item);
  };
  const open = items.find((item) => item.id === openId);
  useShortcuts({
    j: () => step(1),
    ArrowDown: () => step(1),
    k: () => step(-1),
    ArrowUp: () => step(-1),
    ...(open ? { x: () => toggle(open) } : {}),
    "Mod+a": () => onPick(new Set(items.map((item) => item.id))),
  });

  useEffect(() => {
    scroller.current?.querySelector(".row.open")?.scrollIntoView({ block: "nearest" });
  }, [openId]);

  // Fetch more while the end is near: on scroll, and when a page did not fill the column.
  const { more, loadMore } = feed;
  const nearEnd = useCallback(() => {
    const element = scroller.current;
    if (!element || !more) return;
    if (element.scrollTop + element.clientHeight > element.scrollHeight - PREFETCH_PX) loadMore();
  }, [more, loadMore]);
  useEffect(nearEnd, [items.length, nearEnd]);

  const menu = (event: MouseEvent, item: ListItem) => {
    event.preventDefault();
    const targets = picked.has(item.id) && picked.size > 1 ? items.filter((other) => picked.has(other.id)) : [item];
    const actions = actionsFor(targets);
    const anchorAt = { x: event.clientX, y: event.clientY };
    const groups = [
      choose(actions, "reply", "replyAll", "forward"),
      choose(actions, "read", "unread", "flag"),
      choose(actions, "pin", "snooze", "aside"),
      choose(actions, "archive", "move"),
      choose(actions, "delete"),
    ].filter((group) => group.length > 0);
    const entries: MenuEntry[] = groups.flatMap((group, index) => [
      ...(index > 0 ? (["separator"] as const) : []),
      ...group.map((action) => ({
        label: action.label,
        icon: action.icon,
        hint: action.keys[0],
        danger: action.danger,
        onSelect: () => action.run(anchorAt),
      })),
    ]);
    onMenu({ anchor: anchorAt, entries, label: targets.length > 1 ? `${targets.length} messages` : "Message" });
  };

  return (
    <div className="list-scroll" ref={scroller} onScroll={nearEnd}>
      {feed.error && (
        <p className="list-error" role="alert">
          <Icon name="alert" size={16} /> {feed.error}
        </p>
      )}
      {feed.loading ? (
        <Skeleton />
      ) : items.length === 0 && !feed.error ? (
        <div className="empty list-empty">
          <span className="empty-mark">
            <Icon name="check" size={28} />
          </span>
          <h2>{empty.title}</h2>
          <p>{empty.body}</p>
        </div>
      ) : (
        <ul className={picked.size > 0 ? "rows picking" : "rows"} aria-label="Messages">
          {items.map((item, index) => {
            const isPicked = picked.has(item.id);
            const hover = choose(actionsFor([item]), "pin", "read", "unread", "snooze", "archive", "delete");
            return (
              <li
                key={`${item.mailboxId}:${item.id}`}
                className={[
                  "row",
                  item.seen ? "" : "unread",
                  item.id === openId && picked.size === 0 ? "open" : "",
                  isPicked ? "picked" : "",
                ].join(" ")}
                onContextMenu={(event) => menu(event, item)}
              >
                {!item.seen && <span className="unread-dot" aria-label="Unread" />}
                <button
                  type="button"
                  className="row-avatar"
                  aria-label={isPicked ? "Unselect" : "Select"}
                  aria-pressed={isPicked}
                  onClick={() => toggle(item)}
                >
                  <Avatar person={item.from} />
                  <span className="check" aria-hidden="true">
                    <Icon name="check" size={16} />
                  </span>
                </button>
                <button type="button" className="row-main" onClick={(event) => click(event, item, index)}>
                  <span className="row-line">
                    <span className="from">{personName(item.from)}</span>
                    {item.snoozed_until !== null && item.snoozed_until < SOMEDAY ? (
                      <span className="date snoozed" title="Comes back then">
                        <Icon name="clock" size={13} />
                        {laterDate(item.snoozed_until)}
                      </span>
                    ) : (
                      <span className="date">{listDate(item.received_at)}</span>
                    )}
                  </span>
                  <span className="row-line">
                    <span className="subject">{item.subject || "(no subject)"}</span>
                    <span className="marks">
                      {item.pinned && <Icon name="pinFilled" size={14} className="pinned" />}
                      {item.answered && <Icon name="reply" size={14} className="answered" />}
                      {item.flagged && <Icon name="flagFilled" size={14} className="flagged" />}
                    </span>
                  </span>
                </button>
                <span className="row-actions">
                  {hover.map((action) => (
                    <button
                      key={action.id}
                      type="button"
                      className="icon-btn small"
                      title={`${action.label} (${action.keys[0]})`}
                      aria-label={action.label}
                      onClick={() => action.run(null)}
                    >
                      <Icon name={action.icon} size={16} />
                    </button>
                  ))}
                </span>
              </li>
            );
          })}
          {feed.more && (
            <li className="row-more" aria-hidden="true">
              <span className="spinner" />
            </li>
          )}
        </ul>
      )}
    </div>
  );
}

function Skeleton() {
  return (
    <ul className="rows" aria-label="Loading messages" aria-busy="true">
      {Array.from({ length: 9 }, (_, index) => (
        <li key={index} className="row skeleton">
          <span className="bone avatar-bone" />
          <span className="row-main">
            <span className="bone line-a" />
            <span className="bone line-b" />
          </span>
        </li>
      ))}
    </ul>
  );
}
