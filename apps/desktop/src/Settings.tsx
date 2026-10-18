import { useState } from "react";
import { removeAccount, syncAccount } from "./accounts";
import { AccountForm } from "./AccountForm";
import { Avatar } from "./Avatar";
import type { ConfirmRequest } from "./Confirm";
import { Icon, type IconName } from "./icons";
import type { AccountMail, MailState, ServiceCheck } from "./mail";
import { SHORTCUTS, useShortcuts } from "./shortcuts";
import { type Theme, applyTheme, savedTheme } from "./theme";

export type SettingsTab = "accounts" | "appearance" | "shortcuts" | "about";

const TABS: { id: SettingsTab; label: string; icon: IconName }[] = [
  { id: "accounts", label: "Accounts", icon: "user" },
  { id: "appearance", label: "Appearance", icon: "sun" },
  { id: "shortcuts", label: "Shortcuts", icon: "keyboard" },
  { id: "about", label: "About", icon: "info" },
];

export function Settings({
  tab,
  onTab,
  mail,
  service,
  onRecheck,
  confirm,
  onClose,
}: {
  tab: SettingsTab;
  onTab: (tab: SettingsTab) => void;
  mail: MailState;
  service: ServiceCheck;
  onRecheck: () => void;
  confirm: (request: ConfirmRequest) => Promise<boolean>;
  onClose: () => void;
}) {
  useShortcuts({ Escape: onClose }, { modal: true });
  return (
    <div className="backdrop" onMouseDown={onClose}>
      <section
        className="dialog settings"
        role="dialog"
        aria-modal="true"
        aria-label="Settings"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <nav className="settings-nav" aria-label="Settings">
          <h2>Settings</h2>
          {TABS.map((entry) => (
            <button
              key={entry.id}
              type="button"
              className={entry.id === tab ? "nav-item active" : "nav-item"}
              aria-current={entry.id === tab ? "page" : undefined}
              onClick={() => onTab(entry.id)}
            >
              <Icon name={entry.icon} />
              <span className="label">{entry.label}</span>
            </button>
          ))}
        </nav>
        <div className="settings-pane">
          <button type="button" className="icon-btn settings-close" aria-label="Close" title="Close (Esc)" onClick={onClose}>
            <Icon name="close" />
          </button>
          {tab === "accounts" && <AccountsTab mail={mail} confirm={confirm} />}
          {tab === "appearance" && <AppearanceTab />}
          {tab === "shortcuts" && <ShortcutsTab />}
          {tab === "about" && <AboutTab service={service} onRecheck={onRecheck} />}
        </div>
      </section>
    </div>
  );
}

function AccountsTab({ mail, confirm }: { mail: MailState; confirm: (request: ConfirmRequest) => Promise<boolean> }) {
  const [adding, setAdding] = useState(false);
  const accounts = mail.state === "ready" ? mail.accounts : [];
  return (
    <>
      <h3>Accounts</h3>
      {mail.state === "failed" && <p className="error">{mail.message}</p>}
      {accounts.length > 0 && (
        <ul className="account-list">
          {accounts.map((entry) => (
            <AccountRow key={entry.account.id} entry={entry} confirm={confirm} />
          ))}
        </ul>
      )}
      {adding ? (
        <section className="panel">
          <h4>Add an account</h4>
          <AccountForm onDone={() => setAdding(false)} onCancel={() => setAdding(false)} />
        </section>
      ) : (
        <button type="button" className="btn" onClick={() => setAdding(true)}>
          <Icon name="plus" size={16} />
          Add account
        </button>
      )}
    </>
  );
}

