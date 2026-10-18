import { type FormEvent, useCallback, useEffect, useState } from "react";
import {
  type AccountInput,
  type AccountSummary,
  type ConnectionSecurity,
  type MailboxListResult,
  type Role,
  addAccount,
  listAccounts,
  listMailboxes,
  removeAccount,
  syncAccount,
} from "./accounts";

type Load =
  | { state: "loading" }
  | { state: "ready"; accounts: AccountSummary[] }
  | { state: "failed"; message: string };

export function AccountsPanel() {
  const [load, setLoad] = useState<Load>({ state: "loading" });
  const [adding, setAdding] = useState(false);

  const refresh = useCallback(() => {
    listAccounts()
      .then((accounts) => setLoad({ state: "ready", accounts }))
      .catch((error: unknown) => setLoad({ state: "failed", message: String(error) }));
  }, []);

  useEffect(refresh, [refresh]);

  const remove = async (account: AccountSummary) => {
    if (!window.confirm(`Remove ${account.email} from Regards? Its saved password is deleted too.`)) {
      return;
    }
    try {
      await removeAccount(account.id);
    } catch (error) {
      window.alert(String(error));
    }
    refresh();
  };

  return (
    <section className="card" aria-labelledby="accounts-heading">
      <h2 id="accounts-heading">Accounts</h2>
      {load.state === "loading" && <p className="muted">Loading…</p>}
      {load.state === "failed" && <p className="error">{load.message}</p>}
      {load.state === "ready" && load.accounts.length === 0 && !adding && (
        <p className="muted">No accounts yet.</p>
      )}
      {load.state === "ready" && load.accounts.length > 0 && (
        <ul className="accounts">
          {load.accounts.map((account) => (
            <AccountRow key={account.id} account={account} onRemove={() => remove(account)} />
          ))}
        </ul>
      )}
      {adding ? (
        <AddAccountForm
          onDone={() => {
            setAdding(false);
            refresh();
          }}
          onCancel={() => setAdding(false)}
        />
      ) : (
        <button type="button" onClick={() => setAdding(true)}>
          Add account
        </button>
      )}
    </section>
  );
}

const EMPTY: AccountInput = {
  displayName: "",
  email: "",
  username: null,
  password: "",
  imapHost: "",
  imapPort: 993,
  smtpHost: "",
  smtpPort: 587,
  smtpSecurity: "starttls",
};

function AddAccountForm({ onDone, onCancel }: { onDone: () => void; onCancel: () => void }) {
  const [input, setInput] = useState<AccountInput>(EMPTY);
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const set = <K extends keyof AccountInput>(key: K, value: AccountInput[K]) =>
    setInput((current) => ({ ...current, [key]: value }));

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setChecking(true);
    setError(null);
    try {
      await addAccount(input);
      setInput(EMPTY);
      onDone();
    } catch (failure) {
      setError(String(failure));
    } finally {
      setChecking(false);
    }
  };

  return (
    <form className="account-form" onSubmit={submit}>
      <label>
        Your name
        <input
          value={input.displayName}
          onChange={(e) => set("displayName", e.target.value)}
          autoComplete="name"
        />
      </label>
      <label>
        Email address
        <input
          type="email"
          required
          value={input.email}
          onChange={(e) => set("email", e.target.value)}
          autoComplete="email"
        />
      </label>
      <label>
        Password or app password
        <input
          type="password"
          required
          value={input.password}
          onChange={(e) => set("password", e.target.value)}
          autoComplete="current-password"
        />
      </label>
      <fieldset>
        <legend>Incoming mail (IMAP, TLS)</legend>
        <label className="grow">
          Server
          <input
            required
            placeholder="mail.example.com"
            value={input.imapHost}
            onChange={(e) => set("imapHost", e.target.value)}
            autoComplete="off"
            spellCheck={false}
          />
        </label>
        <label className="port">
          Port
          <input
            type="number"
            required
            min={1}
            max={65535}
            value={input.imapPort}
            onChange={(e) => set("imapPort", Number(e.target.value))}
          />
        </label>
      </fieldset>
      <fieldset>
        <legend>Outgoing mail (SMTP)</legend>
        <label className="grow">
          Server
          <input
            placeholder="same as incoming"
            value={input.smtpHost}
            onChange={(e) => set("smtpHost", e.target.value)}
            autoComplete="off"
            spellCheck={false}
          />
        </label>
        <label className="port">
          Port
          <input
            type="number"
            required
            min={1}
            max={65535}
            value={input.smtpPort}
            onChange={(e) => set("smtpPort", Number(e.target.value))}
          />
        </label>
        <label>
          Security
          <select
            value={input.smtpSecurity}
            onChange={(e) => {
              const security = e.target.value as ConnectionSecurity;
              setInput((current) => ({ ...current, smtpSecurity: security, smtpPort: security === "tls" ? 465 : 587 }));
            }}
          >
            <option value="starttls">STARTTLS</option>
            <option value="tls">TLS</option>
          </select>
        </label>
      </fieldset>
      <details>
        <summary>More options</summary>
        <label>
          User name, if it is not the email address
          <input
            value={input.username ?? ""}
            onChange={(e) => set("username", e.target.value === "" ? null : e.target.value)}
            autoComplete="username"
            spellCheck={false}
          />
        </label>
      </details>
      <p aria-live="polite" className={error ? "error" : "muted"}>
        {checking ? "Signing in to check the details…" : error}
      </p>
      <div className="actions">
        <button type="submit" disabled={checking}>
          Add account
        </button>
        <button type="button" onClick={onCancel} disabled={checking}>
          Cancel
        </button>
      </div>
    </form>
  );
}

