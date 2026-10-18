import { type CSSProperties, type KeyboardEvent, type PointerEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { type MoveResult, type UndoInfo, syncAccount } from "./accounts";
import { type ActionHost, type Target, messageActions } from "./actions";
import { BulkPane } from "./BulkPane";
import { Composer } from "./Composer";
import { type Draft, type Queued, newDraft } from "./compose";
import { Confirm, type ConfirmRequest } from "./Confirm";
import { DraftsList } from "./DraftsList";
import { Icon } from "./icons";
import { LIST_MAX, LIST_MIN, clampList, useLayout, useMediaQuery } from "./layout";
import { type ListItem, sourcesFor, useDrafts, useFeed, useMail, useOutbox, useService } from "./mail";
import { type Anchor, Menu, type MenuRequest } from "./Menu";
import { ListHeader, MessageList } from "./MessageList";
import { OutboxList } from "./OutboxList";
import { ReadingPane } from "./ReadingPane";
import { SendToast } from "./SendToast";
import { Settings, type SettingsTab } from "./Settings";
import { ShortcutHelp } from "./ShortcutHelp";
import { useShortcuts } from "./shortcuts";
import { Sidebar } from "./Sidebar";
import { UndoToast } from "./UndoToast";
import { INBOX, ROLE_ICONS, ROLE_LABELS, type View, depth, leafName, sortFolders, viewKey } from "./views";
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
  };
}

