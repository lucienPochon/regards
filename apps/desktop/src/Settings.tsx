import { type ReactNode, useEffect, useState } from "react";
import { removeAccount, syncAccount } from "./accounts";
import { backgroundMode, notificationsShown, setBackgroundMode, setNotificationsShown } from "./background";
import { AccountForm } from "./AccountForm";
import { Avatar } from "./Avatar";
import type { ConfirmRequest } from "./Confirm";
import { Icon, type IconName } from "./icons";
import type { Layout } from "./layout";
import type { AccountMail, MailState, ServiceCheck } from "./mail";
import { SHORTCUTS, useShortcuts } from "./shortcuts";
import { type Theme, applyTheme, savedTheme } from "./theme";

export type SettingsTab = "general" | "accounts" | "appearance" | "shortcuts" | "about";

/** Grouped as in the nav; each item has its own colour. */
const GROUPS: { id: SettingsTab; label: string; icon: IconName; tone: string }[][] = [
  [
    { id: "general", label: "General", icon: "settings", tone: "tone-slate" },
    { id: "accounts", label: "Accounts", icon: "user", tone: "tone-blue" },
  ],
  [
    { id: "appearance", label: "Appearance", icon: "sun", tone: "tone-violet" },
    { id: "shortcuts", label: "Shortcuts", icon: "keyboard", tone: "tone-amber" },
  ],
  [{ id: "about", label: "About", icon: "info", tone: "tone-green" }],
];

export function Settings({
  tab,
  onTab,
  mail,
  service,
  onRecheck,
  layout,
  onLayout,
  confirm,
  onClose,
}: {
  tab: SettingsTab;
  onTab: (tab: SettingsTab) => void;
  mail: MailState;
  service: ServiceCheck;
  onRecheck: () => void;
  layout: Layout;
  onLayout: (change: Partial<Layout>) => void;
  confirm: (request: ConfirmRequest) => Promise<boolean>;
  onClose: () => void;
}) {
  useShortcuts({ Escape: onClose }, { modal: true });
  const me = mail.state === "ready" ? mail.accounts[0]?.account : undefined;
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
          <div className="settings-me">
            {me ? <Avatar person={{ name: me.display_name, address: me.email }} size="md" /> : <Icon name="user" />}
            <span>
              <strong>{me?.display_name || "Regards"}</strong>
              {me && <span className="muted">{me.email}</span>}
            </span>
          </div>
          {GROUPS.map((group, index) => (
            <div key={index} className="settings-group">
              {group.map((entry) => (
                <button
                  key={entry.id}
                  type="button"
                  className={entry.id === tab ? "settings-item active" : "settings-item"}
                  aria-current={entry.id === tab ? "page" : undefined}
                  onClick={() => onTab(entry.id)}
                >
                  <span className={`settings-icon ${entry.tone}`}>
                    <Icon name={entry.icon} size={18} />
                  </span>
                  {entry.label}
                </button>
              ))}
            </div>
          ))}
        </nav>
        <div className="settings-pane">
          <button type="button" className="icon-btn settings-close" aria-label="Close" title="Close (Esc)" onClick={onClose}>
            <Icon name="close" />
          </button>
          {tab === "general" && <GeneralTab />}
          {tab === "accounts" && <AccountsTab mail={mail} confirm={confirm} />}
          {tab === "appearance" && <AppearanceTab layout={layout} onLayout={onLayout} />}
          {tab === "shortcuts" && <ShortcutsTab />}
          {tab === "about" && <AboutTab service={service} onRecheck={onRecheck} />}
        </div>
      </section>
    </div>
  );
}

/** A setting: what it is on the left, the control on the right. */
function Option({ icon, label, description, children }: { icon: IconName; label: string; description?: string; children: ReactNode }) {
  return (
    <div className="option">
      <span className="option-icon">
        <Icon name={icon} size={18} />
      </span>
      <span className="option-text">
        <strong>{label}</strong>
        {description && <span>{description}</span>}
      </span>
      <span className="option-control">{children}</span>
    </div>
  );
}

