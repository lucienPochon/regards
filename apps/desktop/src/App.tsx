import { invoke } from "@tauri-apps/api/core";
import { type CSSProperties, type KeyboardEvent, type PointerEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  type MarkResult,
  type MoveResult,
  type Person,
  type UndoInfo,
  archiveMessages,
  moveMessages,
  pinMessages,
  remindMessages,
  setAside,
  syncAccount,
  trashMessages,
} from "./accounts";
import { type Action, type ActionHost, type Target, choose, messageActions } from "./actions";
import { Avatar } from "./Avatar";
import { Composer } from "./Composer";
import { type Draft, type Queued, newDraft, saveDraft } from "./compose";
import { Confirm, type ConfirmRequest } from "./Confirm";
import { DraftsList } from "./DraftsList";
import { Icon, type IconName } from "./icons";
import { LIST_MAX, LIST_MIN, clampList, useLayout } from "./layout";
import {
  type ListItem,
  sectionSources,
  sourcesFor,
  useDrafts,
  useFeed,
  useFirstPage,
  useMail,
  useMarked,
  useOutbox,
  useService,
  useSmartOverview,
} from "./mail";
import { type Anchor, Menu, type MenuEntry, type MenuRequest } from "./Menu";
import { type Bundle, ListHeader, MessageList, type Screener } from "./MessageList";
import { SenderCard, type SenderRequest } from "./SenderCard";
import { SECTION_LABELS, type Screening, type Section, blockSender, screening, setSender } from "./smart";
import { ViewChooser } from "./ViewChooser";
import { OutboxList } from "./OutboxList";
import { ReadingPane } from "./ReadingPane";
import { SendToast } from "./SendToast";
import { Settings, type SettingsTab } from "./Settings";
import { ShortcutHelp } from "./ShortcutHelp";
import { MOD_KEY, useShortcuts } from "./shortcuts";
import { type DragPayload, Sidebar } from "./Sidebar";
import { useSidebarPrefs } from "./sidebar-prefs";
import { presets, reminderPresets, snoozeMessages } from "./snooze";
import { applyTheme } from "./theme";
import { TimePicker } from "./TimePicker";
import { PLATFORM, TitleBar } from "./TitleBar";
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

/** Where a part of the Smart Inbox leads back to: the inbox it was opened from. */
const inboxOf = (view: Extract<View, { kind: "section" }>): View =>
  view.inbox ? { kind: "mailbox", ...view.inbox } : INBOX;

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
    reminding: items.length > 0 && items.every((item) => item.remind_at !== null),
    reminded: items.some((item) => item.reminded),
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

/** The palette's order of message actions; archive is Done. */
const PALETTE_ACTIONS = [
  "archive",
  "snooze",
  "pin",
  "aside",
  "flag",
  "read",
  "unread",
  "move",
  "junk",
  "delete",
  "reply",
  "replyAll",
  "forward",
  "remind",
] as const;

type Picking = { title: string; confirm: string; initial: number; resolve: (time: number | null) => void };

