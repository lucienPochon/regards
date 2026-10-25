import { type DragEvent, type MouseEvent, type ReactNode, useEffect, useRef, useState } from "react";
import type { MailboxSummary, Role } from "./accounts";
import { Avatar } from "./Avatar";
import type { OutboxEntry } from "./compose";
import { Icon, type IconName } from "./icons";
import type { AccountMail, MailState } from "./mail";
import { type Anchor, type MenuEntry, type MenuRequest, anchorOf } from "./Menu";
import { type SidebarChanges, type SidebarPrefs, arranged } from "./sidebar-prefs";
import { accountColour, accountLabel, usePrefs } from "./prefs";
import { useShortcuts, withKey } from "./shortcuts";
import { t } from "./i18n";
import { useOnline } from "./online";
import { MARKED_ICONS, MARKED_LABELS, ROLE_ICONS, ROLE_LABELS, type View, depth, leafName, sameView, viewKey } from "./views";
import { useDialogFocus } from "./dialogFocus";
import type { SavedSearch } from "./search";

/** What dragging messages carries: the messages, and the account and folder each is in. */
export const DRAG_TYPE = "application/x-regards-messages";
export interface DragPayload {
  ids: number[];
  accounts: string[];
  mailboxes: number[];
}

interface Row {
  view: View;
  icon: IconName;
  label: string;
}

const INBOX: Row = { view: { kind: "role", role: "inbox" }, icon: "inbox", label: t("Inbox") };
const PINNED: Row = { view: { kind: "marked", mark: "pinned" }, icon: "pin", label: MARKED_LABELS.pinned };
const DRAFTS: Row = { view: { kind: "drafts" }, icon: "draft", label: t("Drafts") };
const OUTBOX: Row = { view: { kind: "outbox" }, icon: "outbox", label: t("Outbox") };
const role = (r: Role): Row => ({ view: { kind: "role", role: r }, icon: ROLE_ICONS[r], label: ROLE_LABELS[r] });
const marked = (mark: "snoozed" | "reminders" | "aside"): Row => ({
  view: { kind: "marked", mark },
  icon: MARKED_ICONS[mark],
  label: MARKED_LABELS[mark],
});

/** What dragging a row within its section carries: its view key. */
const ROW_TYPE = "application/x-regards-sidebar-row";

/** The top section, under the inbox, in this order (unless the person moved some). */
const TOP: Row[] = [PINNED, DRAFTS, role("sent"), role("trash"), OUTBOX];
/** Under the Folders header before any starred folder. Archive is where Done mail goes, so it shows a check. */
const ARCHIVE: Row = { ...role("archive"), icon: "check" };
/** The rows the sidebar has unless taken out (they then wait in the All folders panel). */
const STANDARD = [...TOP, ARCHIVE];
/** The other system views, only in the All folders panel unless starred. */
const EXTRA: Row[] = [marked("snoozed"), marked("reminders"), marked("aside"), role("junk"), role("all"), role("flagged")];
/** Roles that can be opened per account from a chevron. */
const PER_ACCOUNT: Role[] = ["inbox", "sent", "trash"];

/**
 * The folders. Locked open beside the mail, or a dark rail of icons that
 * opens out over it (`/`). More opens the All folders panel beside it.
 */
