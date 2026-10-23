import { type FormEvent, useState } from "react";
import { type AccountSummary, type ConnectionSecurity, type LoginInput, type ServerSettings, setLogin } from "./accounts";

const SECURITY: Record<ConnectionSecurity, string> = { tls: "TLS", starttls: "STARTTLS" };

const server = (settings: ServerSettings) => `${settings.host}:${settings.port} (${SECURITY[settings.security]})`;

/**
 * A password account's sign-in (Settings › Accounts › Details): its servers
 * and user name, a new password after it was changed at the provider, or
 * other servers. The daemon signs in with them first and changes nothing
 * if the server says no; the account keeps its mail, drafts and outbox.
 * Accounts that sign in with Microsoft or Google sign in again instead.
 */
export function AccountLogin({ account, open = null }: { account: AccountSummary; open?: "password" | null }) {
  const [editing, setEditing] = useState<"password" | "servers" | null>(open);
  const [done, setDone] = useState<string | null>(null);
  const finish = (said: string) => {
    setEditing(null);
    setDone(said);
  };

  return (
    <section className="account-login" aria-label="Sign-in">
      <h4 className="settings-subhead">Sign-in</h4>
      {editing === null && (
        <>
          <dl className="login-facts">
            <dt>Incoming</dt>
            <dd>{server(account.imap)}</dd>
            <dt>Outgoing</dt>
            <dd>{server(account.smtp)}</dd>
            <dt>User name</dt>
            <dd>{account.username || account.email}</dd>
          </dl>
          {done && (
            <p className="small muted" role="status">
              {done}
            </p>
          )}
          <div className="login-buttons">
            <button type="button" className="btn small" onClick={() => setEditing("password")}>
              Change password…
            </button>
            <button type="button" className="btn small" onClick={() => setEditing("servers")}>
              Edit servers…
            </button>
          </div>
        </>
      )}
      {editing && (
        <LoginForm
          account={account}
          servers={editing === "servers"}
          onCancel={() => setEditing(null)}
          onDone={() => finish(editing === "servers" ? "Saved: it syncs again with these." : "Password changed: it syncs again.")}
        />
      )}
    </section>
  );
}

function LoginForm({
  account,
  servers,
  onCancel,
  onDone,
}: {
  account: AccountSummary;
  /** The servers and user name too; otherwise only the password. */
  servers: boolean;
  onCancel: () => void;
  onDone: () => void;
}) {
  const [input, setInput] = useState<LoginInput>({
    id: account.id,
    username: account.username && account.username !== account.email ? account.username : null,
    password: null,
    imapHost: account.imap.host,
    imapPort: account.imap.port,
    smtpHost: account.smtp.host,
    smtpPort: account.smtp.port,
    smtpSecurity: account.smtp.security,
  });
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const set = <K extends keyof LoginInput>(key: K, value: LoginInput[K]) => setInput((now) => ({ ...now, [key]: value }));

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setChecking(true);
    setError(null);
    try {
      await setLogin(input);
      onDone();
    } catch (failure) {
      setError(String(failure));
    } finally {
      setChecking(false);
    }
  };
  const ready = servers ? input.imapHost.trim() !== "" : (input.password ?? "") !== "";

  return (
    <form className="account-form login-form" onSubmit={submit}>
      <label>
        {servers ? "Password, if it changed" : "New password"}
        <input
          className="input"
          type="password"
          value={input.password ?? ""}
          onChange={(event) => set("password", event.target.value === "" ? null : event.target.value)}
          placeholder={servers ? "Leave empty to keep the saved one" : `The password for ${account.email}`}
          autoComplete="current-password"
          autoFocus={!servers}
        />
        {!servers && (
          <span className="muted small">For an app password, paste the one the provider made for Regards.</span>
        )}
      </label>
      {servers && (
        <>
          <fieldset>
            <legend>Incoming mail (IMAP, TLS)</legend>
            <label className="grow">
              Server
              <input
                className="input"
                value={input.imapHost}
                onChange={(event) => set("imapHost", event.target.value)}
                autoComplete="off"
                spellCheck={false}
                required
              />
            </label>
            <label className="port">
              Port
              <input
                className="input"
                type="number"
                min={1}
                max={65535}
                value={input.imapPort}
                onChange={(event) => set("imapPort", Number(event.target.value))}
              />
            </label>
          </fieldset>
          <fieldset>
            <legend>Outgoing mail (SMTP)</legend>
            <label className="grow">
              Server
              <input
                className="input"
                placeholder="same as incoming"
                value={input.smtpHost}
                onChange={(event) => set("smtpHost", event.target.value)}
                autoComplete="off"
                spellCheck={false}
              />
            </label>
            <label className="port">
              Port
              <input
                className="input"
                type="number"
                min={1}
                max={65535}
                value={input.smtpPort}
                onChange={(event) => set("smtpPort", Number(event.target.value))}
              />
            </label>
            <label className="security">
              Security
              <select
                className="input"
                value={input.smtpSecurity}
                onChange={(event) => {
                  const security = event.target.value as ConnectionSecurity;
                  setInput((now) => ({ ...now, smtpSecurity: security, smtpPort: security === "tls" ? 465 : 587 }));
                }}
              >
                <option value="starttls">STARTTLS</option>
                <option value="tls">TLS</option>
              </select>
            </label>
          </fieldset>
          <label>
            User name, if it is not the email address
            <input
              className="input"
              value={input.username ?? ""}
              onChange={(event) => set("username", event.target.value === "" ? null : event.target.value)}
              autoComplete="username"
              spellCheck={false}
            />
          </label>
        </>
      )}
      <p aria-live="polite" className={error ? "form-status error" : "form-status"}>
        {checking ? "Signing in to check…" : error}
      </p>
      <div className="dialog-actions">
        <button type="button" className="btn small" onClick={onCancel} disabled={checking}>
          Cancel
        </button>
        <button type="submit" className="btn small primary" disabled={checking || !ready}>
          {servers ? "Save" : "Change password"}
        </button>
      </div>
    </form>
  );
}