export function App() {
  const { check, recheck } = useService();
  const mail = useMail();
  const drafts = useDrafts();
  const outbox = useOutbox();
  const [layout, setLayout] = useLayout();
  const [prefs, sidebarChanges] = useSidebarPrefs();
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

  const [chooser, setChooser] = useState<Anchor | null>(null);
  const [sender, setSenderCard] = useState<SenderRequest | null>(null);
  const [screen, setScreen] = useState<Screening>("off");
  useEffect(() => {
    screening().then(setScreen, () => {});
  }, []);

  const accounts = useMemo(() => (mail.state === "ready" ? mail.accounts : []), [mail]);
  const ownAddresses = useMemo(() => new Set(accounts.map((entry) => entry.account.email.toLowerCase())), [accounts]);
  const inInbox = view.kind === "role" && view.role === "inbox";
  // The Smart Inbox: every inbox, or one account's when its inbox is open.
  const accountInbox =
    view.kind === "mailbox"
      ? accounts.find(
          (entry) =>
            entry.account.id === view.accountId && entry.mailboxes.some((m) => m.id === view.mailboxId && m.role === "inbox"),
        )
      : undefined;
  const smartScope = useMemo(
    () => (layout.inboxView !== "smart" ? null : inInbox ? accounts : accountInbox ? [accountInbox] : null),
    [layout.inboxView, inInbox, accounts, accountInbox],
  );
  const smartOn = smartScope !== null;
  const sources = useMemo(
    () => (smartScope ? sectionSources(smartScope, "people") : sourcesFor(accounts, view)),
    [smartScope, accounts, view],
  );
  const feed = useFeed(sources);
  const scope = useMemo(() => smartScope ?? [], [smartScope]);
  const overview = useSmartOverview(scope, smartOn);
  const priorityItems = useFirstPage(useMemo(() => sectionSources(scope, "priority"), [scope]), 3, smartOn);
  const newSenderItems = useFirstPage(
    useMemo(() => sectionSources(scope, "new_senders"), [scope]),
    3,
    smartOn && screen === "section",
  );
  const notificationItems = useFirstPage(useMemo(() => sectionSources(scope, "notifications"), [scope]), 30, smartOn);
  const newsletterItems = useFirstPage(useMemo(() => sectionSources(scope, "newsletters"), [scope]), 30, smartOn);
  const pinnedItems = useMarked(scope, "pinned", smartOn);
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
  const folderOf = useCallback(
    (accountId: string, role: string) =>
      accounts.find((entry) => entry.account.id === accountId)?.mailboxes.find((m) => m.role === role && m.selectable)?.id ??
      null,
    [accounts],
  );
  /** Move to: a page of the palette listing the account's folders. */
  const pickFolder = useCallback(
    (accountId: string, current: number | null) =>
      new Promise<number | null>((resolve) => {
        const folders = accounts.find((entry) => entry.account.id === accountId)?.mailboxes ?? [];
        setMenu({
          anchor: { x: 0, y: 0 },
          variant: "palette",
          title: "Move to",
          label: "Move to folder",
          filter: "Type a folder name",
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
  /** A palette page of times, with a way to pick any date. */
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
  const pickReminder = useCallback(
    () =>
      pickTime("Remind me if no reply by", "Remind me", reminderPresets()).then((time) =>
        typeof time === "number" ? time : null,
      ),
    [pickTime],
  );
  const pickSendTime = useCallback(
    () => pickTime("Send at", "Schedule", sendPresets()).then((time) => (typeof time === "number" ? time : null)),
    [pickTime],
  );
  const junkFolder = useCallback((accountId: string) => folderOf(accountId, "junk"), [folderOf]);

  const host = useMemo<ActionHost>(
    () => ({ compose, moved, marked, failed, confirm, pickFolder, pickSnooze, pickReminder, junkFolder }),
    [compose, moved, marked, failed, confirm, pickFolder, pickSnooze, pickReminder, junkFolder],
  );
  const actionsForItems = useCallback((list: ListItem[]) => messageActions(targetOf(list), host), [host]);
  const actionsFor = useCallback((target: Target) => messageActions(target, host), [host]);

  const changeView = useCallback((next: View) => {
    setView(next);
    setOpen(null);
    setPicked(new Set());
    setOverlay(false);
  }, []);

  const composeFrom =
    openItem?.accountId ?? (view.kind === "mailbox" ? view.accountId : accounts[0]?.account.id) ?? null;
  const writeNew = () => {
    if (composeFrom) startDraft(newDraft(composeFrom), "full");
  };
  /** A new message to someone, from the account they wrote to (the sender card's Write to them). */
  const writeTo = (person: Person, accountId: string) =>
    startDraft(
      newDraft(accountId).then((draft) =>
        saveDraft({ id: draft.id, to: [person], cc: [], bcc: [], subject: "", body: "", html: null }),
      ),
      "full",
    );
  /** The sender card for a message: who sent it, where their mail goes, why it is here. */
  const aboutSender = (about: { id: number; accountId: string; from: Person | null }, anchor?: Anchor) => {
    if (!about.from) return;
    setSenderCard({
      anchor: anchor ?? { x: window.innerWidth / 2 - 160, y: window.innerHeight / 4 },
      messageId: about.id,
      accountId: about.accountId,
      person: about.from,
    });
  };
  const senderEntry = (about: ListItem): MenuEntry => ({
    label: "The sender, and why this is here…",
    icon: "info",
    onSelect: () => aboutSender(about),
  });

  // The folders being shown, synced now.
  const viewAccounts = [...new Set(sources.map((source) => source.accountId))];
  const syncAccounts = viewAccounts.length > 0 ? viewAccounts : accounts.map((entry) => entry.account.id);
  const busy = syncing || accounts.some((entry) => entry.sync?.running);
  const syncProblem = accounts.map((entry) => entry.error ?? entry.sync?.last_error).find(Boolean) ?? null;
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

  /** Messages dropped on a sidebar folder: filed there, or marked as the folder says. */
  const dropOn = (target: View, payload: DragPayload) => {
    const { ids } = payload;
    if (ids.length === 0) return;
    const left = (result: MarkResult) => moved(ids, { moved: result.changed, undo: result.undo });
    // Per account: each message goes to its own account's folder with that role.
    const toRole = (role: string) => {
      const byAccount = new Map<string, number[]>();
      payload.accounts.forEach((account, index) => byAccount.set(account, [...(byAccount.get(account) ?? []), ids[index]]));
      for (const [account, group] of byAccount) {
        const folder = folderOf(account, role);
        if (folder === null) continue;
        moveMessages(group, folder)
          .then((result) => moved(group, result))
          .catch(failed);
      }
    };
    if (target.kind === "role") {
      if (target.role === "archive") archiveMessages(ids).then((result) => moved(ids, result)).catch(failed);
      else if (target.role === "trash") trashMessages(ids).then((result) => moved(ids, result)).catch(failed);
      else toRole(target.role);
    } else if (target.kind === "mailbox") {
      if (payload.accounts.some((account) => account !== target.accountId)) {
        failed("Messages can only be moved to a folder of their own account.");
        return;
      }
      moveMessages(ids, target.mailboxId)
        .then((result) => moved(ids, result))
        .catch(failed);
    } else if (target.kind === "marked") {
      if (target.mark === "pinned") pinMessages(ids, true).then(marked).catch(failed);
      else if (target.mark === "aside") setAside(ids, true).then(left).catch(failed);
      else if (target.mark === "snoozed") {
        void pickSnooze().then((until) => {
          if (until !== null) snoozeMessages(ids, until).then(left).catch(failed);
        });
      } else if (target.mark === "reminders") {
        void pickReminder().then((at) => {
          if (at !== null) remindMessages(ids, at).then(marked).catch(failed);
        });
      }
    }
  };

  /**
   * The command palette: the actions on what it was opened for (a row, the
   * selection, a conversation or one message), then going places and the app's own commands.
   */
  const palette = (actions: Action[] | null, title?: string, extra: MenuEntry[] = []) => {
    const entries: MenuEntry[] = [];
    if (actions) {
      for (const action of choose(actions, ...PALETTE_ACTIONS)) {
        const done = action.id === "archive";
        entries.push({
          label: done ? "Done (archive)" : action.label,
          icon: done ? "check" : action.icon,
          keys: action.keys[0],
          danger: action.danger,
          onSelect: () => action.run(null),
        });
      }
      entries.push(...extra, "separator");
    }
    const go = (target: View, label: string, icon: IconName) =>
      entries.push({ label: `Go to ${label}`, icon, onSelect: () => changeView(target) });
    go(INBOX, "Inbox", "inbox");
    for (const mark of MARKED_VIEWS) go({ kind: "marked", mark }, MARKED_LABELS[mark], MARKED_ICONS[mark]);
    go({ kind: "drafts" }, "Drafts", "draft");
    for (const role of ["sent", "archive", "junk", "trash"] as const) go({ kind: "role", role }, ROLE_LABELS[role], ROLE_ICONS[role]);
    go({ kind: "outbox" }, "Outbox", "outbox");
    entries.push({
      label: "Go to folder…",
      icon: "folder",
      onSelect: () =>
        setMenu({
          anchor: { x: 0, y: 0 },
          variant: "palette",
          title: "Go to folder",
          label: "Go to folder",
          filter: "Type a folder name",
          entries: accounts.flatMap((entry) =>
            sortFolders(entry.mailboxes.filter((m) => m.selectable)).map((mailbox) => ({
              label: leafName(mailbox),
              icon: mailbox.role ? ROLE_ICONS[mailbox.role] : "folder",
              depth: depth(mailbox),
              detail: accounts.length > 1 ? entry.account.email : undefined,
              onSelect: () => changeView({ kind: "mailbox", mailboxId: mailbox.id, accountId: entry.account.id }),
            })),
          ),
        }),
    });
    entries.push(
      "separator",
      { label: "New message", icon: "compose", keys: "c", onSelect: writeNew },
      { label: "Check for new mail", icon: "refresh", onSelect: syncNow },
      { label: layout.locked ? "Fold the sidebar" : "Show the sidebar", icon: "sidebar", keys: "/", onSelect: toggleSidebar },
      {
        label: layout.mode === "split" ? "Open messages in place of the list" : "Open messages beside the list",
        icon: layout.mode === "split" ? "single" : "split",
        onSelect: () => setLayout({ mode: layout.mode === "split" ? "single" : "split" }),
      },
      { label: "Light theme", icon: "sun", onSelect: () => applyTheme("light", true) },
      { label: "Dark theme", icon: "moon", onSelect: () => applyTheme("dark", true) },
      { label: "Theme as the system", icon: "sun", onSelect: () => applyTheme("system", true) },
      { label: "Settings", icon: "settings", keys: "Mod+,", onSelect: () => setSettings("accounts") },
      { label: "Keyboard shortcuts", icon: "keyboard", keys: "?", onSelect: () => setHelp(true) },
    );
    setMenu({
      anchor: { x: 0, y: 0 },
      variant: "palette",
      title: title ?? "Actions",
      filter: "Type a command, or “go to”",
      label: "Commands",
      entries,
    });
  };

  const quit = () => {
    invoke("app_quit").catch(failed);
  };
  /** The window's ⌄ menu on Windows: the app's own commands. */
  const appMenu = (anchor: Anchor) =>
    setMenu({
      anchor,
      label: "Regards",
      entries: [
        { label: "About Regards", onSelect: () => setSettings("about") },
        "separator",
        { label: "Settings…", hint: `${MOD_KEY}+,`, onSelect: () => setSettings("accounts") },
        { label: "Accounts…", onSelect: () => setSettings("accounts") },
        { label: "Keyboard shortcuts", hint: "?", onSelect: () => setHelp(true) },
        "separator",
        { label: "Quit Regards", hint: `${MOD_KEY}+Q`, onSelect: quit },
      ],
    });

  useShortcuts({
    "?": () => setHelp(true),
    c: writeNew,
    "Mod+,": () => setSettings("accounts"),
    "Mod+k": () => palette(openItem ? actionsForItems([openItem]) : null, openItem?.subject || undefined),
    "Mod+q": quit,
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
    // The first run still has the window's own title band and caption buttons.
    return (
      <div className={["welcome-shell", PLATFORM !== "linux" ? "framed" : "", PLATFORM].join(" ")}>
        {PLATFORM !== "linux" && <div className="drag-band" data-tauri-drag-region aria-hidden="true" />}
        {banner}
        <Welcome />
        <TitleBar onAppMenu={appMenu} />
        {menu && <Menu request={menu} onClose={() => setMenu(null)} />}
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
            screening={screen}
            onScreening={setScreen}
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
      </div>
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
      : view.kind === "section"
        ? SECTION_LABELS[view.section]
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
    view.kind === "section"
      ? `Inbox${view.inbox ? ` · ${accounts.find((entry) => entry.account.id === view.inbox?.accountId)?.account.email ?? ""}` : ""}`
      : view.kind === "mailbox"
        ? `${viewAccount?.account.email ?? ""}${accountInbox ? (smartOn ? " · Smart" : " · By date") : ""}`
        : view.kind === "drafts"
          ? `${drafts.length} ${drafts.length === 1 ? "draft" : "drafts"}`
          : view.kind === "outbox"
            ? `${outbox.length} waiting`
            : smartOn
              ? "Smart"
              : "By date";
  const empty =
    view.kind === "role" && view.role === "inbox"
      ? { title: "You're all caught up", body: "New mail shows here as it arrives." }
      : view.kind === "section"
        ? view.section === "new_senders"
          ? { title: "No new senders", body: "Mail from people you haven't written to yet waits here to be accepted or blocked." }
          : { title: `Nothing in ${SECTION_LABELS[view.section]}`, body: "This part of the inbox is empty." }
      : view.kind === "marked"
        ? {
            pinned: { title: "Nothing pinned", body: "Pin a message (p) to keep it in reach here." },
            snoozed: { title: "Nothing snoozed", body: "Snooze a message (b) and it comes back as new at the time you pick." },
            reminders: {
              title: "No reminders",
              body: "Remind yourself (Shift+R) about a message you sent: if nobody replies by then, it comes back to the inbox.",
            },
            aside: { title: "Nothing set aside", body: "Set a message aside (Shift+A) to take it out of the inbox until you need it." },
          }[view.mark]
        : { title: "Nothing here", body: "This folder has no messages." };
  const folders = openItem
    ? (accounts.find((entry) => entry.account.id === openItem.accountId)?.mailboxes ?? []).filter((m) => m.selectable)
    : [];

  // The inbox's title chooses how it looks (and with several accounts, which);
  // other folders' titles choose between all accounts and one.
  const role = view.kind === "role" ? view.role : (viewMailbox?.role ?? null);
  const onTitle =
    role === "inbox"
      ? (anchor: Anchor) => setChooser(anchor)
      : accounts.length > 1 && role
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

  // The Smart Inbox around the People list.
  const openSection = (section: Section) => () =>
    changeView({ kind: "section", section, inbox: view.kind === "mailbox" && accountInbox ? { mailboxId: view.mailboxId, accountId: view.accountId } : undefined });
  const smart = smartOn
    ? {
        priority: {
          items: priorityItems,
          total: overview.priority?.total ?? priorityItems.length,
          onAll: openSection("priority"),
        },
        newSenders:
          screen === "off"
            ? null
            : {
                items: newSenderItems,
                total: overview.new_senders?.total ?? newSenderItems.length,
                held: screen === "hold",
                onAll: openSection("new_senders"),
              },
        bundles: [
          {
            key: "notifications",
            label: "Notifications",
            icon: "bell",
            count: overview.notifications?.total ?? 0,
            unread: (overview.notifications?.unread ?? 0) > 0,
            messages: notificationItems,
            onOpen: openSection("notifications"),
          },
          {
            key: "newsletters",
            label: "Newsletters",
            icon: "newspaper",
            count: overview.newsletters?.total ?? 0,
            unread: (overview.newsletters?.unread ?? 0) > 0,
            messages: newsletterItems,
            onOpen: openSection("newsletters"),
          },
          {
            key: "pinned",
            label: "Pinned",
            icon: "pin",
            count: pinnedItems.length,
            unread: pinnedItems.some((message) => !message.seen),
            messages: pinnedItems,
            onOpen: () => changeView({ kind: "marked", mark: "pinned" }),
          },
        ] satisfies Bundle[],
      }
    : null;
  // A new sender: accepted into People, or blocked (their mail goes to Junk, undoably).
  const screener: Screener = {
    accept: (item) => {
      const address = item.from?.address;
      if (address) setSender(item.accountId, address, { policy: "people", priority: false }).catch(failed);
    },
    block: (item) => {
      const address = item.from?.address;
      if (address) blockSender(item.accountId, address).then((result) => moved([item.id], result), failed);
    },
  };

  const listPane =
    view.kind === "drafts" ? (
      <section className="list-pane" aria-label="Drafts">
        <ListHeader title={title} subtitle={subtitle}>
          <button type="button" className="icon-btn" title="New message (c)" aria-label="New message" onClick={writeNew}>
            <Icon name="compose" size={17} />
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
        onBack={view.kind === "section" ? () => changeView(inboxOf(view)) : undefined}
        onCompose={writeNew}
        canCompose={composeFrom !== null}
        feed={feed}
        mode={layout.mode}
        openId={openItem?.id ?? null}
        picked={picked}
        onOpen={setOpen}
        onPick={setPicked}
        actionsFor={actionsForItems}
        onPalette={(actions, paletteTitle, about) => palette(actions, paletteTitle, about ? [senderEntry(about)] : [])}
        empty={empty}
        onSplit={layout.mode === "single" ? () => setLayout({ mode: "split" }) : undefined}
        own={ownAddresses}
        smart={smart}
        screener={screener}
        screenAll={view.kind === "section" && view.section === "new_senders"}
      />
    );

  const reader = openItem && (
    <ReadingPane
      key={openItem.id}
      item={openItem}
      folders={folders}
      mode={layout.mode}
      actionsFor={actionsFor}
      onPalette={palette}
      onSender={(anchor, messageId, person) =>
        aboutSender({ id: messageId, accountId: openItem.accountId, from: person }, anchor)
      }
      onClose={() => setOpen(null)}
      onStep={layout.mode === "single" ? step : undefined}
    />
  );

  let body;
  if (composer?.mode === "full") {
    body = null;
  } else if (layout.mode === "single") {
    body = reader && picked.size === 0 ? <div className="reader-column">{reader}</div> : listPane;
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
              <div className="empty reader-empty" data-tauri-drag-region>
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
  const framed = PLATFORM !== "linux";
  const sidebarProps = {
    mail,
    outbox,
    // A part of the inbox (Notifications, say) is still the inbox there.
    view: view.kind === "section" ? inboxOf(view) : view,
    onView: changeView,
    onSettings: () => {
      setOverlay(false);
      setSettings("accounts");
    },
    onSync: syncNow,
    syncing: busy,
    syncProblem,
    prefs,
    changes: sidebarChanges,
    onMenu: setMenu,
    onDrop: dropOn,
  };

  return (
    <div className={["app", layout.locked ? "locked" : "", framed ? "framed" : "", PLATFORM].join(" ")}>
      {banner}
      <Sidebar
        {...sidebarProps}
        expanded={layout.locked}
        locked={layout.locked}
        onToggleLock={() => setLayout({ locked: !layout.locked })}
        onExpand={() => setOverlay(true)}
      />
      <main
        className={`content ${layout.mode}`}
        ref={content}
        style={{ "--list-w": `${layout.listWidth}px` } as CSSProperties}
      >
        {framed && <div className="drag-band" data-tauri-drag-region aria-hidden="true" />}
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
            pickReminder={pickReminder}
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
        <TitleBar onAppMenu={appMenu} />
      </main>
      {overlay && !layout.locked && (
        <>
          <div className="scrim" onMouseDown={() => setOverlay(false)} />
          <Sidebar
            {...sidebarProps}
            expanded
            overlay
            locked={false}
            onToggleLock={() => {
              setOverlay(false);
              setLayout({ locked: true });
            }}
            onExpand={() => {}}
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
          screening={screen}
          onScreening={setScreen}
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
      {chooser && (
        <ViewChooser
          anchor={chooser}
          current={layout.inboxView}
          onChoose={(inboxView) => setLayout({ inboxView })}
          accounts={
            accounts.length > 1
              ? [
                  { label: "All accounts", active: inInbox, onSelect: () => changeView(INBOX) },
                  ...accounts.flatMap((entry) => {
                    const inbox = entry.mailboxes.find((m) => m.role === "inbox" && m.selectable);
                    return inbox
                      ? [
                          {
                            label: entry.account.email,
                            active: view.kind === "mailbox" && view.mailboxId === inbox.id,
                            onSelect: () => changeView({ kind: "mailbox", mailboxId: inbox.id, accountId: entry.account.id }),
                          },
                        ]
                      : [];
                  }),
                ]
              : []
          }
          onSettings={() => setSettings("inbox")}
          onClose={() => setChooser(null)}
        />
      )}
      {sender && (
        <SenderCard
          request={sender}
          onClose={() => setSenderCard(null)}
          onWrite={(person) => {
            setSenderCard(null);
            writeTo(person, sender.accountId);
          }}
          onBlocked={(result) => moved(result.moved > 0 ? [sender.messageId] : [], result)}
          onFailed={failed}
        />
      )}
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
