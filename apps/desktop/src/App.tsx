import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { type CSSProperties, type KeyboardEvent, type PointerEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  type MailboxSummary,
  type MarkResult,
  type MessageCursor,
  type MoveResult,
  type Person,
  type Role,
  type UndoInfo,
  archiveMessages,
  createFolder,
  isGmail,
  labelFolders,
  setLabels,
  deleteFolder,
  emptyFolder,
  renameFolder,
  unsubscribe,
  getMessage,
  getThread,
  locateMessage,
  markAllRead,
  moveMessages,
  pinMessages,
  remindMessages,
  setAside,
  syncAccount,
  trashMessages,
} from "./accounts";
import { type Action, type ActionHost, type Target, choose, messageActions, newSendersOf } from "./actions";
import { AddAccountDialog } from "./AddAccount";
import { HoldsDialog, usePendingHolds } from "./AgentHolds";
import { AskUpdates, useUpdateStatus } from "./UpdateCheck";
import { Avatar } from "./Avatar";
import { Composer } from "./Composer";
import {
  type Draft,
  type Dropped,
  type OutboxEntry,
  type Queued,
  type SentElsewhere,
  attachDropped,
  composerWindowOpen,
  getDraft,
  newDraft,
  onFilesDrag,
  onFilesDropped,
  openComposerWindow,
  saveDraft,
} from "./compose";
import { Confirm, type ConfirmRequest } from "./Confirm";
import { DraftsList } from "./DraftsList";
import { Icon, type IconName } from "./icons";
import { type Layout, LIST_MAX, LIST_MIN, SIDE_MAX, SIDE_MIN, clampList, clampSide, inboxViewOf, useLayout } from "./layout";
import {
  type ListItem,
  type MessageFeed,
  inboxSources,
  ALL_AT_ONCE,
  allOf,
  newer,
  sectionSources,
  sourcesFor,
  useDrafts,
  useFeed,
  useFirstPage,
  useMail,
  useMarked,
  useOutbox,
  useSearch,
  useService,
  useSmartOverview,
} from "./mail";
import { type OpenLink, onOpenLink, takeLink } from "./links";
import { recentFolders, rememberFolder } from "./recent-folders";
import { type Opened, forgetOpened, linkOf, useOpened } from "./recent-opened";
import { useRecentSearches } from "./recent-searches";
import { UNDO_ELSEWHERE, openThreadWindow } from "./thread-window";
import {
  type SavedSearch,
  listSavedSearches,
  removeSavedSearch,
  reorderSavedSearches,
  saveSearch,
  searchMessages,
  searchServer,
  searchTerm,
} from "./search";
import { openUrl } from "./background";
import { type Anchor, Menu, type MenuEntry, type MenuRequest } from "./Menu";
import { type Bundle, type InboxCard, ListHeader, MessageList, type Screener } from "./MessageList";
import { groupedIds, useSearchGroups } from "./search-groups";
import { SenderCard, type SenderRequest } from "./SenderCard";
import { SECTION_LABELS, type Screening, type Section, blockSender, screening, setSender } from "./smart";
import { ViewChooser } from "./ViewChooser";
import { OutboxList } from "./OutboxList";
import { ReadingPane } from "./ReadingPane";
import { SendToast } from "./SendToast";
import { Settings, type SettingsTab } from "./Settings";
import { AgentsMenu } from "./AgentsMenu";
import { ConnectDialog, type ConnectId } from "./ConnectDialog";
import { ShortcutHelp } from "./ShortcutHelp";
import { NameDialog, type NameRequest } from "./NameDialog";
import { useLabels } from "./labels";
import { useOnline } from "./online";
import { type Template, type Written, listTemplates, saveWritten } from "./templates";
import { type AssistantJob, AssistantDialog, useAssistantKind } from "./AssistantDialog";
import { commandHint, commandOf, firstKey, keyText, useShortcuts } from "./shortcuts";
import { type DragPayload, Sidebar } from "./Sidebar";
import { useSidebarPrefs } from "./sidebar-prefs";
import { presets, reminderPresets, sendPresets, snoozeMessages, timeHint } from "./snooze";
import { TYPED_TIME_HINT, typedTimeEntries } from "./typedTime";
import { ACCOUNT_COLOURS, COLOUR_NAMES, accountLabel, getPrefs, setPrefs, usePrefs, zoomStep } from "./prefs";
import { useChanges } from "./changes";
import { applyTheme } from "./theme";
import { TimePicker } from "./TimePicker";
import { PLATFORM, TitleBar } from "./TitleBar";
import { type UndoBar, UndoToast } from "./UndoToast";
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
  sameView,
  sortFolders,
  viewKey,
} from "./views";
import { Welcome } from "./Welcome";
import { Launch } from "./AppMark";
import { t, tn } from "./i18n";

/** How long an error notice stays. */
const NOTICE_MS = 8000;
/** Rows a card of the card view shows before "View all". */
const CARD_ROWS = 5;

const trashLike = (item: ListItem) => item.role === "trash" || item.role === "junk";

/** A search on the servers, for words in mail not opened here: asked for, page by page. */
interface ServerFound {
  /** The search it answers: scope and words. */
  key: string;
  state: "loading" | "done" | "failed";
  items: ListItem[];
  next: MessageCursor | null;
  notHere: number;
  /** Accounts whose server could not be reached. */
  unreachable: string[];
  error: string | null;
}

/** While the first results of a search are on their way. */
const SEARCHING: MessageFeed = { items: [], loading: true, error: null, more: false, loadMore: () => {} };

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
    muted: items.length > 0 && items.every((item) => item.muted === true),
    sent: items.length === 1 && items[0].role === "sent",
    aside: items.length > 0 && items.every((item) => item.aside),
    snoozed: items.length > 0 && items.every((item) => item.snoozed_until !== null),
    reminding: items.length > 0 && items.every((item) => item.remind_at !== null),
    reminded: items.some((item) => item.reminded),
    unsubscribe:
      items.length === 1 && (items[0].section === "newsletters" || items[0].section === "notifications")
        ? { from: items[0].from }
        : null,
    done: items.length > 0 && items.every((item) => item.done === true),
    // Rows stand for their conversations.
    conversation: true,
    accept: newSendersOf(items),
  };
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
  "copy",
  "junk",
  "delete",
  "reply",
  "replyAll",
  "forward",
  "again",
  "remind",
  "unsubscribe",
  "label",
  "mute",
] as const;

type Picking = { title: string; confirm: string; initial: number; resolve: (time: number | null) => void };

/** Plain text as HTML paragraphs, for a draft written with formatting. */
function paragraphs(text: string): string {
  const escape = (line: string) => line.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  return text
    .split(/\n{2,}/)
    .map((block) => `<p>${block.split("\n").map(escape).join("<br>")}</p>`)
    .join("");
}