export function Sidebar({
  mail,
  outbox,
  view,
  onView,
  expanded,
  overlay = false,
  locked,
  onToggleLock,
  onExpand,
  onSettings,
  onAgents,
  onSync,
  syncing,
  syncProblem,
  prefs,
  changes,
  onMenu,
  onDrop,
  folderMenu,
  onNewFolder,
  onAddAccount,
  saved,
  resizer,
}: {
  mail: MailState;
  outbox: OutboxEntry[];
  view: View;
  onView: (view: View) => void;
  /** With names and folders; otherwise the icon rail. */
  expanded: boolean;
  /** Open over the content, not beside it. */
  overlay?: boolean;
  locked: boolean;
  onToggleLock: () => void;
  /** Open the full sidebar over the content. */
  onExpand: () => void;
  onSettings: () => void;
  /** The Agents menu: recipes and the agents on this computer, beside `anchor`. */
  onAgents?: (anchor: Anchor) => void;
  onSync: () => void;
  syncing: boolean;
  /** Why the last sync failed, if it did. */
  syncProblem: string | null;
  prefs: SidebarPrefs;
  changes: SidebarChanges;
  onMenu: (request: MenuRequest) => void;
  onDrop: (view: View, payload: DragPayload) => void;
  /** What can be done to a folder itself (new inside, rename, delete, empty), first in its menu. */
  folderMenu?: (view: View) => MenuEntry[];
  /** A new folder at the top of an account. */
  onNewFolder?: (accountId: string) => void;
  /** Adding another account: the last row under the Inbox chevron. */
  onAddAccount?: () => void;
  saved?: SavedList;
  /** The edge that makes the open sidebar wider or narrower. */
  resizer?: ReactNode;
}) {
  const [open, setOpen] = useState<Set<Role>>(() => new Set());
  const [panel, setPanel] = useState(false);
  const [recentsOpen, setRecentsOpen] = useState(true);
  const [dropOn, setDropOn] = useState<string | null>(null);
  // A row being dragged to another place in its section, and the row it is over.
  const [rowOver, setRowOver] = useState<string | null>(null);
  const accounts = mail.state === "ready" ? mail.accounts : [];
  const many = accounts.length > 1;
  // Each account under a chevron: with several; under Inbox also with one, for "Add an account…".
  const listed = (r: Role) => many || (r === "inbox" && onAddAccount !== undefined && accounts.length > 0);
  // An account added: the inbox's list opens on it.
  const counted = useRef(accounts.length);
  useEffect(() => {
    if (accounts.length > counted.current && counted.current > 0) setOpen((now) => new Set(now).add("inbox"));
    counted.current = accounts.length;
  }, [accounts.length]);
  const appPrefs = usePrefs();
  const nameOf = (account: { id: string; email: string }) => accountLabel(appPrefs, account);
  const me = accounts[0]?.account;
  const hidden = new Set(prefs.hidden);
  // Offline, or a sync that fails: said at the foot, and on the rail.
  const online = useOnline();
  const status = !online
    ? { label: t("Offline"), detail: t("Mail you send waits in the Outbox and goes when you're back online.") }
    : syncProblem
      ? { label: t("Sync problem"), detail: syncProblem }
      : null;

  const roleExists = (r: Role) => accounts.some((a) => a.mailboxes.some((m) => m.role === r && m.selectable));
  const exists = (row: Row) => (row.view.kind === "role" ? roleExists(row.view.role) : accounts.length > 0 || row.view.kind !== "marked");
  const inboxes = accounts.flatMap((a) => a.mailboxes.filter((m) => m.role === "inbox"));
  const unread = inboxes.some((m) => m.unread !== undefined)
    ? inboxes.reduce((sum, m) => sum + (m.unread ?? 0), 0)
    : undefined;
  const troubled = outbox.some((e) => e.state === "failed" || e.state === "uncertain" || e.state === "held");
  const count = (row: Row): { value: number; alert: boolean; unread?: boolean } | null => {
    if (row.view.kind === "role" && row.view.role === "inbox" && unread) return { value: unread, alert: false, unread: true };
    if (row.view.kind === "outbox" && outbox.length > 0) return { value: outbox.length, alert: troubled };
    return null;
  };
  const folderName = (target: View): Row | null => {
    if (target.kind !== "mailbox") {
      return [...STANDARD, ...EXTRA, INBOX].find((row) => sameView(row.view, target)) ?? null;
    }
    const account = accounts.find((a) => a.account.id === target.accountId);
    const mailbox = account?.mailboxes.find((m) => m.id === target.mailboxId);
    if (!mailbox) return null;
    const label = many && mailbox.role && account ? `${leafName(mailbox)} · ${nameOf(account.account)}` : leafName(mailbox);
    return { view: target, icon: mailbox.role ? ROLE_ICONS[mailbox.role] : "folder", label };
  };

  const go = (target: View) => {
    setPanel(false);
    onView(target);
  };

  // Messages dropped on a folder go there (see App's onDrop for what each one does).
  const droppable = (target: View) =>
    !(target.kind === "drafts" || target.kind === "outbox" || (target.kind === "role" && (target.role === "sent" || target.role === "drafts")));
  const dragProps = (target: View) =>
    droppable(target)
      ? {
          onDragOver: (event: DragEvent) => {
            if (!event.dataTransfer.types.includes(DRAG_TYPE)) return;
            event.preventDefault();
            event.dataTransfer.dropEffect = "move";
            setDropOn(viewKey(target));
          },
          onDragLeave: () => setDropOn((now) => (now === viewKey(target) ? null : now)),
          onDrop: (event: DragEvent) => {
            event.preventDefault();
            setDropOn(null);
            try {
              onDrop(target, JSON.parse(event.dataTransfer.getData(DRAG_TYPE)) as DragPayload);
            } catch {
              // Not ours.
            }
          },
        }
      : {};

  const rowMenu = (event: MouseEvent, entries: MenuEntry[], target: View) => {
    const own = folderMenu?.(target) ?? [];
    const all: MenuEntry[] = own.length > 0 && entries.length > 0 ? [...own, "separator", ...entries] : [...own, ...entries];
    if (all.length === 0) return;
    event.preventDefault();
    onMenu({ anchor: { x: event.clientX, y: event.clientY }, entries: all, label: t("Folder") });
  };

  // From the keyboard, too: Move up and Move down in a row's menu.
  const moveMenu = (row: Row, section: Row[]): MenuEntry[] => {
    const keys = section.map((one) => viewKey(one.view));
    const at = keys.indexOf(viewKey(row.view));
    const move = (to: number) => {
      const next = keys.filter((_, index) => index !== at);
      next.splice(to, 0, keys[at]);
      changes.arrange(next);
    };
    return [
      ...(at > 0 ? [{ label: t("Move up"), onSelect: () => move(at - 1) }] : []),
      ...(at >= 0 && at < keys.length - 1 ? [{ label: t("Move down"), onSelect: () => move(at + 1) }] : []),
    ];
  };

  const item = (
    row: Row,
    options: {
      count?: { value: number; alert: boolean; unread?: boolean } | null;
      chevron?: Role;
      menu?: MenuEntry[];
      indent?: number;
      /** The rows of its section, as shown: it can be dragged among them. */
      section?: Row[];
    } = {},
  ) => {
    const active = sameView(view, row.view);
    const key = viewKey(row.view);
    const section = options.section;
    const moving = section
      ? {
          draggable: true,
          onDragStart: (event: DragEvent) => {
            event.dataTransfer.setData(ROW_TYPE, key);
            event.dataTransfer.effectAllowed = "move";
          },
          onDragOver: (event: DragEvent) => {
            if (!event.dataTransfer.types.includes(ROW_TYPE)) return;
            event.preventDefault();
            event.dataTransfer.dropEffect = "move";
            setRowOver(key);
          },
          onDragLeave: () => setRowOver((now) => (now === key ? null : now)),
          onDrop: (event: DragEvent) => {
            const moved = event.dataTransfer.getData(ROW_TYPE);
            setRowOver(null);
            const keys = section.map((one) => viewKey(one.view));
            if (!moved || moved === key || !keys.includes(moved)) return;
            event.preventDefault();
            const next = keys.filter((one) => one !== moved);
            next.splice(next.indexOf(key), 0, moved);
            changes.arrange(next);
          },
        }
      : {};
    const menu = section ? [...moveMenu(row, section), ...(options.menu ?? [])] : (options.menu ?? []);
    return (
      <li key={key} className="side-entry" {...moving}>
        {options.chevron && listed(options.chevron) && (
          <button
            type="button"
            className="side-chevron"
            aria-label={open.has(options.chevron) ? t("Hide accounts") : t("Show accounts")}
            aria-expanded={open.has(options.chevron)}
            onClick={() =>
              setOpen((now) => {
                const next = new Set(now);
                const chevron = options.chevron as Role;
                if (next.has(chevron)) next.delete(chevron);
                else next.add(chevron);
                return next;
              })
            }
          >
            <Icon name={options.chevron && open.has(options.chevron) ? "chevronDown" : "chevronRight"} size={12} />
          </button>
        )}
        <button
          type="button"
          className={[
            "side-item",
            active ? "active" : "",
            dropOn === key || rowOver === key ? "drop" : "",
            options.indent ? `depth-${Math.min(options.indent, 4)}` : "",
          ].join(" ")}
          aria-current={active ? "page" : undefined}
          title={expanded ? undefined : row.label}
          onClick={() => go(row.view)}
          onContextMenu={(event) => rowMenu(event, menu, row.view)}
          {...dragProps(row.view)}
        >
          <Icon name={row.icon} size={19} />
          {expanded && <span className="label">{row.label}</span>}
          {expanded && options.count && (
            <span className={options.count.alert ? "count alert" : options.count.unread ? "count unread-count" : "count"}>
              {options.count.value}
            </span>
          )}
        </button>
      </li>
    );
  };

  // Each account's folder with that role, under the chevron; under Inbox, "Add an account…" last.
  const perAccount = (r: Role) =>
    listed(r) &&
    open.has(r) && [
      ...accounts.flatMap((account) => {
        const mailbox = account.mailboxes.find((m) => m.role === r && m.selectable);
        // Its mail still coming in for the first time.
        const first = account.error === null && !account.sync?.last_success_at;
        // A new account shows in the inbox's list before its folders are known.
        if (!mailbox && !(r === "inbox" && first)) return [];
        const target: View | null = mailbox ? { kind: "mailbox", mailboxId: mailbox.id, accountId: account.account.id } : null;
        const active = target !== null && sameView(view, target);
        const person = { name: account.account.display_name, address: account.account.email };
        return [
          <li key={`${r}-${account.account.id}`} className="side-entry account">
            <button
              type="button"
              className={active ? "side-item active" : "side-item"}
              title={expanded ? (first ? t("Its mail is coming in") : undefined) : nameOf(account.account)}
              onClick={() => target && go(target)}
              {...(target ? dragProps(target) : {})}
            >
              <Avatar person={person} size="xs" ring colour={accountColour(appPrefs, account.account.id)} />
              {expanded && <span className="label">{nameOf(account.account)}</span>}
              {expanded && r === "inbox" && first ? (
                <span className="spinner small side-first-sync" role="img" aria-label={t("Its mail is coming in")} />
              ) : expanded && r === "inbox" && mailbox?.unread ? (
                <span className="count unread-count">{mailbox.unread}</span>
              ) : null}
            </button>
          </li>,
        ];
      }),
      r === "inbox" && onAddAccount && (
        <li key="inbox-add-account" className="side-entry account">
          <button
            type="button"
            className="side-item side-add"
            title={expanded ? undefined : t("Add an account…")}
            aria-label={expanded ? undefined : t("Add an account…")}
            onClick={onAddAccount}
          >
            <span className="side-add-mark" aria-hidden="true">
              <Icon name="plus" size={11} />
            </span>
            {expanded && <span className="label">{t("Add an account…")}</span>}
          </button>
        </li>
      ),
    ];

  const hideMenu = (row: Row): MenuEntry[] => [{ label: t("Remove from sidebar"), onSelect: () => changes.hide(row.view) }];
  const rowKey = (row: Row) => viewKey(row.view);
  const top = arranged(
    TOP.filter((row) => !hidden.has(viewKey(row.view)) && exists(row)),
    rowKey,
    prefs.order,
  );

  if (!expanded) {
    return (
      <nav className="sidebar rail" aria-label={t("Folders")}>
        <ul className="side-list">
          {item(INBOX, { chevron: "inbox" })}
          {perAccount("inbox")}
          <li className="rail-gap" aria-hidden="true" />
          {top.map((row) => item(row, { menu: hideMenu(row), section: top }))}
          <li className="side-entry">
            <button type="button" className="side-item" title={t("Show the sidebar (/)")} aria-label={t("Show the sidebar")} onClick={onExpand}>
              <Icon name="menu" size={19} />
            </button>
          </li>
        </ul>
        {/* Empty rail space opens the sidebar too. */}
        <div className="rail-space" onClick={onExpand} aria-hidden="true" />
        <div className="rail-foot">
          {status && (
            <button
              type="button"
              className="rail-me rail-status"
              title={`${status.label}: ${status.detail}`}
              aria-label={`${status.label}: ${status.detail}`}
              onClick={online ? onSync : undefined}
            >
              <Icon name={online ? "alert" : "offline"} size={18} />
            </button>
          )}
          {onAgents && appPrefs.agentsButton && (
            <button
              type="button"
              className="rail-me"
              title={t("Agents")}
              aria-label={t("Agents")}
              onClick={(event) => onAgents(anchorOf(event.currentTarget))}
            >
              <Icon name="agent" size={19} />
            </button>
          )}
          <button type="button" className="rail-me" title={t("Settings")} aria-label={t("Settings")} onClick={onSettings}>
            {me ? <Avatar person={{ name: me.display_name, address: me.email }} size="xs" /> : <Icon name="settings" size={19} />}
          </button>
        </div>
      </nav>
    );
  }

  const folders = arranged(
    [
      ...(hidden.has(viewKey(ARCHIVE.view)) || !exists(ARCHIVE) ? [] : [ARCHIVE]),
      ...prefs.starred.map(folderName).filter((row): row is Row => row !== null),
    ],
    rowKey,
    prefs.order,
  );
  const recents = prefs.recents.map(folderName).filter((row): row is Row => row !== null);

  return (
    <nav className={overlay ? "sidebar expanded overlay" : "sidebar expanded"} aria-label={t("Folders")}>
      <div className="side-top" data-tauri-drag-region>
        <button
          type="button"
          className={syncing ? "side-tool spinning" : "side-tool"}
          title={syncProblem ? t("Sync problem: {problem}", { problem: syncProblem }) : t("Check for new mail")}
          aria-label={t("Check for new mail")}
          aria-describedby={syncProblem ? "sync-problem" : undefined}
          onClick={onSync}
          disabled={syncing}
        >
          <Icon name="refresh" size={17} />
          {syncProblem && <span className="side-alert" aria-hidden="true" />}
        </button>
        <span id="sync-problem" className="sr-only" role="status">
          {syncProblem ? t("Sync problem: {problem}", { problem: syncProblem }) : ""}
        </span>
        <button
          type="button"
          className="side-tool"
          title={locked ? t("Fold the sidebar (/)") : t("Keep the sidebar open")}
          aria-label={locked ? t("Fold the sidebar") : t("Keep the sidebar open")}
          aria-pressed={locked}
          onClick={onToggleLock}
        >
          <Icon name={locked ? "sidebarOn" : "sidebar"} size={17} />
        </button>
      </div>
      <div className="side-scroll">
        <ul className="side-list">
          {item(INBOX, { count: count(INBOX), chevron: "inbox" })}
          {perAccount("inbox")}
          <li className="side-gap" aria-hidden="true" />
          {top.map((row) => {
            const chevron = row.view.kind === "role" && PER_ACCOUNT.includes(row.view.role) ? row.view.role : undefined;
            return [
              item(row, { count: count(row), chevron, menu: hideMenu(row), section: top }),
              chevron ? perAccount(chevron) : null,
            ];
          })}
        </ul>
        <h2 className="side-heading">{t("Folders")}</h2>
        <ul className="side-list">
          {folders.map((row) =>
            item(row, {
              section: folders,
              menu:
                row === ARCHIVE
                  ? hideMenu(row)
                  : [{ label: t("Remove from sidebar"), onSelect: () => changes.unstar(row.view) }],
            }),
          )}
          <li className="side-entry">
            <button
              type="button"
              className={panel ? "side-item open" : "side-item"}
              aria-expanded={panel}
              onClick={() => setPanel(!panel)}
            >
              <Icon name="more" size={19} />
              <span className="label">{t("More")}</span>
            </button>
          </li>
        </ul>
        {saved && saved.searches.length > 0 && <SavedSearches saved={saved} onMenu={onMenu} onOpened={() => setPanel(false)} />}
        {recents.length > 0 && (
          <section className="side-recents">
            <div className="side-heading with-tools">
              <button type="button" className="side-fold" aria-expanded={recentsOpen} onClick={() => setRecentsOpen(!recentsOpen)}>
                <Icon name={recentsOpen ? "chevronDown" : "chevronRight"} size={12} />
                {t("Recents")}
              </button>
              <button type="button" className="side-clear" onClick={changes.clearRecents}>
                {t("Clear")}
              </button>
            </div>
            {recentsOpen && (
              <ul className="side-list">
                {recents.map((row) =>
                  item(row, {
                    menu: [
                      { label: t("Pin to the top"), onSelect: () => changes.star(row.view) },
                      { label: t("Remove from sidebar"), onSelect: () => changes.forget(row.view) },
                    ],
                  }),
                )}
              </ul>
            )}
          </section>
        )}
      </div>
      <div className="side-foot">
        {status && (
          <button
            type="button"
            className="side-status"
            title={online ? t("Try again") : status.detail}
            onClick={online ? onSync : undefined}
          >
            <Icon name={online ? "alert" : "offline"} size={17} />
            <span>
              <strong>{status.label}</strong>
              <span>{status.detail}</span>
            </span>
          </button>
        )}
        {onAgents && appPrefs.agentsButton && (
          <button type="button" className="side-item" onClick={(event) => onAgents(anchorOf(event.currentTarget))}>
            <Icon name="agent" size={19} />
            <span className="label">{t("Agents")}</span>
          </button>
        )}
        <button type="button" className="side-item" onClick={onSettings}>
          {me ? <Avatar person={{ name: me.display_name, address: me.email }} size="xs" /> : <Icon name="settings" size={19} />}
          <span className="label">{t("Settings")}</span>
        </button>
      </div>
      {panel && (
        <FolderPanel
          accounts={accounts}
          view={view}
          prefs={prefs}
          changes={changes}
          onOpen={(target) => {
            changes.visit(target);
            go(target);
          }}
          onClose={() => setPanel(false)}
          onMenu={onMenu}
          folderMenu={folderMenu}
          onNewFolder={onNewFolder}
          saved={saved}
        />
      )}
      {!overlay && resizer}
    </nav>
  );
}

