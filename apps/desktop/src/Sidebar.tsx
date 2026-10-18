import { useState } from "react";
import type { MailboxSummary, Role } from "./accounts";
import { Avatar } from "./Avatar";
import type { Draft, OutboxEntry } from "./compose";
import { Icon, type IconName } from "./icons";
import type { AccountMail, MailState } from "./mail";
import type { SettingsTab } from "./Settings";
import {
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
} from "./views";

/** System folders shown across accounts, after the inbox and the marked views. */
const ACROSS: Role[] = ["sent", "archive", "junk", "trash"];
/** In the rail, which has less room: the folders used most. */
const RAIL_ROLES: Role[] = ["sent", "archive", "trash"];

/**
 * The folders. Usually a dark rail of icons; it opens out (with `/` or the
 * menu button) over the content, or stays open beside it when locked.
 */
export function Sidebar({
  mail,
  drafts,
  outbox,
  view,
  onView,
  expanded,
  overlay = false,
  locked,
  onToggleLock,
  onExpand,
  onSettings,
  onHelp,
  onSync,
  syncing,
}: {
  mail: MailState;
  drafts: Draft[];
  outbox: OutboxEntry[];
  view: View;
  onView: (view: View) => void;
  /** With names and folders; otherwise icons only. */
  expanded: boolean;
  /** Open over the content, not beside it. */
  overlay?: boolean;
  locked: boolean;
  onToggleLock: () => void;
  /** Open the full sidebar over the content. */
  onExpand: () => void;
  onSettings: (tab: SettingsTab) => void;
  onHelp: () => void;
  onSync: () => void;
  syncing: boolean;
}) {
  const [accountsOpen, setAccountsOpen] = useState(false);
  const accounts = mail.state === "ready" ? mail.accounts : [];
  const has = (role: Role) => accounts.some((a) => a.mailboxes.some((m) => m.role === role && m.selectable));
  const troubled = outbox.some((e) => e.state === "failed" || e.state === "uncertain" || e.state === "held");
  const me = accounts[0]?.account;
  const status = summarise(accounts);
  const many = accounts.length > 1;

  const entry = (target: View, icon: IconName, label: string, count?: number, alert?: boolean) => {
    const active = sameView(view, target);
    const isInbox = target.kind === "role" && target.role === "inbox";
    return (
      <li key={label} className="side-entry">
        {isInbox && many && (
          <button
            type="button"
            className="side-chevron"
            aria-label={accountsOpen ? "Hide accounts" : "Show accounts"}
            aria-expanded={accountsOpen}
            onClick={() => setAccountsOpen(!accountsOpen)}
          >
            <Icon name={accountsOpen ? "chevronDown" : "chevronRight"} size={12} />
          </button>
        )}
        <button
          type="button"
          className={active ? "side-item active" : "side-item"}
          aria-current={active ? "page" : undefined}
          title={expanded ? undefined : label}
          onClick={() => onView(target)}
        >
          <Icon name={icon} size={expanded ? 18 : 20} />
          {expanded && <span className="label">{label}</span>}
          {count !== undefined && count > 0 && (
            <span className={alert ? "count alert" : "count"}>{expanded ? count : count > 99 ? "99+" : count}</span>
          )}
        </button>
      </li>
    );
  };

  const accountEntries =
    many &&
    accountsOpen &&
    accounts.map((account) => {
      const inbox = account.mailboxes.find((m) => m.role === "inbox");
      if (!inbox) return null;
      const target: View = { kind: "mailbox", mailboxId: inbox.id, accountId: account.account.id };
      const active = sameView(view, target);
      return (
        <li key={account.account.id} className="side-entry account">
          <button
            type="button"
            className={active ? "side-item active" : "side-item"}
            title={expanded ? undefined : account.account.email}
            onClick={() => onView(target)}
          >
            <Avatar person={{ name: account.account.display_name, address: account.account.email }} size="xs" ring />
            {expanded && <span className="label">{account.account.email}</span>}
          </button>
        </li>
      );
    });

  if (!expanded) {
    return (
      <nav className="sidebar rail" aria-label="Folders">
        <ul className="side-list">
          {entry({ kind: "role", role: "inbox" }, "inbox", "Inbox")}
          {accountEntries}
          <li className="rail-gap" aria-hidden="true" />
          {accounts.length > 0 &&
            MARKED_VIEWS.map((mark) => entry({ kind: "marked", mark }, MARKED_ICONS[mark], MARKED_LABELS[mark]))}
          <li className="rail-gap" aria-hidden="true" />
          {entry({ kind: "drafts" }, "draft", "Drafts", drafts.length)}
          {RAIL_ROLES.filter(has).map((role) => entry({ kind: "role", role }, ROLE_ICONS[role], ROLE_LABELS[role]))}
          {outbox.length > 0 && entry({ kind: "outbox" }, "outbox", "Outbox", outbox.length, troubled)}
          <li className="rail-gap" aria-hidden="true" />
          <li className="side-entry">
            <button type="button" className="side-item" title="All folders (/)" onClick={onExpand}>
              <Icon name="folder" size={20} />
            </button>
          </li>
        </ul>
        <div className="rail-foot">
          <button type="button" className="side-item" title="Show the sidebar (/)" aria-label="Show the sidebar" onClick={onExpand}>
            <Icon name="menu" size={20} />
          </button>
          <button
            type="button"
            className="rail-me"
            title={`Settings · ${status.label}`}
            aria-label="Settings"
            onClick={() => onSettings("accounts")}
          >
            {me ? <Avatar person={{ name: me.display_name, address: me.email }} size="sm" /> : <Icon name="settings" />}
            <span className={`status-dot ${status.tone}`} aria-hidden="true" />
          </button>
        </div>
      </nav>
    );
  }

  return (
    <nav className={overlay ? "sidebar expanded overlay" : "sidebar expanded"} aria-label="Folders">
      <div className="side-top">
        <span className="side-brand">
          <span className="app-mark tiny">
            <Icon name="logo" size={15} />
          </span>
          Regards
        </span>
        <button
          type="button"
          className={syncing ? "side-tool spinning" : "side-tool"}
          title="Check for new mail"
          aria-label="Check for new mail"
          onClick={onSync}
          disabled={syncing}
        >
          <Icon name="refresh" size={17} />
        </button>
        <button
          type="button"
          className="side-tool"
          title={locked ? "Fold the sidebar (/)" : "Keep the sidebar open"}
          aria-label={locked ? "Fold the sidebar" : "Keep the sidebar open"}
          aria-pressed={locked}
          onClick={onToggleLock}
        >
          <Icon name="sidebar" size={17} />
        </button>
      </div>
      <div className="side-scroll">
        <ul className="side-list">
          {entry({ kind: "role", role: "inbox" }, "inbox", "Inbox")}
          {accountEntries}
          {accounts.length > 0 &&
            MARKED_VIEWS.map((mark) => entry({ kind: "marked", mark }, MARKED_ICONS[mark], MARKED_LABELS[mark]))}
          {entry({ kind: "drafts" }, "draft", "Drafts", drafts.length)}
          {ACROSS.filter(has).map((role) => entry({ kind: "role", role }, ROLE_ICONS[role], ROLE_LABELS[role]))}
          {outbox.length > 0 && entry({ kind: "outbox" }, "outbox", "Outbox", outbox.length, troubled)}
        </ul>
        {accounts.length === 1 ? (
          <FolderGroup
            title="Folders"
            account={accounts[0]}
            mailboxes={accounts[0].mailboxes.filter((m) => !m.role || m.role === "all" || m.role === "flagged")}
            view={view}
            onView={onView}
          />
        ) : (
          accounts.map((account) => (
            <FolderGroup
              key={account.account.id}
              title={account.account.email}
              account={account}
              mailboxes={sortFolders(account.mailboxes)}
              view={view}
              onView={onView}
              collapsible
            />
          ))
        )}
      </div>
      <div className="side-foot">
        <button type="button" className="side-item" onClick={() => onSettings("accounts")}>
          {me ? <Avatar person={{ name: me.display_name, address: me.email }} size="xs" /> : <Icon name="settings" />}
          <span className="label">Settings</span>
          <span className={`status-dot ${status.tone}`} title={status.label} aria-label={status.label} />
        </button>
        <button type="button" className="side-tool" title="Keyboard shortcuts (?)" aria-label="Keyboard shortcuts" onClick={onHelp}>
          <Icon name="keyboard" size={17} />
        </button>
      </div>
    </nav>
  );
}