export function App() {
  const { check, recheck } = useService();
  const mail = useMail();
  const drafts = useDrafts();
  const outbox = useOutbox();
  const [layout, setLayout] = useLayout();
  const narrow = useMediaQuery("(max-width: 1080px)");
  const rail = layout.rail ?? narrow;

  const [view, setView] = useState<View>(INBOX);
  const [open, setOpen] = useState<ListItem | null>(null);
  const [picked, setPicked] = useState<Set<number>>(() => new Set());
  const [composing, setComposing] = useState<number | null>(null);
  const [queued, setQueued] = useState<Queued | null>(null);
  const [undo, setUndo] = useState<UndoInfo | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [help, setHelp] = useState(false);
  const [settings, setSettings] = useState<SettingsTab | null>(null);
  const [menu, setMenu] = useState<MenuRequest | null>(null);
  const [asking, setAsking] = useState<{ request: ConfirmRequest; resolve: (ok: boolean) => void } | null>(null);
  const [syncing, setSyncing] = useState(false);
  const closeUndo = useCallback(() => setUndo(null), []);
  const closeQueued = useCallback(() => setQueued(null), []);

  const accounts = useMemo(() => (mail.state === "ready" ? mail.accounts : []), [mail]);
  const sources = useMemo(() => sourcesFor(accounts, view), [accounts, view]);
  const feed = useFeed(sources);
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
  const compose = useCallback(
    (start: Promise<Draft>) => {
      start.then((draft) => setComposing(draft.id)).catch(failed);
    },
    [failed],
  );
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

  const host = useMemo<ActionHost>(
    () => ({ compose, moved, failed, confirm, pickFolder }),
    [compose, moved, failed, confirm, pickFolder],
  );
  const actionsForItems = useCallback((list: ListItem[]) => messageActions(targetOf(list), host), [host]);
  const actionsFor = useCallback((target: Target) => messageActions(target, host), [host]);

  const changeView = (next: View) => {
    setView(next);
    setOpen(null);
    setPicked(new Set());
  };

  const composeFrom =
    openItem?.accountId ?? (view.kind === "mailbox" ? view.accountId : accounts[0]?.account.id) ?? null;
  const writeNew = () => {
    if (composeFrom) compose(newDraft(composeFrom));
  };
  useShortcuts({ "?": () => setHelp(true), c: writeNew, "Mod+,": () => setSettings("accounts") });

  // The folders being shown, synced now.
  const viewAccounts = [...new Set(sources.map((source) => source.accountId))];
  const busy = syncing || accounts.some((entry) => viewAccounts.includes(entry.account.id) && entry.sync?.running);
  const syncNow = () => {
    setSyncing(true);
    Promise.all(viewAccounts.map((id) => syncAccount(id)))
      .catch(failed)
      .finally(() => setSyncing(false));
  };

  // Dragging the column edge sets the width directly; it is remembered on release.
  const app = useRef<HTMLDivElement>(null);
  const drag = useRef<{ x: number; width: number } | null>(null);
  const resize = (width: number) => app.current?.style.setProperty("--list-w", `${clampList(width)}px`);
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
      : view.kind === "role"
        ? accounts.length > 1
          ? "All accounts"
          : accounts[0]?.account.email
        : view.kind === "drafts"
          ? `${drafts.length} ${drafts.length === 1 ? "draft" : "drafts"}`
          : `${outbox.length} waiting`;
  const empty =
    view.kind === "role" && view.role === "inbox"
      ? { title: "You're all caught up", body: "New mail shows here as it arrives." }
      : { title: "Nothing here", body: "This folder has no messages." };
  const folders = openItem
    ? (accounts.find((entry) => entry.account.id === openItem.accountId)?.mailboxes ?? []).filter((m) => m.selectable)
    : [];
  const pickedItems = feed.items.filter((item) => picked.has(item.id));

  return (
    <div
      className={rail ? "app rail" : "app"}
      ref={app}
      style={{ "--list-w": `${layout.listWidth}px` } as CSSProperties}
    >
      {banner}
      <Sidebar
        mail={mail}
        drafts={drafts}
        outbox={outbox}
        view={view}
        onView={changeView}
        rail={rail}
        onCompose={writeNew}
        canCompose={composeFrom !== null}
        onSettings={setSettings}
        onHelp={() => setHelp(true)}
      />
      <section className="list-column" aria-label={title}>
        <ListHeader title={title} subtitle={subtitle} rail={rail} onToggleRail={() => setLayout({ rail: !rail })}>
          {(view.kind === "role" || view.kind === "mailbox") && (
            <button
              type="button"
              className={busy ? "icon-btn spinning" : "icon-btn"}
              title="Check for new mail"
              aria-label="Check for new mail"
              onClick={syncNow}
              disabled={busy || viewAccounts.length === 0}
            >
              <Icon name="refresh" />
            </button>
          )}
        </ListHeader>
        {view.kind === "drafts" ? (
          <DraftsList drafts={drafts} open={composing} onOpen={setComposing} onFailed={failed} />
        ) : view.kind === "outbox" ? (
          <OutboxList entries={outbox} onEdit={setComposing} onFailed={failed} />
        ) : (
          <MessageList
            key={viewKey(view)}
            feed={feed}
            openId={openItem?.id ?? null}
            picked={picked}
            onOpen={setOpen}
            onPick={setPicked}
            actionsFor={actionsForItems}
            onMenu={setMenu}
            empty={empty}
          />
        )}
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
      </section>
      <section className="reader-column">
        {pickedItems.length > 0 ? (
          <BulkPane count={pickedItems.length} actions={actionsForItems(pickedItems)} onClear={() => setPicked(new Set())} />
        ) : openItem ? (
          <ReadingPane
            key={openItem.id}
            item={openItem}
            folders={folders}
            actionsFor={actionsFor}
            onMenu={setMenu}
            onClose={() => setOpen(null)}
          />
        ) : (
          <div className="empty reader-empty">
            <span className="app-mark large">
              <Icon name="logo" size={38} />
            </span>
            <h2>{view.kind === "drafts" ? "Pick a draft to keep writing" : "No message open"}</h2>
            <p>
              {view.kind === "drafts" ? (
                "Drafts are saved as you type."
              ) : (
                <>
                  Choose a message to read it here. <kbd>j</kbd> and <kbd>k</kbd> move through the list.
                </>
              )}
            </p>
          </div>
        )}
      </section>

      <div className="toasts">
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
              if (draftId !== null) setComposing(draftId);
            }}
          />
        )}
      </div>
      {composing !== null && (
        <Composer
          key={composing}
          draftId={composing}
          onClose={() => setComposing(null)}
          onQueued={(result) => {
            setComposing(null);
            setQueued(result);
          }}
        />
      )}
      {help && <ShortcutHelp onClose={() => setHelp(false)} />}
      {settings && (
        <Settings
          tab={settings}
          onTab={setSettings}
          mail={mail}
          service={check}
          onRecheck={recheck}
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
    </div>
  );
}