const ROLE_LABELS: Record<Role, string> = {
  inbox: "Inbox",
  drafts: "Drafts",
  sent: "Sent",
  archive: "Archive",
  all: "All mail",
  flagged: "Flagged",
  junk: "Junk",
  trash: "Trash",
};

function AccountRow({ account, onRemove }: { account: AccountSummary; onRemove: () => void }) {
  const [folders, setFolders] = useState<MailboxListResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [syncing, setSyncing] = useState(false);

  const load = useCallback(() => {
    listMailboxes(account.id)
      .then((result) => {
        setFolders(result);
        setError(null);
      })
      .catch((failure: unknown) => setError(String(failure)));
  }, [account.id]);

  // Poll while the first sync runs; the change feed (ADR-0007) replaces this later.
  useEffect(() => {
    load();
    const timer = window.setInterval(load, 2000);
    return () => window.clearInterval(timer);
  }, [load]);

  const syncNow = async () => {
    setSyncing(true);
    try {
      await syncAccount(account.id);
    } catch (failure) {
      setError(String(failure));
    } finally {
      setSyncing(false);
      load();
    }
  };

  const sync = folders?.sync;
  const status = syncing || sync?.running
    ? "Syncing…"
    : sync?.last_error
      ? `Sync failed: ${sync.last_error}`
      : sync?.last_success_at
        ? `Synced at ${new Date(sync.last_success_at).toLocaleTimeString()}`
        : "Not synced yet";

  return (
    <li className="account">
      <div className="account-head">
        <div>
          <strong>{account.display_name}</strong> <span className="muted">{account.email}</span>
          <div className={sync?.last_error || error ? "error small" : "muted small"}>{error ?? status}</div>
        </div>
        <div className="actions">
          <button type="button" onClick={syncNow} disabled={syncing || sync?.running}>
            Sync now
          </button>
          <button type="button" onClick={onRemove}>
            Remove
          </button>
        </div>
      </div>
      {folders && folders.mailboxes.length > 0 && (
        <ul className="folders">
          {folders.mailboxes.map((mailbox) => (
            <li key={mailbox.id} className={mailbox.selectable ? undefined : "muted"}>
              {mailbox.role ? ROLE_LABELS[mailbox.role] : mailbox.name}
              {mailbox.role && mailbox.name.toLowerCase() !== ROLE_LABELS[mailbox.role].toLowerCase() && (
                <span className="muted small"> ({mailbox.name})</span>
              )}
            </li>
          ))}
        </ul>
      )}
    </li>
  );
}