function AccountRow({ entry, confirm }: { entry: AccountMail; confirm: (request: ConfirmRequest) => Promise<boolean> }) {
  const { account, sync } = entry;
  const [syncing, setSyncing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const syncNow = async () => {
    setSyncing(true);
    setError(null);
    try {
      await syncAccount(account.id);
    } catch (failure) {
      setError(String(failure));
    } finally {
      setSyncing(false);
    }
  };
  const remove = async () => {
    const ok = await confirm({
      title: `Remove ${account.email}?`,
      body: "Its mail, drafts and unsent messages are removed from this computer, and its saved password is deleted. Nothing changes on the server.",
      confirm: "Remove account",
      danger: true,
    });
    if (!ok) return;
    try {
      await removeAccount(account.id);
    } catch (failure) {
      setError(String(failure));
    }
  };

  const problem = error ?? entry.error ?? sync?.last_error ?? null;
  const status =
    syncing || sync?.running
      ? "Syncing…"
      : problem
        ? `Sync failed: ${problem}`
        : sync?.push_active
          ? "Live: new mail arrives as it comes in"
          : sync?.last_success_at
            ? `Synced at ${new Date(sync.last_success_at).toLocaleTimeString()}`
            : "Not synced yet";

  return (
    <li className="account-row">
      <Avatar person={{ name: account.display_name, address: account.email }} size="lg" />
      <div className="account-text">
        <strong>{account.display_name || account.email}</strong>
        <span className="muted">{account.email}</span>
        <span className={problem ? "small error" : "small muted"}>{status}</span>
      </div>
      <div className="account-buttons">
        <button type="button" className="btn small" onClick={syncNow} disabled={syncing || sync?.running}>
          <Icon name="refresh" size={15} />
          Sync now
        </button>
        <button type="button" className="btn small danger" onClick={remove}>
          Remove
        </button>
      </div>
    </li>
  );
}

function AppearanceTab() {
  const [theme, setTheme] = useState<Theme>(savedTheme);
  const choose = (next: Theme) => {
    setTheme(next);
    applyTheme(next, true);
  };
  const options: { id: Theme; label: string }[] = [
    { id: "system", label: "As the system" },
    { id: "light", label: "Light" },
    { id: "dark", label: "Dark" },
  ];
  return (
    <>
      <h3>Appearance</h3>
      <div className="theme-choice" role="radiogroup" aria-label="Theme">
        {options.map((option) => (
          <button
            key={option.id}
            type="button"
            role="radio"
            aria-checked={theme === option.id}
            className={theme === option.id ? "theme-option chosen" : "theme-option"}
            onClick={() => choose(option.id)}
          >
            <span className={`theme-swatch ${option.id}`} aria-hidden="true">
              <span />
              <span />
            </span>
            {option.label}
          </button>
        ))}
      </div>
    </>
  );
}

export function ShortcutsTab() {
  return (
    <>
      <h3>Keyboard shortcuts</h3>
      <dl className="shortcut-list">
        {SHORTCUTS.map((shortcut) => (
          <div key={shortcut.keys}>
            <dt>
              <kbd>{shortcut.keys}</kbd>
            </dt>
            <dd>{shortcut.action}</dd>
          </div>
        ))}
      </dl>
      <p className="muted small">After clicking inside a message's text, click outside it for the keys to work again.</p>
    </>
  );
}

function AboutTab({ service, onRecheck }: { service: ServiceCheck; onRecheck: () => void }) {
  return (
    <>
      <div className="about-head">
        <span className="app-mark">
          <Icon name="logo" size={30} />
        </span>
        <div>
          <h3>Regards</h3>
          <p className="muted">Pre-alpha · local-first email</p>
        </div>
      </div>
      <h4>Background service</h4>
      <ServiceStatus check={service} />
      <button type="button" className="btn" onClick={onRecheck} disabled={service.state === "checking"}>
        <Icon name="refresh" size={15} />
        Check again
      </button>
    </>
  );
}

function ServiceStatus({ check }: { check: ServiceCheck }) {
  if (check.state === "checking") return <p className="muted">Checking…</p>;
  if (check.state === "failed") return <p className="error">Could not check the service: {check.message}</p>;
  const { status } = check;
  if (status.error || !status.running) {
    return (
      <p className="error">
        <span className="status-dot bad" aria-hidden="true" /> The service could not be started. {status.error}
      </p>
    );
  }
  return (
    <>
      <p>
        <span className="status-dot ok" aria-hidden="true" /> Running · version {status.version} · pid {status.pid} · up{" "}
        {status.uptimeSecs ?? 0}s
      </p>
      <p className="muted small">
        {status.desktopAccess ? "This app has desktop access." : "This app has agent access only."}
        {status.started && " Started by this app."}
        {status.replaced && " An older or separately started service was replaced."}
      </p>
    </>
  );
}