export function App() {
  const { check, recheck } = useService();
  const mail = useMail();
  const drafts = useDrafts();
  // A send that failed, or may not have gone: said here too, since the system's
  // notification stays quiet while this window is in front.
  const [unsent, setUnsent] = useState<OutboxEntry | null>(null);
  const outbox = useOutbox(setUnsent);
  const [layout, setLayout] = useLayout();
  const [prefs, sidebarChanges] = useSidebarPrefs();
  const [overlay, setOverlay] = useState(false);

  const [view, setView] = useState<View>(INBOX);
  const [open, setOpen] = useState<ListItem | null>(null);
  // Opened by stepping through the list with the keyboard: read only if one stays on it.
  const [openedByStep, setOpenedByStep] = useState(false);
  const [picked, setPicked] = useState<Set<number>>(() => new Set());
  // The draft being written; its account and the message it answers, once known.
  const [composer, setComposer] = useState<{
    id: number;
    mode: "full" | "panel";
    accountId?: string;
    replyingTo?: number | null;
  } | null>(null);
  const [queued, setQueued] = useState<Queued | null>(null);
  // The assistant: offered once a provider is chosen in Settings; what was asked of it.
  const assistantKind = useAssistantKind();
  const [assistantJob, setAssistantJob] = useState<AssistantJob | null>(null);
  const [undo, setUndo] = useState<UndoBar | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  // Something done that has no undo, said briefly.
  const [said, setSaid] = useState<string | null>(null);
  // Said with a way to go on: "Unsubscribed · Archive their mail".
  const [offer, setOffer] = useState<{ text: string; label?: string; run?: () => void } | null>(null);
  useEffect(() => {
    if (!offer) return;
    const timer = window.setTimeout(() => setOffer(null), 12_000);
    return () => window.clearTimeout(timer);
  }, [offer]);
  const [help, setHelp] = useState(false);
  // Adding another account from the main screen, in a window over the mail.
  const [addingAccount, setAddingAccount] = useState(false);
  const [settings, setSettings] = useState<SettingsTab | null>(null);
  // The sidebar's Agents menu, beside its button.
  const [agentsMenu, setAgentsMenu] = useState<Anchor | null>(null);
  // The Connect dialog, with this agent's card open.
  const [connecting, setConnecting] = useState<ConnectId | null>(null);
  const [menu, setMenu] = useState<MenuRequest | null>(null);
  const [asking, setAsking] = useState<{ request: ConfirmRequest; resolve: (ok: boolean) => void } | null>(null);
  const [picking, setPicking] = useState<Picking | null>(null);
  const [syncing, setSyncing] = useState(false);
  const closeUndo = useCallback(() => setUndo(null), []);
  const closeQueued = useCallback(() => setQueued(null), []);

  // Searching all mail, in place of the list's header: what is typed, and in which account (null: all).
  const [search, setSearch] = useState<{ query: string; accountId: string | null } | null>(null);
  const [searchRound, setSearchRound] = useState(0);
  const [recentSearches, rememberSearch, forgetSearch] = useRecentSearches();
  const opened = useOpened();
  const typed = search?.query.trim() ?? "";
  const [searchFor, setSearchFor] = useState("");
  useEffect(() => {
    const timer = window.setTimeout(() => setSearchFor(typed), typed ? 200 : 0);
    return () => window.clearTimeout(timer);
  }, [typed]);
  const [chooser, setChooser] = useState<Anchor | null>(null);
  // What agents asked for and is waiting for the person (Settings › Agents).
  const holds = usePendingHolds();
  // What agents wait for, decided over the mail (the "Agents are waiting" bubble).
  const [holdsOpen, setHoldsOpen] = useState(false);
  // Checking for updates: asked once, and a newer version shown when one is found.
  const updates = useUpdateStatus();
  const refreshUpdates = updates.refresh;
  useEffect(() => {
    if (!settings) refreshUpdates();
  }, [settings, refreshUpdates]);
  // A hold a link asked to see, in Settings › Agents.
  const [focusHold, setFocusHold] = useState<number | null>(null);
  const [sender, setSenderCard] = useState<SenderRequest | null>(null);
  const [screen, setScreen] = useState<Screening>("off");
  useEffect(() => {
    screening().then(setScreen, () => {});
  }, []);

  const accounts = useMemo(() => (mail.state === "ready" ? mail.accounts : []), [mail]);
  const accountsNow = useRef(accounts);
  accountsNow.current = accounts;
  const ownAddresses = useMemo(() => new Set(accounts.map((entry) => entry.account.email.toLowerCase())), [accounts]);
  // The first run's sign-up stays up after the account is added, until its "Open Inbox".
  const [welcoming, setWelcoming] = useState(false);
  const noAccounts = mail.state === "ready" && accounts.length === 0;
  useEffect(() => {
    if (noAccounts) setWelcoming(true);
  }, [noAccounts]);
  const appPrefs = usePrefs();
  // Settings › Accessibility: the whole window larger or smaller.
  useEffect(() => {
    document.documentElement.style.setProperty("zoom", appPrefs.zoom === 1 ? "" : String(appPrefs.zoom));
  }, [appPrefs.zoom]);
  // Zoom from anywhere, a field included: Ctrl + / Ctrl − / Ctrl 0 (Cmd on a Mac), or the keys chosen instead.
  useEffect(() => {
    const keys = (event: globalThis.KeyboardEvent) => {
      const command = commandOf(event, ["zoomIn", "zoomOut", "zoomReset"]);
      if (command === null) return;
      const now = getPrefs().zoom;
      const zoom = command === "zoomIn" ? zoomStep(now, 1) : command === "zoomOut" ? zoomStep(now, -1) : 1;
      event.preventDefault();
      setPrefs({ zoom });
      setSaid(`Zoom ${Math.round(zoom * 100)} %`);
    };
    window.addEventListener("keydown", keys);
    return () => window.removeEventListener("keydown", keys);
  }, []);
  // Accounts the person keeps out of the unified inbox (Settings › Inbox › Email groups).
  const hiddenKey = appPrefs.hiddenAccounts.join(",");
  const unified = useMemo(
    () => accounts.filter((entry) => !hiddenKey.split(",").includes(entry.account.id)),
    [accounts, hiddenKey],
  );
  // "Mark everything read": the folders (and Smart Inbox section) the list shows; none for Pinned, Drafts and the like.
  const readTargets = (): { mailboxId: number; section: Section | null }[] => {
    const inboxesOf = (entries: typeof accounts, role: Role) =>
      entries.flatMap((entry) => entry.mailboxes.filter((m) => m.role === role).map((m) => m.id));
    if (view.kind === "mailbox") return [{ mailboxId: view.mailboxId, section: null }];
    if (view.kind === "role") return inboxesOf(view.role === "inbox" ? unified : accounts, view.role).map((mailboxId) => ({ mailboxId, section: null }));
    if (view.kind === "section") {
      const ids = view.inbox ? [view.inbox.mailboxId] : inboxesOf(unified, "inbox");
      return ids.map((mailboxId) => ({ mailboxId, section: view.section }));
    }
    return [];
  };
  const markAll = (targets: { mailboxId: number; section: Section | null }[]) => {
    Promise.all(targets.map((target) => markAllRead(target.mailboxId, target.section)))
      .then((results) => {
        const changed = results.reduce((sum, result) => sum + result.changed, 0);
        const undos = results.flatMap((result) => (result.undo ? [result.undo] : []));
        if (changed === 0 || undos.length === 0) {
          setSaid(t("Nothing unread here"));
          return;
        }
        setUndo({
          id: undos[0].id,
          description: undos.length === 1 ? undos[0].description : tn(changed, "Marked 1 message read", "Marked {count} messages read"),
          also: undos.slice(1).map((one) => one.id),
        });
      })
      .catch(failed);
  };
  const markAllHere = () => markAll(readTargets());
  // Accounts whose Microsoft or Google sign-in expired: nothing syncs or sends until they sign in again.
  const signedOut = accounts.filter((entry) => entry.account.needs_sign_in);
  const inInbox = view.kind === "role" && view.role === "inbox";
  // The Smart Inbox: every inbox, or one account's when its inbox is open.
  const accountInbox =
    view.kind === "mailbox"
      ? accounts.find(
          (entry) =>
            entry.account.id === view.accountId && entry.mailboxes.some((m) => m.id === view.mailboxId && m.role === "inbox"),
        )
      : undefined;
  // With several accounts, one account's inbox can look its own way.
  const ownViewOf = accounts.length > 1 && accountInbox ? accountInbox.account.id : null;
  const inboxView = inboxViewOf(layout, ownViewOf);
  const smartScope = useMemo(
    () => (inboxView !== "smart" ? null : inInbox ? unified : accountInbox ? [accountInbox] : null),
    [inboxView, inInbox, unified, accountInbox],
  );
  const smartOn = smartScope !== null && search === null;
  // Show Done (Ctrl+E): the inbox by date with its Done mail mixed in, greyed.
  const [showDone, setShowDone] = useState(false);
  const inboxByDate = (inInbox || accountInbox !== null) && inboxView === "date" && search === null;
  const doneShown = showDone && inboxByDate;
  // The inbox as cards: unread mail by kind, then what has been read.
  const cardScope = useMemo(
    () => (inboxView !== "cards" ? null : inInbox ? unified : accountInbox ? [accountInbox] : null),
    [inboxView, inInbox, unified, accountInbox],
  );
  const cardsOn = cardScope !== null && search === null;
  const found = useSearch(accounts, search ? searchFor : "", search?.accountId ?? null, searchRound);
  // The Smart Inbox's list: People, and any group the person chose to see ungrouped.
  const ungroupNotifications = !appPrefs.grouped.notifications;
  const ungroupNewsletters = !appPrefs.grouped.newsletters;
  const sources = useMemo(
    () =>
      found.sources ??
      (smartScope
        ? [
            ...sectionSources(smartScope, "people"),
            ...(ungroupNotifications ? sectionSources(smartScope, "notifications") : []),
            ...(ungroupNewsletters ? sectionSources(smartScope, "newsletters") : []),
          ]
        : cardScope
          ? inboxSources(cardScope, true)
          : sourcesFor(view.kind === "role" ? unified : accounts, view, doneShown)),
    [found.sources, smartScope, cardScope, accounts, unified, view, ungroupNotifications, ungroupNewsletters, doneShown],
  );
  const listed = useFeed(sources);
  // The last results stay while the next ones come, so typing doesn't flash the list.
  const lastFound = useRef<MessageFeed | null>(null);
  if (!found.sources) lastFound.current = null;
  else if (!listed.loading) lastFound.current = listed;
  const local: MessageFeed =
    typed && !found.sources ? SEARCHING : found.sources && listed.loading && lastFound.current ? lastFound.current : listed;

  // The servers' matches join the results, once each.
  const serverKey = search && searchFor && found.sources ? `${search.accountId ?? ""}:${searchFor}` : "";
  const [server, setServer] = useState<ServerFound | null>(null);
  const serverNow = server && server.key === serverKey ? server : null;
  const feed = useMemo((): MessageFeed => {
    if (!serverNow || serverNow.items.length === 0) return local;
    const shown = new Set(local.items.map((item) => item.id));
    const extra = serverNow.items.filter((item) => !shown.has(item.id));
    return extra.length === 0 ? local : { ...local, items: [...local.items, ...extra].sort(newer) };
  }, [local, serverNow]);
  // By category, Priority keeps its mail once read: the Seen card leaves it out.
  const shownFeed = useMemo(
    (): MessageFeed => (cardsOn ? { ...feed, items: feed.items.filter((item) => item.section !== "priority") } : feed),
    [cardsOn, feed],
  );
  const placeOf = useMemo(() => {
    const places = new Map<number, { accountId: string; role: ListItem["role"] }>();
    for (const entry of accounts) {
      for (const mailbox of entry.mailboxes) places.set(mailbox.id, { accountId: entry.account.id, role: mailbox.role });
    }
    return places;
  }, [accounts]);
  const askServer = () => {
    if (!search || !serverKey) return;
    const key = serverKey;
    const previous = serverNow?.state === "done" || serverNow?.state === "failed" ? serverNow : null;
    const before = previous?.state === "done" ? previous.next : null;
    const kept = before ? (previous?.items ?? []) : [];
    setServer({ key, state: "loading", items: kept, next: null, notHere: previous?.notHere ?? 0, unreachable: [], error: null });
    searchServer(searchFor, { accountId: search.accountId, before }).then(
      (result) => {
        const items = result.messages.flatMap((message): ListItem[] => {
          const place = placeOf.get(message.mailbox_id);
          return place ? [{ ...message, mailboxId: message.mailbox_id, ...place }] : [];
        });
        setServer((now) =>
          now?.key !== key
            ? now
            : {
                key,
                state: "done",
                items: [...kept, ...items],
                next: result.next,
                notHere: result.not_here,
                unreachable: result.unreachable,
                error: null,
              },
        );
      },
      (error: unknown) => setServer((now) => (now?.key !== key ? now : { ...now, state: "failed", error: String(error) })),
    );
  };
  const scope = useMemo(() => smartScope ?? [], [smartScope]);
  const overview = useSmartOverview(scope, smartOn);
  const priorityItems = useFirstPage(
    useMemo(() => sectionSources(scope, "priority"), [scope]),
    appPrefs.priorityRows > 0 ? appPrefs.priorityRows : 50,
    smartOn,
  );
  const newSenderItems = useFirstPage(
    useMemo(() => sectionSources(scope, "new_senders"), [scope]),
    3,
    smartOn && screen === "section",
  );
  const notificationItems = useFirstPage(useMemo(() => sectionSources(scope, "notifications"), [scope]), 30, smartOn);
  const newsletterItems = useFirstPage(useMemo(() => sectionSources(scope, "newsletters"), [scope]), 30, smartOn);
  const pinnedItems = useMarked(scope, "pinned", smartOn);
  const invitationItems = useMarked(scope, "invitations", smartOn && appPrefs.grouped.invitations);
  // The card view's cards: the newest unread of each kind, and the pinned.
  const cardList = useMemo(() => cardScope ?? [], [cardScope]);
  const cardCounts = useSmartOverview(cardList, cardsOn);
  const cardNewSenders = useFirstPage(
    useMemo(() => sectionSources(cardList, "new_senders", false), [cardList]),
    CARD_ROWS,
    cardsOn && screen === "section",
  );
  // Priority keeps its mail once read; the other cards hold only unread mail.
  const cardPriority = useFirstPage(useMemo(() => sectionSources(cardList, "priority"), [cardList]), CARD_ROWS, cardsOn);
  const cardPeople = useFirstPage(useMemo(() => sectionSources(cardList, "people", false), [cardList]), CARD_ROWS, cardsOn);
  const cardNotifications = useFirstPage(useMemo(() => sectionSources(cardList, "notifications", false), [cardList]), CARD_ROWS, cardsOn);
  const cardNewsletters = useFirstPage(useMemo(() => sectionSources(cardList, "newsletters", false), [cardList]), CARD_ROWS, cardsOn);
  const cardPinned = useMarked(cardList, "pinned", cardsOn);
  const cardPinnedRows = useFirstPage(
    useMemo(() => sourcesFor(cardList, { kind: "marked", mark: "pinned" }), [cardList]),
    CARD_ROWS,
    cardsOn,
  );
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
  useEffect(() => {
    if (!said) return;
    const timer = window.setTimeout(() => setSaid(null), NOTICE_MS / 2);
    return () => window.clearTimeout(timer);
  }, [said]);

  const failed = useCallback((error: unknown) => setNotice(String(error)), []);
  // Shown while it is still failed: sent again or edited, it goes.
  const unsentNow = unsent && outbox.find((entry) => entry.id === unsent.id && (entry.state === "failed" || entry.state === "uncertain"));
  /** Replies and forwards open in a panel over the conversation; new messages take the whole pane. */
  const startDraft = useCallback(
    (start: Promise<Draft>, mode: "full" | "panel") => {
      start
        .then((draft) => setComposer({ id: draft.id, mode, accountId: draft.account_id, replyingTo: draft.replying_to }))
        .catch(failed);
    },
    [failed],
  );
  // Settings › Composer: replies and forwards open over the conversation, or full size.
  const compose = useCallback((start: Promise<Draft>) => startDraft(start, getPrefs().replyOpens), [startDraft]);
  const editDraft = useCallback((id: number) => {
    // Open in a window of its own already: that window comes to the front.
    void composerWindowOpen(id).then(
      (elsewhere) => {
        if (elsewhere) {
          setComposer((now) => (now?.id === id ? null : now));
          void openComposerWindow(id, "");
        }
      },
      () => {},
    );
    setComposer({ id, mode: "full" });
    getDraft(id).then(
      (draft) =>
        setComposer((now) =>
          now?.id === id ? { ...now, accountId: draft.account_id, replyingTo: draft.replying_to } : now,
        ),
      () => {},
    );
  }, []);
  /**
   * A message has gone to the Outbox: remember its account, and with Send and
   * Done (`done`) or doneAfterReply, file the conversation it answers.
   */
  const sentFrom = (sent: { accountId?: string; replyingTo?: number | null } | null, done = false) => {
    if (sent?.accountId) setPrefs({ lastFrom: sent.accountId });
    if (!sent?.replyingTo || !(done || getPrefs().doneAfterReply)) return;
    getThread(sent.replyingTo)
      .then(({ messages }) => {
        const ids = messages.filter((entry) => entry.mailbox_role === "inbox").map((entry) => entry.message.id);
        if (ids.length === 0) return;
        return archiveMessages(ids).then((result) => moved(ids, result));
      })
      .catch(failed);
  };
  const confirm = useCallback(
    (request: ConfirmRequest) => new Promise<boolean>((resolve) => setAsking({ request, resolve })),
    [],
  );
  // A name asked for: a new folder's, or a folder's new one.
  const [naming, setNaming] = useState<NameRequest | null>(null);
  const folderOf = useCallback(
    (accountId: string, role: string) =>
      accounts.find((entry) => entry.account.id === accountId)?.mailboxes.find((m) => m.role === role && m.selectable)?.id ??
      null,
    [accounts],
  );
  /**
   * Move to (or Copy to): a page of the palette listing the account's folders,
   * those used last at the top until something is typed.
   */
  const pickFolder = useCallback(
    (accountId: string, current: number | null, _anchor?: Anchor | null, purpose: "move" | "copy" = "move") =>
      new Promise<number | null>((resolve) => {
        const folders = (accounts.find((entry) => entry.account.id === accountId)?.mailboxes ?? []).filter(
          (folder) => folder.selectable && folder.id !== current,
        );
        const pick = (id: number) => {
          rememberFolder(accountId, id);
          resolve(id);
        };
        const icon = (folder: MailboxSummary) => (folder.role ? ROLE_ICONS[folder.role] : "folder");
        const recent = recentFolders(accountId).flatMap((id) => folders.filter((folder) => folder.id === id));
        setMenu({
          anchor: { x: 0, y: 0 },
          variant: "palette",
          title: purpose === "copy" ? t("Copy to") : t("Move to"),
          label: purpose === "copy" ? t("Copy to folder") : t("Move to folder"),
          filter: t("Type a folder name"),
          entries: [
            ...recent.map((folder) => ({
              label: leafName(folder),
              detail: t("Recent"),
              icon: icon(folder),
              quick: true,
              onSelect: () => pick(folder.id),
            })),
            ...(recent.length > 0 ? ["separator" as const] : []),
            ...sortFolders(folders).map((folder) => ({
              label: leafName(folder),
              icon: icon(folder),
              depth: depth(folder),
              onSelect: () => pick(folder.id),
            })),
          ],
          onDismiss: () => resolve(null),
        });
      }),
    [accounts],
  );
  // Messages that left the list: the next one opens in place of the one that went.
  const moved = useCallback((ids: number[], result: MoveResult | null) => {
    const gone = new Set(ids);
    setPicked(new Set());
    // The servers' matches are not kept current: what left goes from them here.
    setServer((now) => (now ? { ...now, items: now.items.filter((item) => !gone.has(item.id)) } : now));
    setOpen((current) => {
      if (!current || !gone.has(current.id)) return current;
      const list = items.current;
      const index = list.findIndex((item) => item.id === current.id);
      const afterMove = getPrefs().afterMove;
      if (afterMove === "list") return null;
      if (index === -1) {
        const near = around.current?.id === current.id ? around.current : null;
        const order = afterMove === "previous" ? [near?.before, near?.after] : [near?.after, near?.before];
        return order.find((item) => item && !gone.has(item.id)) ?? null;
      }
      const after = list.slice(index + 1).find((item) => !gone.has(item.id));
      const before = list
        .slice(0, index)
        .reverse()
        .find((item) => !gone.has(item.id));
      return afterMove === "previous" ? (before ?? after ?? null) : (after ?? before ?? null);
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
          filter: TYPED_TIME_HINT,
          typed: (query) => typedTimeEntries(query, resolve),
          entries: [
            ...choices.map((choice) => ({
              label: choice.label,
              icon: "clock" as const,
              hint: timeHint(choice.until),
              onSelect: () => resolve(choice.until),
            })),
            "separator" as const,
            {
              label: t("Pick a date and time…"),
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
  const pickSnooze = useCallback(() => pickTime(t("Snooze until"), t("Snooze"), presets(new Date(), getPrefs().snoozePresets)), [pickTime]);
  const pickReminder = useCallback(
    () =>
      pickTime(t("Remind me if no reply by"), t("Remind me"), reminderPresets(new Date(), getPrefs().reminderPresets)).then((time) =>
        typeof time === "number" ? time : null,
      ),
    [pickTime],
  );
  /**
   * A palette page of templates to start from, by name, and a way to keep
   * what is written as one (named after its subject; renamed in Settings).
   */
  const pickTemplate = useCallback(
    (_anchor: Anchor, written: Written) =>
      listTemplates().then(
        (templates) =>
          new Promise<Template | null>((resolve) => {
            const hasText = written.html.replace(/<[^>]*>/g, "").trim() !== "";
            const name = written.subject.trim() || "Untitled";
            const keep: MenuEntry[] = hasText
              ? [
                  {
                    label: t("Save this message as a template"),
                    icon: "plus",
                    detail: name,
                    onSelect: () => {
                      resolve(null);
                      saveWritten(name, written).then(
                        (saved) => setSaid(t("Saved as the template “{name}”", { name: saved.name })),
                        failed,
                      );
                    },
                  },
                ]
              : [];
            setMenu({
              anchor: { x: 0, y: 0 },
              variant: "palette",
              title: t("Templates"),
              label: t("Insert a template"),
              filter: t("Type a template's name"),
              entries: [
                ...templates.map(
                  (template): MenuEntry => ({
                    label: template.name,
                    icon: "text",
                    detail: template.subject || undefined,
                    onSelect: () => resolve(template),
                  }),
                ),
                ...(templates.length > 0 ? ["separator" as const] : []),
                ...keep,
                {
                  label: templates.length > 0 ? t("Edit templates…") : t("Write a template…"),
                  icon: "settings",
                  onSelect: () => {
                    resolve(null);
                    setSettings("templates");
                  },
                },
              ],
              onDismiss: () => resolve(null),
            });
          }),
      ),
    [failed],
  );
  const pickSendTime = useCallback(
    () => pickTime(t("Send at"), t("Schedule"), sendPresets(new Date(), getPrefs().sendPresets)).then((time) => (typeof time === "number" ? time : null)),
    [pickTime],
  );
  const junkFolder = useCallback((accountId: string) => folderOf(accountId, "junk"), [folderOf]);
  const inboxFolder = useCallback((accountId: string) => folderOf(accountId, "inbox"), [folderOf]);

  /** Everything from an address still in the inboxes, archived with one undo (after unsubscribing). */
  const archiveFrom = useCallback(
    async (address: string) => {
      const ids: number[] = [];
      let before: MessageCursor | null = null;
      do {
        const page = await searchMessages(`${searchTerm("from", address)} in:inbox`, { before, limit: 200 });
        ids.push(...page.messages.map((message) => message.id));
        before = page.next;
      } while (before && ids.length < ALL_AT_ONCE);
      if (ids.length === 0) return;
      moved(ids, await archiveMessages(ids.slice(0, ALL_AT_ONCE)));
    },
    [moved],
  );
  // Leaving a mailing list (#202) acts outside the app: asked first, naming the sender.
  const leaveList = useCallback(
    (id: number, from: Person | null) => {
      const who = from?.name || from?.address || "this sender";
      void confirm({
        title: t("Unsubscribe from {who}?", { who }),
        body: t("Regards asks them to stop sending to you: in one click where they take it, otherwise by an email (which you can undo) or on their own page."),
        confirm: t("Unsubscribe"),
      })
        .then(async (ok) => {
          if (!ok) return;
          const result = await unsubscribe(id, getPrefs().undoSeconds);
          if (result.kind === "sent") {
            setQueued(result.queued);
          } else if (result.kind === "web") {
            await openUrl(result.url);
            setSaid(t("Finish on {who}'s page, in your browser", { who }));
          } else {
            const address = from?.address;
            setOffer(
              address
                ? { text: t("Unsubscribed from {who}", { who }), label: t("Archive their mail"), run: () => void archiveFrom(address).catch(failed) }
                : { text: t("Unsubscribed from {who}", { who }) },
            );
          }
        })
        .catch(failed);
    },
    [confirm, failed, archiveFrom],
  );
  // Gmail labels (ADR-0009 §6): each Gmail account's label folders, and the labels of what shows.
  const gmailLabels = useMemo(
    () => new Map(accounts.filter((entry) => isGmail(entry.account)).map((entry) => [entry.account.id, labelFolders(entry.mailboxes)])),
    [accounts],
  );
  const labelIds = useMemo(
    () =>
      [...feed.items, ...(openItem ? [openItem] : [])]
        .filter((item) => gmailLabels.has(item.accountId))
        .map((item) => item.id),
    [feed.items, openItem, gmailLabels],
  );
  const shownLabels = useLabels(labelIds);
  const labelsOf = (item: ListItem) =>
    (shownLabels.labels.get(item.id) ?? []).flatMap((id) => {
      const folder = gmailLabels.get(item.accountId)?.find((mailbox) => mailbox.id === id);
      return folder ? [{ id, name: leafName(folder) }] : [];
    });
  // The picker reads these when opened: the host stays the same as labels change.
  const labelState = useRef({ folders: gmailLabels, labels: shownLabels.labels, update: shownLabels.update });
  labelState.current = { folders: gmailLabels, labels: shownLabels.labels, update: shownLabels.update };
  const hasLabels = useCallback((accountId: string) => labelState.current.folders.has(accountId), []);
  const pickLabels = useCallback(
    (ids: number[], accountId: string) => {
      const { folders, labels } = labelState.current;
      const have = (label: number) => ids.every((id) => (labels.get(id) ?? []).includes(label));
      const change = (add: number[], remove: number[]) =>
        setLabels(ids, add, remove)
          .then((result) => {
            labelState.current.update(result.messages);
            if (result.gone.length > 0) moved(result.gone, null);
          })
          .catch(failed);
      setMenu({
        anchor: { x: 0, y: 0 },
        variant: "palette",
        title: ids.length === 1 ? t("Labels") : t("Labels of {count} messages", { count: ids.length }),
        label: t("Labels"),
        filter: t("Find a label"),
        entries: [
          ...(folders.get(accountId) ?? []).map((folder) => ({
            label: folder.delimiter ? folder.name.split(folder.delimiter).join(" › ") : folder.name,
            icon: have(folder.id) ? ("check" as const) : ("tag" as const),
            onSelect: () => void (have(folder.id) ? change([], [folder.id]) : change([folder.id], [])),
          })),
          "separator",
          {
            label: t("New label…"),
            icon: "plus",
            onSelect: () =>
              setNaming({
                title: t("New label"),
                label: t("Label name"),
                value: "",
                confirm: t("Create"),
                onSave: (name) => createFolder(accountId, null, name).then((made) => change([made.id], [])),
              }),
          },
        ],
      });
    },
    [moved, failed],
  );
  // Copies go through the server: say where, as they show once that folder syncs.
  const copied = useCallback(
    (count: number, mailboxId: number) => {
      const folder = accounts.flatMap((entry) => entry.mailboxes).find((mailbox) => mailbox.id === mailboxId);
      const name = folder ? leafName(folder) : t("the folder");
      setNotice(count === 0 ? t("Already in {folder}", { folder: name }) : tn(count, "Copied to {folder}", "{count} messages copied to {folder}", { folder: name }));
    },
    [accounts],
  );
  const host = useMemo<ActionHost>(
    () => ({
      compose,
      moved,
      marked,
      failed,
      confirm,
      pickFolder,
      copied,
      pickSnooze,
      pickReminder,
      junkFolder,
      inboxFolder,
      unsubscribe: leaveList,
      hasLabels,
      pickLabels,
    }),
    [compose, moved, marked, failed, confirm, pickFolder, copied, pickSnooze, pickReminder, junkFolder, inboxFolder, leaveList, hasLabels, pickLabels],
  );
  const actionsForItems = useCallback((list: ListItem[]) => messageActions(targetOf(list), host), [host]);
  const actionsFor = useCallback((target: Target) => messageActions(target, host), [host]);

  const changeView = useCallback((next: View) => {
    setView(next);
    setSearch(null);
    setOpen(null);
    setPicked(new Set());
    setOverlay(false);
  }, []);

  /** An account's own inbox, once its folders are known; every inbox until then. */
  const openInboxOf = (accountId: string) => {
    const entry = accountsNow.current.find((one) => one.account.id === accountId);
    const inbox = entry?.mailboxes.find((mailbox) => mailbox.role === "inbox");
    changeView(inbox ? { kind: "mailbox", mailboxId: inbox.id, accountId } : INBOX);
  };

  // A regards:// link (agents give them, so the person can see what they found or did),
  // or a mailto: one once Regards is the mail app. Any web page can open a link, so it
  // only ever opens a view, or a new message to look over: nothing is sent.
  const openLink = (link: OpenLink) => {
    setOverlay(false);
    if (link.kind === "compose") {
      setSettings(null);
      writeFromLink(link);
      return;
    }
    if (link.kind === "hold") {
      setFocusHold(link.id);
      setSettings("agents");
      return;
    }
    setSettings(null);
    if (link.kind === "draft") {
      getDraft(link.id).then(
        (draft) => setComposer({ id: draft.id, mode: "full" }),
        () => setNotice(t("That draft is no longer here.")),
      );
      return;
    }
    if (link.kind === "message_id") {
      locateMessage(link.message_id).then(
        (id) => (id === null ? setNotice(t("That message is no longer on this computer.")) : openLink({ kind: "message", id })),
        () => setNotice(t("That message is no longer on this computer.")),
      );
      return;
    }
    getMessage(link.id)
      .then((detail) => {
        const owner = accounts.find((entry) => entry.mailboxes.some((m) => m.id === detail.mailbox_id));
        const mailbox = owner?.mailboxes.find((m) => m.id === detail.mailbox_id);
        if (!owner || !mailbox) throw new Error("not here");
        // Its folder, with it open.
        changeView({ kind: "mailbox", mailboxId: mailbox.id, accountId: owner.account.id });
        setOpen({ ...detail.message, mailboxId: mailbox.id, accountId: owner.account.id, role: mailbox.role });
      })
      .catch(() => setNotice(t("That message is no longer on this computer.")));
  };
  const linkTo = useRef(openLink);
  linkTo.current = openLink;
  // Recently opened, from the search page: the message wherever it is now.
  const openRecent = (entry: Opened) => {
    setSearch(null);
    setPicked(new Set());
    const gone = () => {
      forgetOpened(entry.link);
      setNotice(t("That message is no longer on this computer."));
    };
    const link = linkOf(entry.link);
    if (!link) gone();
    else if (link.kind === "message_id") {
      locateMessage(link.message_id).then((id) => (id === null ? gone() : openLink({ kind: "message", id })), gone);
    } else openLink(link);
  };
  useEffect(() => {
    const stop = onOpenLink((link) => linkTo.current(link));
    return () => void stop.then((unlisten) => unlisten()).catch(() => {});
  }, []);
  // The link Regards was started with: once its accounts are known.
  const tookLink = useRef(false);
  const mailReady = mail.state === "ready";
  useEffect(() => {
    if (!mailReady || tookLink.current) return;
    tookLink.current = true;
    takeLink().then((link) => link && linkTo.current(link), () => {});
  }, [mailReady]);

  // "last": the account the last message went from, while it is still here.
  const wantedFrom = appPrefs.defaultFrom === "last" ? appPrefs.lastFrom : appPrefs.defaultFrom;
  const defaultFrom = accounts.some((entry) => entry.account.id === wantedFrom) ? wantedFrom : null;
  const composeFrom =
    openItem?.accountId ?? (view.kind === "mailbox" ? view.accountId : (defaultFrom ?? accounts[0]?.account.id)) ?? null;

  // Files dropped on the window while no message is being written: a new message
  // with them (the composer takes its own drops). Paths never reach this side.
  const [filesOver, setFilesOver] = useState(false);
  const canDrop = !composer && composeFrom !== null && !settings;
  const dropped = useRef<(drop: Dropped) => void>(() => {});
  dropped.current = (drop) => {
    setFilesOver(false);
    if (!canDrop || !composeFrom) return;
    newDraft(composeFrom)
      .then((draft) =>
        attachDropped(draft.id, drop.id).then((result) => {
          setComposer({ id: draft.id, mode: "full" });
          if (result.refused.length > 0) {
            setNotice(t("Not attached: {files}", { files: result.refused.map((file) => `${file.name} (${file.error})`).join(", ") }));
          }
        }),
      )
      .catch(failed);
  };
  // A message sent from a window of its own: its undo is shown here.
  const sentElsewhere = useRef<(sent: SentElsewhere) => void>(() => {});
  sentElsewhere.current = (sent) => {
    sentFrom(sent, sent.queued.done);
    setQueued(sent.queued);
  };
  useEffect(() => {
    const stop = listen<SentElsewhere>("composer_queued", (event) => sentElsewhere.current(event.payload));
    return () => void stop.then((unlisten) => unlisten()).catch(() => {});
  }, []);
  // What was done in a conversation's own window: its undo shows here.
  useEffect(() => {
    const stop = listen<UndoInfo>(UNDO_ELSEWHERE, (event) => setUndo(event.payload));
    return () => void stop.then((unlisten) => unlisten()).catch(() => {});
  }, []);

  useEffect(() => {
    const stops = [onFilesDrag(setFilesOver), onFilesDropped((drop) => dropped.current(drop))];
    return () => {
      for (const stop of stops) void stop.then((unlisten) => unlisten()).catch(() => {});
    };
  }, []);
  const writeNew = () => {
    if (composeFrom) startDraft(newDraft(composeFrom), "full");
  };
  /**
   * A new message from a mailto: link, from the default account. The draft keeps what
   * the daemon put in it (an automatic copy, a signature); the link's fields join them.
   */
  const writeFromLink = (link: Extract<OpenLink, { kind: "compose" }>) => {
    const from = defaultFrom ?? accounts[0]?.account.id;
    if (!from) {
      setNotice(t("Add an account first: then email links open a new message here."));
      return;
    }
    const people = (addresses: string[]): Person[] => addresses.map((address) => ({ name: null, address }));
    const join = (had: Person[], more: string[]) => [
      ...had,
      ...people(more).filter((person) => !had.some((one) => one.address?.toLowerCase() === person.address?.toLowerCase())),
    ];
    startDraft(
      newDraft(from).then((draft) =>
        saveDraft({
          id: draft.id,
          to: join(draft.to, link.to),
          cc: join(draft.cc, link.cc),
          bcc: join(draft.bcc, link.bcc),
          subject: link.subject || draft.subject,
          body: link.body ? (draft.body ? `${link.body}\n\n${draft.body}` : link.body) : draft.body,
          html: draft.html && link.body ? `${paragraphs(link.body)}${draft.html}` : draft.html,
        }),
      ),
      "full",
    );
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
    label: t("The sender, and why this is here…"),
    icon: "info",
    onSelect: () => aboutSender(about),
  });

  // The folders being shown, synced now.
  const viewAccounts = [...new Set(sources.map((source) => source.accountId))];
  const syncAccounts = viewAccounts.length > 0 ? viewAccounts : accounts.map((entry) => entry.account.id);
  const busy = syncing || accounts.some((entry) => entry.sync?.running);
  const syncProblem =
    accounts
      .map((entry) => entry.error ?? entry.sync?.last_error ?? (entry.sync?.push_error && t("Live updates stopped: {problem}", { problem: entry.sync.push_error })))
      .find(Boolean) ?? null;
  const syncNow = () => {
    setSyncing(true);
    Promise.all(syncAccounts.map((id) => syncAccount(id)))
      .catch(failed)
      .finally(() => setSyncing(false));
  };

  // Back online: check for mail at once, so connections and sends waiting to
  // try again don't sit out their delay. Quietly: the network may still flap.
  const online = useOnline();
  const wasOnline = useRef(online);
  useEffect(() => {
    if (online && !wasOnline.current) {
      void Promise.allSettled(accounts.map((entry) => syncAccount(entry.account.id)));
    }
    wasOnline.current = online;
  }, [online]);

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

  // Folders (#198): made, renamed and deleted on the server; Trash and Junk emptied.
  const mailboxOf = (target: View) =>
    target.kind === "mailbox"
      ? (accounts.find((entry) => entry.account.id === target.accountId)?.mailboxes.find((m) => m.id === target.mailboxId) ?? null)
      : null;
  const howMany = (count: number) =>
    count >= ALL_AT_ONCE ? t("More than {count} messages", { count: ALL_AT_ONCE }) : tn(count, "1 message", "{count} messages");
  /** A new folder in an account, inside `parent` or at its top; opened once made. */
  const newFolder = (accountId: string, parent: MailboxSummary | null) =>
    setNaming({
      title: parent ? t("New folder in {folder}", { folder: leafName(parent) }) : t("New folder"),
      label: t("Folder name"),
      value: "",
      confirm: t("Create"),
      onSave: (name) =>
        createFolder(accountId, parent?.id ?? null, name).then((made) => changeView({ kind: "mailbox", mailboxId: made.id, accountId })),
    });
  /** Ctrl+Shift+U: a new folder where one is: inside the open folder, else at the top of its account. */
  const newFolderHere = () => {
    const accountId = view.kind === "mailbox" ? view.accountId : accounts[0]?.account.id;
    const here = mailboxOf(view);
    if (accountId) newFolder(accountId, here && !here.role ? here : null);
  };
  /** The folders inside `folder`, at any depth. */
  const foldersIn = (accountId: string, folder: MailboxSummary) => {
    const delimiter = folder.delimiter;
    if (!delimiter) return [];
    const mailboxes = accounts.find((entry) => entry.account.id === accountId)?.mailboxes ?? [];
    return mailboxes.filter((m) => m.name.startsWith(folder.name + delimiter));
  };
  const deleteFolderAsked = (accountId: string, folder: MailboxSummary) => {
    const inside = foldersIn(accountId, folder);
    const gone = [folder, ...inside];
    const views = gone.map((m): View => ({ kind: "mailbox", mailboxId: m.id, accountId }));
    // Counted first, so the question says how much mail goes.
    allOf(views.flatMap((target) => sourcesFor(accounts, target)))
      .then(async (list) => {
        const mail = list.length === 0 ? t("no mail") : howMany(list.length).toLowerCase();
        const ok = await confirm({
          title: t("Delete “{folder}”?", { folder: leafName(folder) }),
          body:
            inside.length === 0
              ? t("“{folder}”, with {mail}, is deleted on the server. This cannot be undone.", { folder: leafName(folder), mail })
              : tn(
                  inside.length,
                  "“{folder}” and the folder inside it, with {mail}, are deleted on the server. This cannot be undone.",
                  "“{folder}” and the {count} folders inside it, with {mail}, are deleted on the server. This cannot be undone.",
                  { folder: leafName(folder), mail },
                ),
          confirm: t("Delete folder"),
          danger: true,
        });
        if (!ok) return;
        await deleteFolder(folder.id);
        for (const target of views) {
          sidebarChanges.unstar(target);
          sidebarChanges.forget(target);
        }
        if (views.some((target) => sameView(target, view))) changeView(INBOX);
      })
      .catch(failed);
  };
  /** The Trash or Junk shown, of one account or all: what it holds deleted for good, after asking. */
  const emptyRole = (target: View): "trash" | "junk" | null => {
    const role = target.kind === "role" ? target.role : (mailboxOf(target)?.role ?? null);
    return role === "trash" || role === "junk" ? role : null;
  };
  const emptyHere = (target: View) => {
    const role = emptyRole(target);
    if (!role) return;
    const label = ROLE_LABELS[role];
    const sources = sourcesFor(accounts, target);
    allOf(sources)
      .then(async (list) => {
        if (list.length === 0) {
          setSaid(t("{folder} is empty", { folder: label }));
          return;
        }
        const ok = await confirm({
          title: t("Empty {folder}?", { folder: label }),
          body:
            list.length >= ALL_AT_ONCE
              ? t("More than {count} messages are deleted for good on the server. This cannot be undone.", { count: ALL_AT_ONCE })
              : tn(
                  list.length,
                  "1 message is deleted for good on the server. This cannot be undone.",
                  "{count} messages are deleted for good on the server. This cannot be undone.",
                ),
          confirm: t("Empty {folder}", { folder: label }),
          danger: true,
        });
        if (!ok) return;
        const results = await Promise.all(sources.flatMap((source) => (source.mailboxId === null ? [] : [emptyFolder(source.mailboxId)])));
        moved(
          list.map((item) => item.id),
          null,
        );
        const deleted = results.reduce((sum, result) => sum + result.deleted, 0);
        setSaid(tn(deleted, "Deleted 1 message for good", "Deleted {count} messages for good"));
      })
      .catch(failed);
  };
  /** A folder's own menu: in the sidebar and the folder panel. */
  const folderMenu = (target: View): MenuEntry[] => {
    const role = emptyRole(target);
    if (role) return [{ label: `Empty ${ROLE_LABELS[role]}…`, icon: "trash", danger: true, onSelect: () => emptyHere(target) }];
    const folder = mailboxOf(target);
    if (!folder || target.kind !== "mailbox") return [];
    const { accountId } = target;
    const entries: MenuEntry[] = [{ label: t("New folder inside…"), icon: "folder", onSelect: () => newFolder(accountId, folder) }];
    if (!folder.role) {
      entries.push(
        {
          label: t("Rename…"),
          icon: "compose",
          onSelect: () =>
            setNaming({
              title: t("Rename “{name}”", { name: leafName(folder) }),
              label: t("Folder name"),
              value: leafName(folder),
              confirm: t("Rename"),
              onSave: (name) => renameFolder(folder.id, name),
            }),
        },
        { label: t("Delete folder…"), icon: "trash", danger: true, onSelect: () => deleteFolderAsked(accountId, folder) },
      );
    }
    return entries;
  };

  // Saved searches (#205): kept by name, run again when opened from the sidebar.
  const [savedSearches, setSavedSearches] = useState<SavedSearch[]>([]);
  const loadSaved = useCallback(() => {
    listSavedSearches().then(setSavedSearches, () => setSavedSearches([]));
  }, []);
  useEffect(loadSaved, [loadSaved]);
  useChanges(loadSaved);
  // Those shown as groups of the Smart Inbox: of the one account whose inbox is open, or all;
  // each found as far back as the list is loaded, as the list leaves their messages out.
  const searchGroups = useSearchGroups(
    savedSearches,
    appPrefs.searchGroups,
    !inInbox && accountInbox ? accountInbox.account.id : null,
    smartOn,
    smartOn ? (shownFeed.items[shownFeed.items.length - 1]?.listed_at ?? null) : null,
  );
  // A group's messages show in the group alone, not in the list below as well.
  const inGroups = useMemo(() => groupedIds(searchGroups), [searchGroups]);
  const listFeed = useMemo(
    (): MessageFeed =>
      smartOn && inGroups.size > 0 ? { ...shownFeed, items: shownFeed.items.filter((item) => !inGroups.has(item.id)) } : shownFeed,
    [smartOn, inGroups, shownFeed],
  );
  const openSaved = (saved: SavedSearch) => {
    if (layout.mode === "single") setOpen(null);
    setSearch({ query: saved.query, accountId: saved.account_id });
  };
  const savedNow =
    search && savedSearches.find((saved) => saved.query === search.query.trim() && saved.account_id === search.accountId);
  const saveThisSearch = () => {
    const query = search?.query.trim();
    if (!query) return;
    const accountId = search?.accountId ?? null;
    setNaming({
      title: t("Save this search"),
      label: t("Name"),
      value: "",
      confirm: t("Save"),
      onSave: (name) =>
        saveSearch({ name, query, accountId }).then(() => setSaid(t("Saved: “{name}” is in the sidebar", { name }))),
    });
  };
  // The sender card's "Group their mail": a saved search of their mail, shown as a group.
  const senderGroupOf = (query: string, accountId: string) =>
    savedSearches.find((saved) => saved.query.trim() === query && (saved.account_id === null || saved.account_id === accountId));
  const senderGrouped = (query: string, accountId: string) => {
    const saved = senderGroupOf(query, accountId);
    return saved !== undefined && appPrefs.searchGroups.includes(saved.id);
  };
  const groupSender = (query: string, name: string, accountId: string, on: boolean) => {
    const existing = senderGroupOf(query, accountId);
    if (!on) {
      if (existing) setPrefs({ searchGroups: getPrefs().searchGroups.filter((id) => id !== existing.id) });
      return;
    }
    const shown = (saved: SavedSearch) => {
      setPrefs({ searchGroups: [...getPrefs().searchGroups.filter((id) => id !== saved.id), saved.id] });
      setSaid(t("“{name}” is now one row of the inbox by priority", { name: saved.name }));
    };
    if (existing) shown(existing);
    else
      saveSearch({ name, query, accountId }).then((saved) => {
        loadSaved();
        shown(saved);
      }, failed);
  };
  const savedMenu = (saved: SavedSearch): MenuEntry[] => {
    const change = (fields: Partial<{ name: string; query: string; color: string | null }>) =>
      saveSearch({ id: saved.id, name: saved.name, query: saved.query, accountId: saved.account_id, color: saved.color, ...fields });
    const grouped = appPrefs.searchGroups.includes(saved.id);
    return [
      {
        label: grouped ? t("Stop showing as a group in the inbox") : t("Show as a group in the inbox"),
        icon: "sections",
        onSelect: () =>
          setPrefs({
            searchGroups: grouped
              ? appPrefs.searchGroups.filter((id) => id !== saved.id)
              : [...appPrefs.searchGroups, saved.id],
          }),
      },
      {
        label: t("Rename…"),
        icon: "compose",
        onSelect: () =>
          setNaming({ title: t("Rename “{name}”", { name: saved.name }), label: t("Name"), value: saved.name, confirm: t("Rename"), onSave: (name) => change({ name }) }),
      },
      {
        label: t("Edit the search…"),
        icon: "search",
        onSelect: () =>
          setNaming({
            title: t("What “{name}” looks for", { name: saved.name }),
            label: t("Search for"),
            value: saved.query,
            confirm: t("Save"),
            onSave: (query) => change({ query }),
          }),
      },
      {
        label: t("Colour…"),
        icon: "sun",
        onSelect: () =>
          setMenu({
            anchor: { x: 0, y: 0 },
            variant: "palette",
            title: t("Colour of “{name}”", { name: saved.name }),
            label: t("Colour"),
            entries: [
              { label: t("No colour"), onSelect: () => void change({ color: null }).catch(failed) },
              ...ACCOUNT_COLOURS.map((colour) => ({
                label: COLOUR_NAMES[colour] ?? colour,
                onSelect: () => void change({ color: colour }).catch(failed),
              })),
            ],
          }),
      },
      "separator",
      {
        label: t("Forget it…"),
        icon: "trash",
        danger: true,
        onSelect: () =>
          void confirm({
            title: `Forget “${saved.name}”?`,
            body: t("The saved search goes from the sidebar. Its mail stays where it is."),
            confirm: t("Forget"),
            danger: true,
          }).then((ok) => {
            if (ok) removeSavedSearch(saved.id).catch(failed);
          }),
      },
    ];
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
        failed(t("Messages can only be moved to a folder of their own account."));
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

  /** Every folder of every account, to pick one by typing its name. */
  const goToFolder = () =>
    setMenu({
      anchor: { x: 0, y: 0 },
      variant: "palette",
      title: t("Go to folder"),
      label: t("Go to folder"),
      filter: t("Type a folder name"),
      entries: accounts.flatMap((entry) =>
        sortFolders(entry.mailboxes.filter((m) => m.selectable)).map((mailbox) => ({
          label: leafName(mailbox),
          icon: mailbox.role ? ROLE_ICONS[mailbox.role] : "folder",
          depth: depth(mailbox),
          detail: accounts.length > 1 ? entry.account.email : undefined,
          onSelect: () => changeView({ kind: "mailbox", mailboxId: mailbox.id, accountId: entry.account.id }),
        })),
      ),
    });

  /** The inbox, shown by priority, by category or by date. */
  const inboxBy = (inboxView: Layout["inboxView"]) => {
    setLayout({ inboxView });
    changeView(INBOX);
  };

  /**
   * The command palette: the actions on what it was opened for (a row, the
   * selection, a conversation or one message), then going places and the app's own commands.
   */
  const palette = (actions: Action[] | null, title?: string, extra: MenuEntry[] = []) => {
    const entries: MenuEntry[] = [];
    if (actions) {
      for (const action of choose(actions, ...PALETTE_ACTIONS)) {
        const done = action.id === "archive" && action.label === t("Archive");
        entries.push({
          label: done ? t("Done (archive)") : action.label,
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
    entries.push({ label: t("Go to folder…"), icon: "folder", keys: firstKey("goFolder"), onSelect: goToFolder });
    entries.push("separator");
    if (readTargets().length > 0) entries.push({ label: t("Mark everything here read"), icon: "mailOpen", onSelect: markAllHere });
    entries.push(
      { label: t("New message"), icon: "compose", keys: firstKey("compose"), onSelect: writeNew },
      {
        label: t("Search mail"),
        icon: "search",
        keys: firstKey("search"),
        onSelect: () => {
          if (layout.mode === "single") setOpen(null);
          setSearch({ query: "", accountId: null });
        },
      },
      ...(search?.query.trim() && !savedNow ? [{ label: t("Save this search…"), icon: "star" as const, onSelect: saveThisSearch }] : []),
      ...savedSearches.map((saved) => ({ label: `Search: ${saved.name}`, icon: "search" as const, detail: saved.query, onSelect: () => openSaved(saved) })),
      { label: t("Check for new mail"), icon: "refresh", keys: firstKey("sync"), onSelect: syncNow },
      { label: layout.locked ? t("Fold the sidebar") : t("Show the sidebar"), icon: "sidebar", keys: firstKey("sidebar"), onSelect: toggleSidebar },
      // Back beside the list from here; leaving it is chosen in Settings › Appearance, not by a stray pick.
      ...(layout.mode === "single"
        ? [{ label: t("Open messages beside the list"), icon: "split" as const, onSelect: () => setLayout({ mode: "split" }) }]
        : []),
      { label: t("Light theme"), icon: "sun", onSelect: () => applyTheme("light", true) },
      { label: t("Dark theme"), icon: "moon", onSelect: () => applyTheme("dark", true) },
      { label: t("Theme as the system"), icon: "sun", onSelect: () => applyTheme("system", true) },
      { label: t("Settings"), icon: "settings", keys: firstKey("settings"), onSelect: () => setSettings("accounts") },
      { label: t("Keyboard shortcuts"), icon: "keyboard", keys: firstKey("help"), onSelect: () => setHelp(true) },
    );
    setMenu({
      anchor: { x: 0, y: 0 },
      variant: "palette",
      title: title ?? t("Actions"),
      filter: t("Type a command, or “go to”"),
      label: t("Commands"),
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
      label: t("Regards"),
      entries: [
        { label: t("About Regards"), onSelect: () => setSettings("support") },
        "separator",
        { label: t("Settings…"), hint: commandHint("settings"), onSelect: () => setSettings("accounts") },
        { label: t("Accounts…"), onSelect: () => setSettings("accounts") },
        { label: t("Add an account…"), icon: "plus", onSelect: () => setAddingAccount(true) },
        { label: t("Keyboard shortcuts"), hint: commandHint("help"), onSelect: () => setHelp(true) },
        "separator",
        { label: t("Quit Regards"), hint: commandHint("quit"), onSelect: quit },
      ],
    });

  useShortcuts({
    help: () => setHelp(true),
    compose: writeNew,
    settings: () => setSettings("accounts"),
    commands: () => palette(openItem ? actionsForItems([openItem]) : null, openItem?.subject || undefined),
    // The list handles it when shown; here when a message covers it.
    search: () => {
      if (layout.mode === "single") setOpen(null);
      setSearch((now) => now ?? { query: "", accountId: null });
    },
    quit,
    newFolder: newFolderHere,
    ...(inboxByDate ? { showDone: () => setShowDone((now) => !now) } : {}),
    sidebar: toggleSidebar,
    goAside: () => changeView({ kind: "marked", mark: "aside" }),
    goPinned: () => changeView({ kind: "marked", mark: "pinned" }),
    goSnoozed: () => changeView({ kind: "marked", mark: "snoozed" }),
    goFolder: goToFolder,
    inboxPriority: () => inboxBy("smart"),
    inboxCategory: () => inboxBy("cards"),
    inboxDate: () => inboxBy("date"),
    sync: syncNow,
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

  // The open sidebar's edge, the same way.
  const appRoot = useRef<HTMLDivElement>(null);
  const sideDrag = useRef<{ x: number; width: number } | null>(null);
  const sideResizer = (
    <div
      className="side-resizer"
      role="separator"
      aria-orientation="vertical"
      aria-label={t("Resize the sidebar")}
      aria-valuenow={layout.sidebarWidth}
      aria-valuemin={SIDE_MIN}
      aria-valuemax={SIDE_MAX}
      tabIndex={0}
      onPointerDown={(event) => {
        event.currentTarget.setPointerCapture(event.pointerId);
        sideDrag.current = { x: event.clientX, width: layout.sidebarWidth };
        event.currentTarget.classList.add("dragging");
      }}
      onPointerMove={(event) => {
        if (!sideDrag.current) return;
        const width = clampSide(sideDrag.current.width + event.clientX - sideDrag.current.x);
        appRoot.current?.style.setProperty("--side-w", `${width}px`);
      }}
      onPointerUp={(event) => {
        if (!sideDrag.current) return;
        event.currentTarget.classList.remove("dragging");
        setLayout({ sidebarWidth: clampSide(sideDrag.current.width + event.clientX - sideDrag.current.x) });
        sideDrag.current = null;
      }}
      onDoubleClick={() => setLayout({ sidebarWidth: 238 })}
      onKeyDown={(event) => {
        const by = event.key === "ArrowLeft" ? -16 : event.key === "ArrowRight" ? 16 : 0;
        if (!by) return;
        event.preventDefault();
        setLayout({ sidebarWidth: clampSide(layout.sidebarWidth + by) });
      }}
    />
  );

  const service =
    check.state === "failed"
      ? t("Could not reach the background service: {problem}", { problem: check.message })
      : check.state === "done" && (check.status.error || !check.status.running)
        ? `${t("The background service is not running.")} ${check.status.error ?? ""}`
        : null;
  const banner = service && (
    <div className="banner" role="alert">
      <Icon name="alert" size={16} />
      <span>{service}</span>
      <button type="button" className="btn small" onClick={recheck}>
        {t("Try again")}
      </button>
    </div>
  );

  if (noAccounts || (welcoming && mail.state === "ready")) {
    // The first run still has the window's own title band and caption buttons.
    return (
      <div className={["welcome-shell", PLATFORM !== "linux" ? "framed" : "", PLATFORM].join(" ")}>
        {PLATFORM !== "linux" && <div className="drag-band" data-tauri-drag-region aria-hidden="true" />}
        {banner}
        <Welcome onDone={() => setWelcoming(false)} />
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
            focusHold={focusHold}
            onClose={() => {
              setSettings(null);
              setFocusHold(null);
            }}
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
      ? `Inbox${view.inbox ? ` · ${(() => { const owner = accounts.find((entry) => entry.account.id === view.inbox?.accountId); return owner ? accountLabel(appPrefs, owner.account) : ""; })()}` : ""}`
      : view.kind === "mailbox"
        ? `${viewAccount ? accountLabel(appPrefs, viewAccount.account) : ""}${accountInbox ? (smartOn ? " · By priority" : cardsOn ? " · By category" : " · By date") : ""}`
        : view.kind === "drafts"
          ? `${drafts.length} ${drafts.length === 1 ? "draft" : "drafts"}`
          : view.kind === "outbox"
            ? `${outbox.length} waiting`
            : smartOn
              ? t("By priority")
              : cardsOn
                ? t("By category")
                : t("By date");
  // An account's mail still coming in for the first time: its folders are
  // not empty, the messages are not here yet.
  const arriving =
    view.kind !== "marked" &&
    view.kind !== "drafts" &&
    view.kind !== "outbox" &&
    accounts.some(
      (entry) => syncAccounts.includes(entry.account.id) && entry.error === null && !entry.sync?.last_success_at,
    );
  const empty = arriving
    ? {
        title: t("Your mail is coming in"),
        body: t("Messages show here as they download. The first time can take a few minutes."),
        arriving: true,
      }
    : view.kind === "role" && view.role === "inbox"
      ? { title: t("You're all caught up"), body: t("New mail shows here as it arrives.") }
      : view.kind === "section"
        ? view.section === "new_senders"
          ? { title: t("No new senders"), body: t("Mail from people you haven't written to yet waits here to be accepted or blocked.") }
          : { title: t("Nothing in {section}", { section: SECTION_LABELS[view.section] }), body: t("This part of the inbox is empty.") }
      : view.kind === "marked"
        ? {
            pinned: { title: t("Nothing pinned"), body: t("Pin a message (p) to keep it in reach here.") },
            snoozed: { title: t("Nothing snoozed"), body: t("Snooze a message (b) and it comes back as new at the time you pick.") },
            reminders: {
              title: t("No reminders"),
              body: t("Remind yourself (Shift+R) about a message you sent: if nobody replies by then, it comes back to the inbox."),
            },
            aside: { title: t("Nothing set aside"), body: t("Set a message aside (Shift+A) to take it out of the inbox until you need it.") },
            invitations: { title: t("No invitations to answer"), body: t("Invitations by email wait here until you accept or decline them.") },
          }[view.mark]
        : { title: t("Nothing here"), body: t("This folder has no messages.") };
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
            label: t("Accounts"),
            entries: [
              { label: t("All accounts"), icon: "inbox", onSelect: () => changeView({ kind: "role", role }) },
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

  /** Done with these: archived together, one undo (as Done on a selection). */
  const doneWith = (list: ListItem[]) => {
    if (list.length > 0) choose(actionsForItems(list), "archive")[0]?.run(null);
  };
  /** Done with a whole group: every message of the section in every inbox. */
  const doneSection = (section: Section) => () => {
    allOf(sectionSources(scope, section)).then(doneWith, failed);
  };

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
        bundles: (
          [
          {
            key: "notifications",
            label: t("Notifications"),
            icon: "bell",
            count: overview.notifications?.total ?? 0,
            unread: (overview.notifications?.unread ?? 0) > 0,
            messages: notificationItems,
            newest: notificationItems[0]?.listed_at,
            onOpen: openSection("notifications"),
            onDone: doneSection("notifications"),
          },
          {
            key: "newsletters",
            label: t("Newsletters"),
            icon: "newspaper",
            count: overview.newsletters?.total ?? 0,
            unread: (overview.newsletters?.unread ?? 0) > 0,
            messages: newsletterItems,
            newest: newsletterItems[0]?.listed_at,
            onOpen: openSection("newsletters"),
            onDone: doneSection("newsletters"),
          },
          {
            key: "invitations",
            label: t("Invitations"),
            icon: "calendar",
            count: invitationItems.length,
            unread: invitationItems.some((message) => !message.seen),
            messages: invitationItems,
            newest: invitationItems[0]?.listed_at,
            onOpen: () => changeView({ kind: "marked", mark: "invitations" }),
          },
          // Saved searches the person shows as groups: what each finds in the inbox.
          ...searchGroups.map(
            (group): Bundle => ({
              key: `search-${group.saved.id}`,
              label: group.saved.name,
              icon: "search",
              colour: group.saved.color,
              count: group.messages.length,
              unread: group.messages.some((message) => !message.seen),
              messages: group.messages,
              newest: group.messages[0]?.listed_at,
              onOpen: () => openSaved(group.saved),
            }),
          ),
          {
            key: "pinned",
            label: t("Pinned"),
            icon: "pin",
            count: pinnedItems.length,
            unread: pinnedItems.some((message) => !message.seen),
            messages: pinnedItems,
            newest: pinnedItems[0]?.listed_at,
            onOpen: () => changeView({ kind: "marked", mark: "pinned" }),
          },
          ] satisfies Bundle[]
        ).filter((bundle) => bundle.key.startsWith("search-") || appPrefs.grouped[bundle.key as keyof typeof appPrefs.grouped]),
      }
    : null;
  const cardOf = (section: Section, label: string, items: ListItem[]): InboxCard => ({
    key: section,
    label,
    items,
    total: Math.max((section === "priority" ? cardCounts[section]?.total : cardCounts[section]?.unread) ?? 0, items.length),
    onAll: openSection(section),
    onRead:
      section === "new_senders"
        ? undefined
        : () =>
            markAll(
              cardList.flatMap((entry) =>
                entry.mailboxes.filter((m) => m.role === "inbox").map((m) => ({ mailboxId: m.id, section })),
              ),
            ),
    screen: section === "new_senders",
  });
  const cards: InboxCard[] | null = cardsOn
    ? [
        ...(screen === "section" ? [cardOf("new_senders", t("New senders"), cardNewSenders)] : []),
        cardOf("priority", "Priority", cardPriority),
        cardOf("people", "People", cardPeople),
        cardOf("notifications", "Notifications", cardNotifications),
        cardOf("newsletters", "Newsletters", cardNewsletters),
        {
          key: "pinned",
          label: t("Pinned"),
          items: cardPinnedRows,
          total: Math.max(cardPinned.length, cardPinnedRows.length),
          onAll: () => changeView({ kind: "marked", mark: "pinned" }),
        },
      ]
    : null;
  // A new sender: accepted into People, or blocked (their mail goes to Junk, undoably).
  /**
   * Everyone in New senders, each once with their messages: all of the
   * section (one account's when only its inbox is shown), not only the rows shown.
   */
  const newSendersAll = async () => {
    const only = view.kind === "section" ? view.inbox?.accountId : undefined;
    const list = await allOf(sectionSources(only ? scope.filter((entry) => entry.account.id === only) : scope, "new_senders"));
    const senders = new Map<string, { accountId: string; address: string; ids: number[] }>();
    for (const item of list) {
      const address = item.from?.address?.toLowerCase();
      if (!address) continue;
      const key = `${item.accountId} ${address}`;
      const sender = senders.get(key) ?? { accountId: item.accountId, address, ids: [] };
      sender.ids.push(item.id);
      senders.set(key, sender);
    }
    return [...senders.values()];
  };
  const someone = (count: number) => tn(count, "1 sender", "{count} senders");
  const screener: Screener = {
    accept: (item) => {
      const address = item.from?.address;
      if (address) setSender(item.accountId, address, { policy: "people", priority: false }).catch(failed);
    },
    block: (item) => {
      const address = item.from?.address;
      if (address) blockSender(item.accountId, address).then((result) => moved([item.id], result), failed);
    },
    acceptAll: () => {
      newSendersAll()
        .then(async (senders) => {
          await Promise.all(senders.map((sender) => setSender(sender.accountId, sender.address, { policy: "people", priority: false })));
          if (senders.length > 0) setSaid(t("Accepted {senders}: their mail is in People", { senders: someone(senders.length) }));
        })
        .catch(failed);
    },
    // One undo for all of them: each block's own undo, made together.
    blockAll: () => {
      newSendersAll()
        .then(async (senders) => {
          if (senders.length === 0) return;
          const ok = await confirm({
            title: `Block ${someone(senders.length)}?`,
            body: t("Their mail goes to Junk, and so does what they send later. Undo brings it all back."),
            confirm: t("Block all"),
            danger: true,
          });
          if (!ok) return;
          const undos: UndoInfo[] = [];
          for (const sender of senders) {
            const result = await blockSender(sender.accountId, sender.address);
            if (result.undo) undos.push(result.undo);
          }
          moved(
            senders.flatMap((sender) => sender.ids),
            null,
          );
          if (undos.length > 0) {
            setUndo({ id: undos[0].id, description: `Blocked ${someone(senders.length)}`, also: undos.slice(1).map((undo) => undo.id) });
          }
        })
        .catch(failed);
    },
  };

  const listPane =
    view.kind === "drafts" ? (
      <section className="list-pane" aria-label={t("Drafts")}>
        <ListHeader title={title} subtitle={subtitle}>
          <button type="button" className="icon-btn" title={t("New message (c)")} aria-label={t("New message")} onClick={writeNew}>
            <Icon name="compose" size={17} />
          </button>
        </ListHeader>
        <DraftsList drafts={drafts} open={composer?.id ?? null} onOpen={editDraft} onFailed={failed} onUndo={setUndo} />
      </section>
    ) : view.kind === "outbox" ? (
      <section className="list-pane" aria-label={t("Outbox")}>
        <ListHeader title={title} subtitle={subtitle} />
        <OutboxList
          entries={outbox}
          onEdit={editDraft}
          onFailed={failed}
          onSignIn={() => setSettings("accounts")}
          onPickTime={pickSendTime}
        />
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
        onMarkAllRead={readTargets().length > 0 ? markAllHere : undefined}
        onEmpty={emptyRole(view) ? { label: `Empty ${ROLE_LABELS[emptyRole(view) ?? "trash"]}`, run: () => emptyHere(view) } : undefined}
        showDone={inboxByDate ? { on: showDone, toggle: () => setShowDone((now) => !now) } : undefined}
        feed={listFeed}
        mode={layout.mode}
        openId={openItem?.id ?? null}
        picked={picked}
        onOpen={(item, how) => {
          if (typed) rememberSearch(typed);
          setOpenedByStep(how === "step");
          setOpen(item);
        }}
        onPick={setPicked}
        actionsFor={actionsForItems}
        labelsOf={gmailLabels.size > 0 ? labelsOf : undefined}
        onDoneAll={role === "inbox" || view.kind === "section" ? doneWith : undefined}
        onPalette={(actions, paletteTitle, about) => palette(actions, paletteTitle, about ? [senderEntry(about)] : [])}
        empty={empty}
        onSplit={layout.mode === "single" && layout.handle ? () => setLayout({ mode: "split" }) : undefined}
        own={ownAddresses}
        smart={smart}
        cards={cards}
        search={{
          query: search?.query ?? null,
          onSave: savedNow ? undefined : saveThisSearch,
          onQuery: (query) => {
            setSearch(query === null ? null : { query, accountId: search?.accountId ?? null });
            if (query === null) setPicked(new Set());
          },
          scope:
            accounts.length > 1
              ? {
                  accounts: accounts.map((entry) => ({ id: entry.account.id, label: entry.account.email })),
                  accountId: search?.accountId ?? null,
                  onScope: (accountId) => setSearch({ query: search?.query ?? "", accountId }),
                }
              : null,
          recents: recentSearches,
          onRemember: rememberSearch,
          onForget: forgetSearch,
          opened,
          onOpened: openRecent,
          top: found.top,
          indexing: found.indexing,
          onRetry: () => setSearchRound((round) => round + 1),
          suggestion: found.suggestion,
          server: {
            state: serverNow?.state ?? "idle",
            added: serverNow ? serverNow.items.filter((item) => !local.items.some((other) => other.id === item.id)).length : 0,
            notHere: serverNow?.notHere ?? 0,
            unreachable: (serverNow?.unreachable ?? []).map(
              (id) => accounts.find((entry) => entry.account.id === id)?.account.email ?? id,
            ),
            more: serverNow?.state === "done" && serverNow.next !== null,
            many: (search?.accountId ?? null) === null && accounts.length > 1,
            error: serverNow?.error ?? null,
            onSearch: askServer,
          },
        }}
        screener={screener}
        screenAll={view.kind === "section" && view.section === "new_senders"}
      />
    );

  const reader = openItem && (
    <ReadingPane
      key={openItem.id}
      item={openItem}
      labels={labelsOf(openItem)}
      onLabel={(mailboxId) => changeView({ kind: "mailbox", mailboxId, accountId: openItem.accountId })}
      onDraft={editDraft}
      onOwnWindow={() => void openThreadWindow(openItem.id, openItem.subject).catch(failed)}
      stepped={openedByStep}
      folders={folders}
      mode={layout.mode}
      actionsFor={actionsFor}
      onPalette={palette}
      onSender={(anchor, messageId, person) =>
        aboutSender({ id: messageId, accountId: openItem.accountId, from: person }, anchor)
      }
      onAssistant={assistantKind ? (messageId, kind) => setAssistantJob({ messageId, kind }) : undefined}
      assistant={{
        name: assistantKind === "anthropic" ? "Claude" : "Assistant",
        onMenu: (anchor) =>
          setMenu({
            anchor,
            label: t("Assistant"),
            entries: assistantKind
              ? [
                  { label: t("Summarise this conversation…"), icon: "text", onSelect: () => setAssistantJob({ messageId: openItem.id, kind: "summary" }) },
                  { label: t("Draft a reply…"), icon: "reply", onSelect: () => setAssistantJob({ messageId: openItem.id, kind: "reply" }) },
                ]
              : // Off until a provider is chosen: the button leads there.
                [{ label: t("Set up the assistant…"), icon: "settings", onSelect: () => setSettings("assistant") }],
          }),
      }}
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
          aria-label={t("Resize the message list")}
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
              <h2>{tn(picked.size, "1 message selected", "{count} messages selected")}</h2>
              <p>{t("The actions at the top of the list apply to all of them.")}</p>
            </div>
          ) : (
            (reader ?? (
              <div className="empty reader-empty" data-tauri-drag-region>
                <span className="zero-mark" aria-hidden="true">
                  <Icon name="mail" size={30} />
                </span>
                <h2>{view.kind === "drafts" ? t("Pick a draft to keep writing") : t("No message open")}</h2>
                <p>
                  {t("Choose a message to read it here.")} <kbd>{keyText(firstKey("next") ?? "ArrowDown")}</kbd> {t("and")}{" "}
                  <kbd>{keyText(firstKey("previous") ?? "ArrowUp")}</kbd> {t("move through the list.")}
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
    onAddAccount: () => {
      setOverlay(false);
      setAddingAccount(true);
    },
    onAgents: (anchor: Anchor) => {
      setOverlay(false);
      setAgentsMenu(anchor);
    },
    onSync: syncNow,
    syncing: busy,
    syncProblem,
    prefs,
    changes: sidebarChanges,
    onMenu: setMenu,
    onDrop: dropOn,
    folderMenu,
    onNewFolder: (accountId: string) => newFolder(accountId, null),
    saved: {
      searches: savedSearches,
      active: savedNow ? savedNow.id : null,
      onOpen: openSaved,
      menu: savedMenu,
      onReorder: (ids: number[]) => {
        reorderSavedSearches(ids).then(setSavedSearches, failed);
      },
    },
  };

  return (
    <div
      ref={appRoot}
      style={{ "--side-w": `${layout.sidebarWidth}px` } as CSSProperties}
      className={[
        "app",
        layout.locked ? "locked" : "",
        framed ? "framed" : "",
        PLATFORM,
        `avatars-${appPrefs.avatars}`,
        `density-${appPrefs.density}`,
        `preview-${appPrefs.previewLines}`,
        appPrefs.sidebarCounts ? "" : "no-counts",
      ].join(" ")}
    >
      {banner}
      {mail.state === "loading" && <Launch />}
      <Sidebar
        {...sidebarProps}
        expanded={layout.locked}
        locked={layout.locked}
        resizer={layout.locked ? sideResizer : undefined}
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
        {filesOver && canDrop && (
          <div className="window-drop" aria-hidden="true">
            <span>
              <Icon name="paperclip" size={22} />
              {t("Drop to write a new message with these files")}
            </span>
          </div>
        )}
        {composer && (
          <Composer
            key={composer.id}
            draftId={composer.id}
            mode={composer.mode}
            onMode={(mode) => setComposer({ ...composer, mode })}
            onClose={() => setComposer(null)}
            onDiscarded={setUndo}
            onQueued={(result) => {
              sentFrom(composer, result.done);
              setComposer(null);
              setQueued(result);
            }}
            pickSendTime={pickSendTime}
            pickReminder={pickReminder}
            pickTemplate={pickTemplate}
            onAddAccount={() => setAddingAccount(true)}
            onPopOut={(subject) => {
              setComposer(null);
              openComposerWindow(composer.id, subject).catch(failed);
            }}
          />
        )}
        {addingAccount && (
          <AddAccountDialog
            onClose={() => setAddingAccount(false)}
            onAdded={(account) => {
              setAddingAccount(false);
              setOffer({
                text: t("{account} is added. Its mail is coming in.", { account: account.email }),
                label: t("Open its Inbox"),
                run: () => openInboxOf(account.id),
              });
            }}
          />
        )}
        {assistantJob && (
          <AssistantDialog
            job={assistantJob}
            onClose={() => setAssistantJob(null)}
            onDraft={(draft) => {
              startDraft(Promise.resolve(draft), getPrefs().replyOpens);
              setSaid(t("Drafted by the assistant: check it before sending"));
            }}
          />
        )}
        <div className="dock" aria-live="polite">
          {unsentNow && (
            <div className="toast error" role="alert">
              <Icon name="alert" size={16} />
              <span>
                {unsentNow.state === "uncertain"
                  ? t("“{subject}” may not have been sent. Check before sending it again.", { subject: unsentNow.subject || t("(no subject)") })
                  : unsentNow.last_error
                    ? t("“{subject}” was not sent: {problem}", { subject: unsentNow.subject || t("(no subject)"), problem: unsentNow.last_error })
                    : t("“{subject}” was not sent.", { subject: unsentNow.subject || t("(no subject)") })}
              </span>
              <button
                type="button"
                onClick={() => {
                  setUnsent(null);
                  changeView({ kind: "outbox" });
                }}
              >
                {t("Outbox")}
              </button>
              <button type="button" onClick={() => setUnsent(null)}>
                {t("Dismiss")}
              </button>
            </div>
          )}
          {notice && (
            <div className="toast error" role="alert">
              <Icon name="alert" size={16} />
              <span>{notice}</span>
              <button type="button" onClick={() => setNotice(null)}>
                {t("Dismiss")}
              </button>
            </div>
          )}
          {offer && (
            <div className="toast">
              <Icon name="check" size={16} />
              <span>{offer.text}</span>
              {offer.run && (
                <button
                  type="button"
                  onClick={() => {
                    offer.run?.();
                    setOffer(null);
                  }}
                >
                  {offer.label}
                </button>
              )}
              <button type="button" onClick={() => setOffer(null)}>
                {t("Dismiss")}
              </button>
            </div>
          )}
          {said && (
            <div className="toast">
              <Icon name="check" size={16} />
              <span>{said}</span>
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
          {updates.status?.checks === null && accounts.length > 0 && composer?.mode !== "full" && (
            <AskUpdates
              onAnswer={(on) => updates.status && updates.setStatus({ ...updates.status, checks: on })}
            />
          )}
          {updates.status?.available && composer?.mode !== "full" && (
            <button type="button" className="aside-bubble update" onClick={() => setSettings("support")}>
              <Icon name="refresh" size={15} />
              {t("Regards {version} is available", { version: updates.status.available.version })}
            </button>
          )}
          {signedOut.length > 0 && composer?.mode !== "full" && (
            <button
              type="button"
              className="aside-bubble signed-out"
              title={t("Its mail doesn't sync, and nothing is sent from it, until you sign in again")}
              onClick={() => setSettings("accounts")}
            >
              <Icon name="alert" size={15} />
              {signedOut.length === 1
                ? t("Sign in again: {account}", { account: signedOut[0].account.email })
                : t("{count} accounts need you to sign in again", { count: signedOut.length })}
            </button>
          )}
          {holds.length > 0 && composer?.mode !== "full" && (
            <button
              type="button"
              className="aside-bubble agent-waiting"
              title={holds.map((hold) => hold.description).join("\n")}
              onClick={() => setHoldsOpen(true)}
            >
              <Icon name="agent" size={15} />
              {holds.length === 1 ? t("An agent is waiting for you") : t("Agents are waiting for you: {count}", { count: holds.length })}
            </button>
          )}
          {showBubble && (
            <button
              type="button"
              className="aside-bubble"
              title={t("Set aside (Shift+A to add)")}
              onClick={() => changeView({ kind: "marked", mark: "aside" })}
            >
              <span className="faces">
                {asideFaces.map((message) => (
                  <Avatar key={message.id} person={message.from} size="xs" />
                ))}
              </span>
              {t("{count} set aside", { count: aside.length })}
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
      {agentsMenu && (
        <AgentsMenu
          anchor={agentsMenu}
          onSettings={() => setSettings("agents")}
          onConnect={setConnecting}
          onClose={() => setAgentsMenu(null)}
        />
      )}
      {connecting && (
        <ConnectDialog open={connecting} onSettings={() => setSettings("agents")} onClose={() => setConnecting(null)} />
      )}
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
          focusHold={focusHold}
          onClose={() => {
            setSettings(null);
            setFocusHold(null);
          }}
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
      {naming && <NameDialog request={naming} onClose={() => setNaming(null)} />}
      {menu && <Menu request={menu} onClose={() => setMenu(null)} />}
      {chooser && (
        <ViewChooser
          anchor={chooser}
          current={inboxView}
          onChoose={(choice) =>
            ownViewOf ? setLayout({ accountViews: { ...layout.accountViews, [ownViewOf]: choice } }) : setLayout({ inboxView: choice })
          }
          forAccount={
            ownViewOf && accountInbox
              ? {
                  label: accountLabel(appPrefs, accountInbox.account),
                  own: layout.accountViews[ownViewOf] !== undefined,
                  onShared: () => {
                    const { [ownViewOf]: _left, ...rest } = layout.accountViews;
                    setLayout({ accountViews: rest });
                  },
                }
              : undefined
          }
          accounts={
            accounts.length > 1
              ? [
                  { label: t("All accounts"), active: inInbox, onSelect: () => changeView(INBOX) },
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
          onSearch={(address) => {
            setSenderCard(null);
            if (layout.mode === "single") setOpen(null);
            setSearch({ query: `${searchTerm("from", address)} `, accountId: null });
          }}
          onBlocked={(result) => moved(result.moved > 0 ? [sender.messageId] : [], result)}
          grouped={(query) => senderGrouped(query, sender.accountId)}
          onGroup={(query, name, on) => groupSender(query, name, sender.accountId, on)}
          onFailed={failed}
          onHolds={() => {
            setSenderCard(null);
            setSettings("agents");
          }}
        />
      )}
      {holdsOpen && (
        <HoldsDialog
          accounts={accounts.map(({ account }) => ({ id: account.id, email: account.email }))}
          onSettings={() => {
            setHoldsOpen(false);
            setSettings("agents");
          }}
          onClose={() => setHoldsOpen(false)}
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
