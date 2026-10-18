import { useState } from "react";
import type { MailboxSummary, Role } from "./accounts";
import type { Draft, OutboxEntry } from "./compose";
import { Icon, type IconName } from "./icons";
import type { AccountMail, MailState } from "./mail";
import type { SettingsTab } from "./Settings";
import {
  MAIN_ROLES,
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

/** Roles the main list shows across accounts; an account's own list leaves them out when it is the only one. */
const SHOWN_ABOVE = new Set<Role>(["inbox", "drafts", ...MAIN_ROLES]);

export function Sidebar({
  mail,
  drafts,
  outbox,
  view,
  onView,
  rail,
  onCompose,
  canCompose,
  onSettings,
  onHelp,
}: {
  mail: MailState;
  drafts: Draft[];
  outbox: OutboxEntry[];
  view: View;
  onView: (view: View) => void;
  /** Icons only. */
  rail: boolean;
  onCompose: () => void;
  canCompose: boolean;
  onSettings: (tab: SettingsTab) => void;
  onHelp: () => void;
}) {
  const accounts = mail.state === "ready" ? mail.accounts : [];
  const has = (role: Role) => accounts.some((a) => a.mailboxes.some((m) => m.role === role && m.selectable));
  const troubled = outbox.some((entry) => entry.state === "failed" || entry.state === "uncertain");

  const item = (target: View, icon: IconName, label: string, count?: number, alert?: boolean) => (
    <li key={label}>
      <button
        type="button"
        className={sameView(view, target) ? "nav-item active" : "nav-item"}
        aria-current={sameView(view, target) ? "page" : undefined}
        title={rail ? label : undefined}
        onClick={() => onView(target)}
      >
        <Icon name={icon} />
        <span className="label">{label}</span>
        {count !== undefined && count > 0 && <span className={alert ? "count alert" : "count"}>{count}</span>}
      </button>
    </li>
  );

  return (
    <nav className="sidebar" aria-label="Folders">
      <div className="sidebar-top">
        <button
          type="button"
          className="compose-button"
          onClick={onCompose}
          disabled={!canCompose}
          title={rail ? "New message (c)" : "c"}
        >
          <Icon name="compose" />
          <span className="label">New message</span>
        </button>
      </div>
      <div className="sidebar-scroll">
        <ul className="nav">
          {item({ kind: "role", role: "inbox" }, "inbox", "Inbox")}
          {accounts.length > 0 &&
            MARKED_VIEWS.map((mark) => item({ kind: "marked", mark }, MARKED_ICONS[mark], MARKED_LABELS[mark]))}
          {item({ kind: "drafts" }, "draft", "Drafts", drafts.length)}
          {outbox.length > 0 && item({ kind: "outbox" }, "outbox", "Outbox", outbox.length, troubled)}
          {MAIN_ROLES.filter(has).map((role) => item({ kind: "role", role }, ROLE_ICONS[role], ROLE_LABELS[role]))}
        </ul>
        {!rail &&
          (accounts.length === 1 ? (
            <FolderGroup
              title="Folders"
              account={accounts[0]}
              mailboxes={accounts[0].mailboxes.filter((m) => !m.role || !SHOWN_ABOVE.has(m.role))}
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
          ))}
      </div>
      <footer className="sidebar-foot">
        <SyncStatus accounts={accounts} rail={rail} onClick={() => onSettings("accounts")} />
        <button type="button" className="icon-btn" title="Keyboard shortcuts (?)" aria-label="Keyboard shortcuts" onClick={onHelp}>
          <Icon name="keyboard" />
        </button>
        <button type="button" className="icon-btn" title="Settings" aria-label="Settings" onClick={() => onSettings("accounts")}>
          <Icon name="settings" />
        </button>
      </footer>
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
    <section className="nav-group">
      {collapsible ? (
        <button type="button" className="nav-group-head" aria-expanded={open} onClick={() => setOpen(!open)}>
          <Icon name={open ? "chevronDown" : "chevronRight"} size={14} />
          <span className="label">{title}</span>
        </button>
      ) : (
        <h2 className="nav-group-head">
          <span className="label">{title}</span>
        </h2>
      )}
      {open && (
        <ul className="nav">
          {account.error && <li className="nav-note error">{account.error}</li>}
          {mailboxes.map((mailbox) => {
            const target: View = { kind: "mailbox", mailboxId: mailbox.id, accountId: account.account.id };
            const label = leafName(mailbox);
            const indent = depth(mailbox) ? ` depth-${depth(mailbox)}` : "";
            if (!mailbox.selectable) {
              return (
                <li key={mailbox.id}>
                  <span className={`nav-item static${indent}`}>
                    <Icon name="folder" />
                    <span className="label">{label}</span>
                  </span>
                </li>
              );
            }
            const active = sameView(view, target);
            return (
              <li key={mailbox.id}>
                <button
                  type="button"
                  className={`nav-item${active ? " active" : ""}${indent}`}
                  aria-current={active ? "page" : undefined}
                  title={mailbox.name}
                  onClick={() => onView(target)}
                >
                  <Icon name={mailbox.role ? ROLE_ICONS[mailbox.role] : "folder"} />
                  <span className="label">{label}</span>
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}

/** One line for all accounts: syncing, live, a problem, or when it last worked. */
function SyncStatus({ accounts, rail, onClick }: { accounts: AccountMail[]; rail: boolean; onClick: () => void }) {
  const { tone, label } = summarise(accounts);
  return (
    <button type="button" className="sync-status" onClick={onClick} title={rail ? label : "Accounts"}>
      <span className={`status-dot ${tone}`} aria-hidden="true" />
      <span className="label">{label}</span>
    </button>
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