function Choice<T extends string>({
  value,
  options,
  onChange,
  label,
}: {
  value: T;
  options: { id: T; label: string }[];
  onChange: (value: T) => void;
  label: string;
}) {
  return (
    <span className="choice" role="radiogroup" aria-label={label}>
      {options.map((option) => (
        <button
          key={option.id}
          type="button"
          role="radio"
          aria-checked={value === option.id}
          className={value === option.id ? "chosen" : undefined}
          onClick={() => onChange(option.id)}
        >
          {option.label}
        </button>
      ))}
    </span>
  );
}

/**
 * Background mode and notifications: kept by the app (desktop.json), and
 * also in the tray menu.
 */
function GeneralTab() {
  const [background, setBackground] = useState<boolean | null>(null);
  const [notifications, setNotifications] = useState<boolean | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    backgroundMode().then(setBackground, (failure: unknown) => setError(String(failure)));
    notificationsShown().then(setNotifications, (failure: unknown) => setError(String(failure)));
  }, []);
  const change = (save: (on: boolean) => Promise<unknown>, show: (on: boolean) => void) => (value: "on" | "off") => {
    const on = value === "on";
    setError(null);
    save(on).then(
      () => show(on),
      (failure: unknown) => setError(String(failure)),
    );
  };
  const onOff = [
    { id: "on" as const, label: "On" },
    { id: "off" as const, label: "Off" },
  ];
  return (
    <>
      <h3>General</h3>
      {error && <p className="error">{error}</p>}
      <Option
        icon="clock"
        label="Run in the background"
        description={
          background === false
            ? "Off: closing the window quits Regards. Snoozed mail, reminders and scheduled messages then wait until it is open again."
            : "Regards starts when you log in and stays in the tray when you close its window, so snoozed mail comes back and scheduled messages go. They need this computer to be on."
        }
      >
        {background !== null && (
          <Choice label="Run in the background" value={background ? "on" : "off"} options={onOff} onChange={change(setBackgroundMode, setBackground)} />
        )}
      </Option>
      <Option
        icon="bell"
        label="Notifications"
        description="When a snooze ends, nobody has replied by a reminder, or a message could not be sent. They show who and the subject, never the message."
      >
        {notifications !== null && (
          <Choice label="Notifications" value={notifications ? "on" : "off"} options={onOff} onChange={change(setNotificationsShown, setNotifications)} />
        )}
      </Option>
    </>
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
      <Avatar person={{ name: account.display_name, address: account.email }} size="lg" ring />
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

function AppearanceTab({ layout, onLayout }: { layout: Layout; onLayout: (change: Partial<Layout>) => void }) {
  const [theme, setTheme] = useState<Theme>(savedTheme);
  return (
    <>
      <h3>Appearance</h3>
      <Option icon="sun" label="Theme" description="Light, dark, or as this computer is set.">
        <Choice
          label="Theme"
          value={theme}
          options={[
            { id: "system", label: "System" },
            { id: "light", label: "Light" },
            { id: "dark", label: "Dark" },
          ]}
          onChange={(next) => {
            setTheme(next);
            applyTheme(next, true);
          }}
        />
      </Option>
      <Option
        icon={layout.mode === "split" ? "split" : "single"}
        label="Layout"
        description="A message opens in place of the list, or beside it."
      >
        <Choice
          label="Layout"
          value={layout.mode}
          options={[
            { id: "single", label: "In place" },
            { id: "split", label: "Split view" },
          ]}
          onChange={(mode) => onLayout({ mode })}
        />
      </Option>
      <Option icon="sidebar" label="Sidebar" description="Keep the folders open beside the mail, or fold them to icons (/).">
        <Choice
          label="Sidebar"
          value={layout.locked ? "open" : "icons"}
          options={[
            { id: "icons", label: "Icons" },
            { id: "open", label: "Open" },
          ]}
          onChange={(value) => onLayout({ locked: value === "open" })}
        />
      </Option>
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