/**
 * Every folder: the system ones not in the sidebar, then each account's
 * folders. The star adds one to the sidebar; Ctrl+click stars without closing.
 */
function FolderPanel({
  accounts,
  view,
  prefs,
  changes,
  onOpen,
  onClose,
  onMenu,
  folderMenu,
  onNewFolder,
  saved,
}: {
  accounts: AccountMail[];
  view: View;
  prefs: SidebarPrefs;
  changes: SidebarChanges;
  onOpen: (view: View) => void;
  onClose: () => void;
  onMenu: (request: MenuRequest) => void;
  folderMenu?: (view: View) => MenuEntry[];
  onNewFolder?: (accountId: string) => void;
  saved?: SavedList;
}) {
  useShortcuts({ Escape: onClose }, { modal: true });
  const appPrefs = usePrefs();
  const panel = useRef<HTMLElement>(null);
  useDialogFocus(panel);
  const nameOf = (account: { id: string; email: string }) => accountLabel(appPrefs, account);
  const hidden = new Set(prefs.hidden);
  const starred = new Set(prefs.starred.map(viewKey));
  const roleExists = (r: Role) => accounts.some((a) => a.mailboxes.some((m) => m.role === r && m.selectable));
  const system = [
    ...STANDARD.filter((row) => hidden.has(viewKey(row.view))),
    ...EXTRA.filter((row) => row.view.kind !== "role" || roleExists(row.view.role)),
  ];
  const inSidebar = (target: View) =>
    starred.has(viewKey(target)) || (STANDARD.some((row) => sameView(row.view, target)) && !hidden.has(viewKey(target)));
  const toggle = (target: View) => {
    const standard = STANDARD.some((row) => sameView(row.view, target));
    if (standard) {
      if (hidden.has(viewKey(target))) changes.show(target);
      else changes.hide(target);
    } else if (starred.has(viewKey(target))) changes.unstar(target);
    else changes.star(target);
  };

  const row = (target: View, icon: IconName, label: string, indent = 0) => {
    const on = inSidebar(target);
    return (
      <li
        key={viewKey(target)}
        className={sameView(view, target) ? "panel-row active" : "panel-row"}
        onContextMenu={(event) => {
          const entries = folderMenu?.(target) ?? [];
          if (entries.length === 0) return;
          event.preventDefault();
          onMenu({ anchor: { x: event.clientX, y: event.clientY }, entries, label: t("Folder") });
        }}
      >
        <button
          type="button"
          className={indent ? `panel-open depth-${Math.min(indent, 4)}` : "panel-open"}
          title={label}
          onClick={(event) => (event.ctrlKey || event.metaKey ? toggle(target) : onOpen(target))}
        >
          <Icon name={icon} size={17} />
          <span className="label">{label}</span>
        </button>
        <button
          type="button"
          className={on ? "panel-star on" : "panel-star"}
          title={on ? t("Remove from the sidebar") : t("Add to the sidebar")}
          aria-label={t("{name} in the sidebar", { name: label })}
          aria-pressed={on}
          onClick={() => toggle(target)}
        >
          <Icon name={on ? "starFilled" : "star"} size={15} />
        </button>
      </li>
    );
  };

  const tree = (mailboxes: MailboxSummary[]) =>
    mailboxes
      .filter((m) => m.role === null)
      .sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base" }));

  return (
    <>
      <div className="panel-catcher" onMouseDown={onClose} aria-hidden="true" />
      <aside ref={panel} className="folder-panel" role="dialog" aria-label={t("All folders")} tabIndex={-1}>
        <div className="panel-scroll">
          <h2 className="panel-head">{t("All folders")}</h2>
          <ul>{system.map((entry) => row(entry.view, entry.icon, entry.label))}</ul>
          {saved && saved.searches.length > 0 && (
            <section className="panel-account">
              <h3 className="panel-head account">
                <Icon name="search" size={15} />
                <span className="label">{t("Saved searches")}</span>
              </h3>
              <ul>
                {saved.searches.map((search) => (
                  <li key={search.id} className={saved.active === search.id ? "panel-row active" : "panel-row"}>
                    <button
                      type="button"
                      className="panel-open"
                      title={search.query}
                      onClick={() => {
                        saved.onOpen(search);
                        onClose();
                      }}
                    >
                      <Icon name="search" size={17} />
                      <span className="label">{search.name}</span>
                    </button>
                  </li>
                ))}
              </ul>
            </section>
          )}
          {accounts.map((account) => (
            <section key={account.account.id} className="panel-account">
              <h3 className="panel-head account">
                <Avatar
                  person={{ name: account.account.display_name, address: account.account.email }}
                  size="xs"
                  colour={accountColour(appPrefs, account.account.id)}
                />
                <span className="label">{nameOf(account.account)}</span>
                {onNewFolder && (
                  <button
                    type="button"
                    className="panel-new"
                    title={withKey(t("New folder in {account}", { account: account.account.email }), "newFolder")}
                    aria-label={t("New folder in {account}", { account: account.account.email })}
                    onClick={() => onNewFolder(account.account.id)}
                  >
                    <Icon name="plus" size={14} />
                  </button>
                )}
              </h3>
              <ul>
                {tree(account.mailboxes).map((mailbox) =>
                  mailbox.selectable ? (
                    row({ kind: "mailbox", mailboxId: mailbox.id, accountId: account.account.id }, "folder", leafName(mailbox), depth(mailbox))
                  ) : (
                    <li key={mailbox.id} className="panel-row static">
                      <span className={depth(mailbox) ? `panel-open depth-${Math.min(depth(mailbox), 4)}` : "panel-open"}>
                        <Icon name="folder" size={17} />
                        <span className="label">{leafName(mailbox)}</span>
                      </span>
                    </li>
                  ),
                )}
              </ul>
            </section>
          ))}
        </div>
        <p className="panel-hint">
          {t("Ctrl+click adds several folders to the sidebar. Right-click a folder to rename or delete it.")}
        </p>
      </aside>
    </>
  );
}