function FolderGroup({
  title,
  account,
  mailboxes,
  view,
  onView,
  collapsible = false,
}: {
  title: string;
  account: AccountMail;
  mailboxes: MailboxSummary[];
  view: View;
  onView: (view: View) => void;
  collapsible?: boolean;
}) {
  const [open, setOpen] = useState(true);
  if (mailboxes.length === 0 && !account.error) return null;
  return (
    <section className="side-group">
      {collapsible ? (
        <button type="button" className="side-heading" aria-expanded={open} onClick={() => setOpen(!open)}>
          <span className="label">{title}</span>
          <Icon name={open ? "chevronDown" : "chevronRight"} size={12} />
        </button>
      ) : (
        <h2 className="side-heading">
          <span className="label">{title}</span>
        </h2>
      )}
      {open && (
        <ul className="side-list">
          {account.error && <li className="side-note">{account.error}</li>}
          {mailboxes.map((mailbox) => {
            const target: View = { kind: "mailbox", mailboxId: mailbox.id, accountId: account.account.id };
            const indent = depth(mailbox) ? ` depth-${depth(mailbox)}` : "";
            if (!mailbox.selectable) {
              return (
                <li key={mailbox.id} className="side-entry">
                  <span className={`side-item static${indent}`}>
                    <Icon name="folder" size={18} />
                    <span className="label">{leafName(mailbox)}</span>
                  </span>
                </li>
              );
            }
            const active = sameView(view, target);
            return (
              <li key={mailbox.id} className="side-entry">
                <button
                  type="button"
                  className={`side-item${active ? " active" : ""}${indent}`}
                  aria-current={active ? "page" : undefined}
                  title={mailbox.name}
                  onClick={() => onView(target)}
                >
                  <Icon name={mailbox.role ? ROLE_ICONS[mailbox.role] : "folder"} size={18} />
                  <span className="label">{leafName(mailbox)}</span>
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}

export function summarise(accounts: AccountMail[]): { tone: "ok" | "busy" | "bad" | "idle"; label: string } {
  if (accounts.length === 0) return { tone: "idle", label: "No accounts" };
  if (accounts.some((a) => a.error || a.sync?.last_error)) return { tone: "bad", label: "Sync problem" };
  if (accounts.some((a) => a.sync?.running)) return { tone: "busy", label: "Syncing…" };
  if (accounts.every((a) => a.sync?.push_active)) return { tone: "ok", label: "Up to date" };
  const last = Math.max(0, ...accounts.map((a) => a.sync?.last_success_at ?? 0));
  if (last > 0) {
    const time = new Date(last).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    return { tone: "ok", label: `Synced at ${time}` };
  }
  return { tone: "idle", label: "Not synced yet" };
}
