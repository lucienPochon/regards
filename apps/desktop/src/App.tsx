import { type CSSProperties, type KeyboardEvent, type PointerEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { type MarkResult, type MoveResult, type UndoInfo, syncAccount } from "./accounts";
import { type ActionHost, type Target, choose, messageActions } from "./actions";
import { Avatar } from "./Avatar";
import { Composer } from "./Composer";
import { type Draft, type Queued, newDraft } from "./compose";
import { Confirm, type ConfirmRequest } from "./Confirm";
import { DraftsList } from "./DraftsList";
import { Icon, type IconName } from "./icons";
import { LIST_MAX, LIST_MIN, clampList, useLayout } from "./layout";
import { type ListItem, sourcesFor, useDrafts, useFeed, useMail, useMarked, useOutbox, useService } from "./mail";
import { type Anchor, Menu, type MenuEntry, type MenuRequest } from "./Menu";
import { ListHeader, MessageList } from "./MessageList";
import { OutboxList } from "./OutboxList";
import { ReadingPane } from "./ReadingPane";
import { SendToast } from "./SendToast";
import { Settings, type SettingsTab } from "./Settings";
import { ShortcutHelp } from "./ShortcutHelp";
import { MOD_KEY, useShortcuts } from "./shortcuts";
import { Sidebar } from "./Sidebar";
import { presets } from "./snooze";
import { applyTheme } from "./theme";
import { TimePicker } from "./TimePicker";
import { UndoToast } from "./UndoToast";
import {
  INBOX,
  MARKED_ICONS,
  MARKED_LABELS,
  MARKED_VIEWS,
  ROLE_ICONS,
  ROLE_LABELS,
  type View,
  depth,
  leafName,

  sortFolders,
  viewKey,
} from "./views";
import { Welcome } from "./Welcome";

/** How long an error notice stays. */
const NOTICE_MS = 8000;

const trashLike = (item: ListItem) => item.role === "trash" || item.role === "junk";

/** What a set of listed messages have in common, for the actions on them. */
function targetOf(items: ListItem[]): Target {
  const accounts = new Set(items.map((item) => item.accountId));
  const folders = new Set(items.map((item) => item.mailboxId));
  return {
    ids: items.map((item) => item.id),
    single: items.length === 1 ? items[0].id : null,
    accountId: accounts.size === 1 ? items[0].accountId : null,
    mailboxId: folders.size === 1 ? items[0].mailboxId : null,
    seen: items.every((item) => item.seen),
    flagged: items.every((item) => item.flagged),
    disposable: items.length > 0 && items.every(trashLike),
    pinned: items.length > 0 && items.every((item) => item.pinned),
    aside: items.length > 0 && items.every((item) => item.aside),
    snoozed: items.length > 0 && items.every((item) => item.snoozed_until !== null),
  };
}

/** A time beside its preset: "18:00", or "Thu 08:00" when it is not today. */
function timeHint(until: number | "someday"): string | undefined {
  if (until === "someday") return undefined;
  const date = new Date(until);
  const time = date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  return date.toDateString() === new Date().toDateString()
    ? time
    : `${date.toLocaleDateString([], { weekday: "short" })} ${time}`;
}

/** When to send later: built from calendar days and hours, so 08:00 stays 08:00 across a clock change. */
function sendPresets(now = new Date()): { label: string; until: number }[] {
  const at = (days: number, hour: number) =>
    new Date(now.getFullYear(), now.getMonth(), now.getDate() + days, hour, 0, 0, 0).getTime();
  const list: { label: string; until: number }[] = [];
  const later = new Date(now.getTime() + 3 * 3600_000);
  later.setMinutes(Math.ceil(later.getMinutes() / 15) * 15, 0, 0);
  if (later.getDate() === now.getDate() && later.getHours() < 21) list.push({ label: "Later today", until: later.getTime() });
  if (now.getHours() < 17) list.push({ label: "This evening", until: at(0, 18) });
  list.push({ label: "Tomorrow morning", until: at(1, 8) });
  const toMonday = ((8 - now.getDay()) % 7) || 7;
  if (toMonday > 1) list.push({ label: "Monday morning", until: at(toMonday, 8) });
  return list;
}

type Picking = { title: string; confirm: string; initial: number; resolve: (time: number | null) => void };

export function App() {
  const { check, recheck } = useService();
  const mail = useMail();
  const drafts = useDrafts();
  const outbox = useOutbox();
  const [layout, setLayout] = useLayout();
  const [overlay, setOverlay] = useState(false);

  const [view, setView] = useState<View>(INBOX);
  const [open, setOpen] = useState<ListItem | null>(null);
  const [picked, setPicked] = useState<Set<number>>(() => new Set());
  const [composer, setComposer] = useState<{ id: number; mode: "full" | "panel" } | null>(null);
  const [queued, setQueued] = useState<Queued | null>(null);
  const [undo, setUndo] = useState<UndoInfo | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [help, setHelp] = useState(false);
  const [settings, setSettings] = useState<SettingsTab | null>(null);
  const [menu, setMenu] = useState<MenuRequest | null>(null);
  const [asking, setAsking] = useState<{ request: ConfirmRequest; resolve: (ok: boolean) => void } | null>(null);
  const [picking, setPicking] = useState<Picking | null>(null);
  const [syncing, setSyncing] = useState(false);
  const closeUndo = useCallback(() => setUndo(null), []);
  const closeQueued = useCallback(() => setQueued(null), []);

  const accounts = useMemo(() => (mail.state === "ready" ? mail.accounts : []), [mail]);
  const sources = useMemo(() => sourcesFor(accounts, view), [accounts, view]);
  const feed = useFeed(sources);
  const inInbox = view.kind === "role" && view.role === "inbox";
  const pinned = useMarked(accounts, "pinned", inInbox);
  const aside = useMarked(accounts, "aside", inInbox);
  const items = useRef(feed.items);
  items.current = feed.items;
  // The list's copy is fresher (flags change); fall back to the one opened.
  const openItem = open ? (feed.items.find((item) => item.id === open.id) ?? open) : null;
  // The open message's neighbours, kept for when it has already left the list
  // by the time its move is confirmed.
  const around = useRef<{ id: number; after: ListItem | null; before: ListItem | null } | null>(null);
  useEffect(() => {
    const index = open ? feed.items.findIndex((item) => item.id === open.id) : -1;
    if (open && index !== -1) {
      around.current = { id: open.id, after: feed.items[index + 1] ?? null, before: feed.items[index - 1] ?? null };
    }
  }, [open, feed.items]);

  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(null), NOTICE_MS);
    return () => window.clearTimeout(timer);
  }, [notice]);

  const failed = useCallback((error: unknown) => setNotice(String(error)), []);
  /** Replies and forwards open in a panel over the conversation; new messages take the whole pane. */
  const startDraft = useCallback(
    (start: Promise<Draft>, mode: "full" | "panel") => {
      start.then((draft) => setComposer({ id: draft.id, mode })).catch(failed);
    },
    [failed],
  );
  const compose = useCallback((start: Promise<Draft>) => startDraft(start, "panel"), [startDraft]);
  const editDraft = useCallback((id: number) => setComposer({ id, mode: "full" }), []);
  const confirm = useCallback(
    (request: ConfirmRequest) => new Promise<boolean>((resolve) => setAsking({ request, resolve })),
    [],
  );
  const pickFolder = useCallback(
    (accountId: string, current: number | null, anchor: Anchor | null) =>
      new Promise<number | null>((resolve) => {
        const folders = accounts.find((entry) => entry.account.id === accountId)?.mailboxes ?? [];
        setMenu({
          anchor: anchor ?? { x: window.innerWidth / 2 - 120, y: window.innerHeight / 4 },
          label: "Move to folder",
          filter: "Move to…",
          entries: sortFolders(folders.filter((folder) => folder.selectable && folder.id !== current)).map((folder) => ({
            label: leafName(folder),
            icon: folder.role ? ROLE_ICONS[folder.role] : "folder",
            depth: depth(folder),
            onSelect: () => resolve(folder.id),
          })),
          onDismiss: () => resolve(null),
        });
      }),
    [accounts],
  );
  // Messages that left the list: the next one opens in place of the one that went.
  const moved = useCallback((ids: number[], result: MoveResult | null) => {
    const gone = new Set(ids);
    setPicked(new Set());
    setOpen((current) => {
      if (!current || !gone.has(current.id)) return current;
      const list = items.current;
      const index = list.findIndex((item) => item.id === current.id);
      if (index === -1) {
        const near = around.current?.id === current.id ? around.current : null;
        return [near?.after, near?.before].find((item) => item && !gone.has(item.id)) ?? null;
      }
      const after = list.slice(index + 1).find((item) => !gone.has(item.id));
      const before = list
        .slice(0, index)
        .reverse()
        .find((item) => !gone.has(item.id));
      return after ?? before ?? null;
    });
    if (result?.undo) setUndo(result.undo);
  }, []);

  // A mark that keeps the message where it is (pin): only the undo.
  const marked = useCallback((result: MarkResult) => {
    if (result.undo) setUndo(result.undo);
  }, []);
  /** The palette of times, with a way to pick any date. */
  const pickTime = useCallback(
    (title: string, confirmLabel: string, choices: { label: string; until: number | "someday" }[]) =>
      new Promise<number | "someday" | null>((resolve) => {
        setMenu({
          anchor: { x: 0, y: 0 },
          variant: "palette",
          title,
          label: title,
          entries: [
            ...choices.map((choice) => ({
              label: choice.label,
              icon: "clock" as const,
              hint: timeHint(choice.until),
              onSelect: () => resolve(choice.until),
            })),
            "separator" as const,
            {
              label: "Pick a date and time…",
              icon: "plus" as const,
              onSelect: () =>
                setPicking({ title, confirm: confirmLabel, initial: Date.now() + 24 * 3600_000, resolve: (time) => resolve(time) }),
            },
          ],
          onDismiss: () => resolve(null),
        });
      }),
    [],
  );
  const pickSnooze = useCallback(() => pickTime("Snooze until", "Snooze", presets()), [pickTime]);
  const pickSendTime = useCallback(
    () => pickTime("Send at", "Schedule", sendPresets()).then((time) => (typeof time === "number" ? time : null)),
    [pickTime],
  );

  const host = useMemo<ActionHost>(
    () => ({ compose, moved, marked, failed, confirm, pickFolder, pickSnooze }),
    [compose, moved, marked, failed, confirm, pickFolder, pickSnooze],
  );
  const actionsForItems = useCallback((list: ListItem[]) => messageActions(targetOf(list), host), [host]);
  const actionsFor = useCallback((target: Target) => messageActions(target, host), [host]);

  const changeView = (next: View) => {
    setView(next);
    setOpen(null);
    setPicked(new Set());
    setOverlay(false);
  };

  const composeFrom =
    openItem?.accountId ?? (view.kind === "mailbox" ? view.accountId : accounts[0]?.account.id) ?? null;
  const writeNew = () => {
    if (composeFrom) startDraft(newDraft(composeFrom), "full");
  };

  // The folders being shown, synced now.
  const viewAccounts = [...new Set(sources.map((source) => source.accountId))];
  const syncAccounts = viewAccounts.length > 0 ? viewAccounts : accounts.map((entry) => entry.account.id);
  const busy = syncing || accounts.some((entry) => entry.sync?.running);
  const syncNow = () => {
    setSyncing(true);
    Promise.all(syncAccounts.map((id) => syncAccount(id)))
      .catch(failed)
      .finally(() => setSyncing(false));
  };

  const toggleSidebar = () => {
    if (layout.locked) setLayout({ locked: false });
    else setOverlay((now) => !now);
  };

  const step = (by: number) => {
    const list = items.current;
    const index = list.findIndex((item) => item.id === open?.id);
    const next = list[index + by];
    if (index !== -1 && next) setOpen(next);
  };

  const palette = () => {
    const entries: MenuEntry[] = [];
    if (openItem) {
      const message = actionsForItems([openItem]);
      for (const action of choose(message, "reply", "replyAll", "forward", "archive", "snooze", "pin", "aside", "flag", "read", "unread", "move", "delete")) {
        entries.push({ label: action.label, icon: action.icon, hint: action.keys[0], danger: action.danger, onSelect: () => action.run(null) });
      }
      entries.push("separator");
    }
    entries.push({ label: "New message", icon: "compose", hint: "c", onSelect: writeNew });
    entries.push({ label: "Check for new mail", icon: "refresh", onSelect: syncNow });
    const go = (target: View, label: string, icon: IconName) =>
      entries.push({ label: `Go to ${label}`, icon, onSelect: () => changeView(target) });
    go(INBOX, "Inbox", "inbox");
    for (const mark of MARKED_VIEWS) go({ kind: "marked", mark }, MARKED_LABELS[mark], MARKED_ICONS[mark]);
    go({ kind: "drafts" }, "Drafts", "draft");
    for (const role of ["sent", "archive", "junk", "trash"] as const) go({ kind: "role", role }, ROLE_LABELS[role], ROLE_ICONS[role]);
    if (outbox.length > 0) go({ kind: "outbox" }, "Outbox", "outbox");
    entries.push(
      {
        label: layout.mode === "split" ? "Open messages in place of the list" : "Open messages beside the list",
        icon: layout.mode === "split" ? "single" : "split",
        onSelect: () => setLayout({ mode: layout.mode === "split" ? "single" : "split" }),
      },
      { label: layout.locked ? "Fold the sidebar" : "Show the sidebar", icon: "sidebar", hint: "/", onSelect: toggleSidebar },
      { label: "Light theme", icon: "sun", onSelect: () => applyTheme("light", true) },
      { label: "Dark theme", icon: "moon", onSelect: () => applyTheme("dark", true) },
      { label: "Theme as the system", icon: "sun", onSelect: () => applyTheme("system", true) },
      { label: "Settings", icon: "settings", hint: `${MOD_KEY}+,`, onSelect: () => setSettings("accounts") },
      { label: "Keyboard shortcuts", icon: "keyboard", hint: "?", onSelect: () => setHelp(true) },
    );
    setMenu({ anchor: { x: 0, y: 0 }, variant: "palette", filter: "Type a command or a folder", label: "Commands", entries });
  };

  useShortcuts({
    "?": () => setHelp(true),
    c: writeNew,
    "Mod+,": () => setSettings("accounts"),
    "Mod+k": palette,
    "/": toggleSidebar,
    ...(overlay ? { Escape: () => setOverlay(false) } : {}),
  });

  // Dragging the list's edge sets the width directly; it is remembered on release.
  const content = useRef<HTMLElement>(null);
  const drag = useRef<{ x: number; width: number } | null>(null);
  const resize = (width: number) => content.current?.style.setProperty("--list-w", `${clampList(width)}px`);
  const splitter = {
    onPointerDown: (event: PointerEvent<HTMLDivElement>) => {
      event.currentTarget.setPointerCapture(event.pointerId);
      drag.current = { x: event.clientX, width: layout.listWidth };
      event.currentTarget.classList.add("dragging");
    },
    onPointerMove: (event: PointerEvent<HTMLDivElement>) => {
      if (drag.current) resize(drag.current.width + event.clientX - drag.current.x);
    },
    onPointerUp: (event: PointerEvent<HTMLDivElement>) => {
      if (!drag.current) return;
      event.currentTarget.classList.remove("dragging");
      setLayout({ listWidth: clampList(drag.current.width + event.clientX - drag.current.x) });
      drag.current = null;
    },
    onKeyDown: (event: KeyboardEvent<HTMLDivElement>) => {
      const by = event.key === "ArrowLeft" ? -20 : event.key === "ArrowRight" ? 20 : 0;
      if (!by) return;
      event.preventDefault();
      setLayout({ listWidth: clampList(layout.listWidth + by) });
    },
  };

  const service =
    check.state === "failed"
      ? `Could not reach the background service: ${check.message}`
      : check.state === "done" && (check.status.error || !check.status.running)
        ? `The background service is not running. ${check.status.error ?? ""}`
        : null;
  const banner = service && (
    <div className="banner" role="alert">
      <Icon name="alert" size={16} />
      <span>{service}</span>
      <button type="button" className="btn small" onClick={recheck}>
        Try again
      </button>
    </div>
  );

  if (mail.state === "ready" && accounts.length === 0) {
    return (
      <>
        {banner}
        <Welcome />
      </>
    );
  }

  const viewMailbox =
    view.kind === "mailbox"
      ? accounts.find((entry) => entry.account.id === view.accountId)?.mailboxes.find((m) => m.id === view.mailboxId)
      : undefined;
  const viewAccount = view.kind === "mailbox" ? accounts.find((entry) => entry.account.id === view.accountId) : undefined;
  const title =
    view.kind === "role"
      ? ROLE_LABELS[view.role]
      : view.kind === "marked"
        ? MARKED_LABELS[view.mark]
        : view.kind === "mailbox"
          ? viewMailbox
            ? leafName(viewMailbox)
            : "Folder"
          : view.kind === "drafts"
            ? "Drafts"
            : "Outbox";
  const subtitle =
    view.kind === "mailbox"
      ? viewAccount?.account.email
      : view.kind === "role" || view.kind === "marked"
        ? accounts.length > 1
          ? "All accounts"
          : accounts[0]?.account.email
        : view.kind === "drafts"
          ? `${drafts.length} ${drafts.length === 1 ? "draft" : "drafts"}`
          : `${outbox.length} waiting`;
  const empty =
    view.kind === "role" && view.role === "inbox"
      ? { title: "You're all caught up", body: "New mail shows here as it arrives." }
      : view.kind === "marked"
        ? {
            pinned: { title: "Nothing pinned", body: "Pin a message (p) to keep it in reach here." },
            snoozed: { title: "Nothing snoozed", body: "Snooze a message (b) and it comes back as new at the time you pick." },
            aside: { title: "Nothing set aside", body: "Set a message aside (Shift+A) to take it out of the inbox until you need it." },
          }[view.mark]
        : { title: "Nothing here", body: "This folder has no messages." };
  const folders = openItem
    ? (accounts.find((entry) => entry.account.id === openItem.accountId)?.mailboxes ?? []).filter((m) => m.selectable)
    : [];

  // With several accounts, the title chooses between all of them and one.
  const role = view.kind === "role" ? view.role : (viewMailbox?.role ?? null);
  const onTitle =
    accounts.length > 1 && role
      ? (anchor: Anchor) =>
          setMenu({
            anchor,
            label: "Accounts",
            entries: [
              { label: "All accounts", icon: "inbox", onSelect: () => changeView({ kind: "role", role }) },
              "separator",
              ...accounts.flatMap((entry) => {
                const mailbox = entry.mailboxes.find((m) => m.role === role && m.selectable);
                return mailbox
                  ? [
                      {
                        label: entry.account.email,
                        icon: "user" as const,
                        onSelect: () => changeView({ kind: "mailbox", mailboxId: mailbox.id, accountId: entry.account.id }),
                      },
                    ]
                  : [];
              }),
            ],
          })
      : undefined;

  const listPane =
    view.kind === "drafts" ? (
      <section className="list-pane" aria-label="Drafts">
        <ListHeader title={title} subtitle={subtitle}>
          <button type="button" className="icon-btn" title="New message (c)" aria-label="New message" onClick={writeNew}>
            <Icon name="compose" />
          </button>
        </ListHeader>
        <DraftsList drafts={drafts} open={composer?.id ?? null} onOpen={editDraft} onFailed={failed} />
      </section>
    ) : view.kind === "outbox" ? (
      <section className="list-pane" aria-label="Outbox">
        <ListHeader title={title} subtitle={subtitle} />
        <OutboxList entries={outbox} onEdit={editDraft} onFailed={failed} />
      </section>
    ) : (
      <MessageList
        key={viewKey(view)}
        title={title}
        subtitle={subtitle}
        onTitle={onTitle}
        onCompose={writeNew}
        canCompose={composeFrom !== null}
        feed={feed}
        mode={layout.mode}
        openId={openItem?.id ?? null}
        picked={picked}
        onOpen={setOpen}
        onPick={setPicked}
        actionsFor={actionsForItems}
        onMenu={setMenu}
        empty={empty}
        bundle={inInbox ? { label: "Pinned", icon: "pin", messages: pinned, onOpen: () => changeView({ kind: "marked", mark: "pinned" }) } : null}
        onSplit={layout.mode === "single" ? () => setLayout({ mode: "split" }) : undefined}
      />
    );

  const reader = openItem && (
    <ReadingPane
      key={openItem.id}
      item={openItem}
      folders={folders}
      mode={layout.mode}
      actionsFor={actionsFor}
      onMenu={setMenu}
      onClose={() => setOpen(null)}
      onStep={layout.mode === "single" ? step : undefined}
    />
  );

  let body;
  if (composer?.mode === "full") {
    body = null;
  } else if (layout.mode === "single") {
    body = reader && picked.size === 0 ? reader : listPane;
  } else {
    body = (
      <>
        {listPane}
        <div
          className="splitter"
          role="separator"
          aria-orientation="vertical"
          aria-label="Resize the message list"
          aria-valuenow={layout.listWidth}
          aria-valuemin={LIST_MIN}
          aria-valuemax={LIST_MAX}
          tabIndex={0}
          {...splitter}
        />
        <div className="reader-column">
          {picked.size > 0 ? (
            <div className="empty">
              <div className="stack" aria-hidden="true">
                <span />
                <span />
                <span>{picked.size}</span>
              </div>
              <h2>{picked.size === 1 ? "1 message selected" : `${picked.size} messages selected`}</h2>
              <p>The actions at the top of the list apply to all of them.</p>
            </div>
          ) : (
            (reader ?? (
              <div className="empty">
                <span className="zero-mark" aria-hidden="true">
                  <Icon name="mail" size={30} />
                </span>
                <h2>{view.kind === "drafts" ? "Pick a draft to keep writing" : "No message open"}</h2>
                <p>
                  Choose a message to read it here. <kbd>j</kbd> and <kbd>k</kbd> move through the list.
                </p>
              </div>
            ))
          )}
        </div>
      </>
    );
  }

  const showBubble = inInbox && aside.length > 0 && !openItem && composer?.mode !== "full";
  const asideFaces = aside.slice(0, 3);

  return (
    <div className={layout.locked ? "app locked" : "app"}>
      {banner}
      <Sidebar
        mail={mail}
        drafts={drafts}
        outbox={outbox}
        view={view}
        onView={changeView}
        expanded={layout.locked}
        locked={layout.locked}
        onToggleLock={() => setLayout({ locked: !layout.locked })}
        onExpand={() => setOverlay(true)}
        onSettings={setSettings}
        onHelp={() => setHelp(true)}
        onSync={syncNow}
        syncing={busy}
      />
      <main
        className={`content ${layout.mode}`}
        ref={content}
        style={{ "--list-w": `${layout.listWidth}px` } as CSSProperties}
      >
        {body}
        {composer && (
          <Composer
            key={composer.id}
            draftId={composer.id}
            mode={composer.mode}
            onMode={(mode) => setComposer({ ...composer, mode })}
            onClose={() => setComposer(null)}
            onQueued={(result) => {
              setComposer(null);
              setQueued(result);
            }}
            pickSendTime={pickSendTime}
          />
        )}
        <div className="dock">
          {notice && (
            <div className="toast error" role="alert">
              <Icon name="alert" size={16} />
              <span>{notice}</span>
              <button type="button" onClick={() => setNotice(null)}>
                Dismiss
              </button>
            </div>
          )}
          {undo && <UndoToast undo={undo} onDone={closeUndo} />}
          {queued && (
            <SendToast
              key={queued.outbox_id}
              queued={queued}
              onDone={closeQueued}
              onUndone={(draftId) => {
                setQueued(null);
                if (draftId !== null) editDraft(draftId);
              }}
            />
          )}
          {showBubble && (
            <button
              type="button"
              className="aside-bubble"
              title="Set aside (Shift+A to add)"
              onClick={() => changeView({ kind: "marked", mark: "aside" })}
            >
              <span className="faces">
                {asideFaces.map((message) => (
                  <Avatar key={message.id} person={message.from} size="xs" />
                ))}
              </span>
              {aside.length} set aside
            </button>
          )}
        </div>
      </main>
      {overlay && !layout.locked && (
        <>
          <div className="scrim" onMouseDown={() => setOverlay(false)} />
          <Sidebar
            mail={mail}
            drafts={drafts}
            outbox={outbox}
            view={view}
            onView={changeView}
            expanded
            overlay
            locked={false}
            onToggleLock={() => {
              setOverlay(false);
              setLayout({ locked: true });
            }}
            onExpand={() => {}}
            onSettings={(tab) => {
              setOverlay(false);
              setSettings(tab);
            }}
            onHelp={() => setHelp(true)}
            onSync={syncNow}
            syncing={busy}
          />
        </>
      )}
      {help && <ShortcutHelp onClose={() => setHelp(false)} />}
      {settings && (
        <Settings
          tab={settings}
          onTab={setSettings}
          mail={mail}
          service={check}
          onRecheck={recheck}
          layout={layout}
          onLayout={setLayout}
          confirm={confirm}
          onClose={() => setSettings(null)}
        />
      )}
      {asking && (
        <Confirm
          request={asking.request}
          onAnswer={(ok) => {
            asking.resolve(ok);
            setAsking(null);
          }}
        />
      )}
      {menu && <Menu request={menu} onClose={() => setMenu(null)} />}
      {picking && (
        <TimePicker
          title={picking.title}
          confirm={picking.confirm}
          initial={picking.initial}
          onAnswer={(time) => {
            picking.resolve(time);
            setPicking(null);
          }}
        />
      )}
    </div>
  );
}