/** The saved searches the sidebar lists, and what can be done with them. */
export interface SavedList {
  searches: SavedSearch[];
  /** The one whose results show, if any. */
  active: number | null;
  onOpen: (search: SavedSearch) => void;
  /** Rename, change what it looks for or its colour, forget it. */
  menu: (search: SavedSearch) => MenuEntry[];
  /** Put them in this order (dragged). */
  onReorder: (ids: number[]) => void;
}

const SAVED_TYPE = "application/x-regards-saved-search";

/**
 * Saved searches under the folders: each runs again when opened; right-click
 * to change or forget it; drag one to put it elsewhere in the list.
 */
function SavedSearches({ saved, onMenu, onOpened }: { saved: SavedList; onMenu: (request: MenuRequest) => void; onOpened: () => void }) {
  const [over, setOver] = useState<number | null>(null);
  const drop = (onto: number, event: DragEvent) => {
    event.preventDefault();
    setOver(null);
    const moved = Number(event.dataTransfer.getData(SAVED_TYPE));
    if (!moved || moved === onto) return;
    const ids = saved.searches.map((search) => search.id).filter((id) => id !== moved);
    ids.splice(ids.indexOf(onto), 0, moved);
    saved.onReorder(ids);
  };
  return (
    <section className="side-saved">
      <h2 className="side-heading">{t("Saved searches")}</h2>
      <ul className="side-list">
        {saved.searches.map((search) => (
          <li
            key={search.id}
            className="side-entry"
            draggable
            onDragStart={(event) => {
              event.dataTransfer.setData(SAVED_TYPE, String(search.id));
              event.dataTransfer.effectAllowed = "move";
            }}
            onDragOver={(event) => {
              if (!event.dataTransfer.types.includes(SAVED_TYPE)) return;
              event.preventDefault();
              setOver(search.id);
            }}
            onDragLeave={() => setOver((now) => (now === search.id ? null : now))}
            onDrop={(event) => drop(search.id, event)}
          >
            <button
              type="button"
              className={["side-item", saved.active === search.id ? "active" : "", over === search.id ? "drop" : ""].join(" ")}
              aria-current={saved.active === search.id ? "page" : undefined}
              title={search.query}
              onClick={() => {
                saved.onOpen(search);
                onOpened();
              }}
              onContextMenu={(event) => {
                event.preventDefault();
                onMenu({ anchor: { x: event.clientX, y: event.clientY }, entries: saved.menu(search), label: t("Saved search") });
              }}
            >
              <Icon name="search" size={19} className={search.color ? "saved-colour" : undefined} />
              {search.color && <span className="saved-dot" style={{ background: search.color }} aria-hidden="true" />}
              <span className="label">{search.name}</span>
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}
