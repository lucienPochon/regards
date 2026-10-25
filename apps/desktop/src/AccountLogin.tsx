import { type FormEvent, useState } from "react";
import { type AccountSummary, type ConnectionSecurity, type LoginInput, type ServerSettings, setLogin } from "./accounts";
import { t } from "./i18n";

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
    <section className="account-login" aria-label={t("Sign-in")}>
      <h4 className="settings-subhead">{t("Sign-in")}</h4>
      {editing === null && (
        <>
          <dl className="login-facts">
            <dt>{t("Incoming")}</dt>
            <dd>{server(account.imap)}</dd>
            <dt>{t("Outgoing")}</dt>
            <dd>{server(account.smtp)}</dd>
            <dt>{t("User name")}</dt>
            <dd>{account.username || account.email}</dd>
          </dl>
          {done && (
            <p className="small muted" role="status">
              {done}
            </p>
          )}
          <div className="login-buttons">
            <button type="button" className="btn small" onClick={() => setEditing("password")}>
              {t("Change password…")}
            </button>
            <button type="button" className="btn small" onClick={() => setEditing("servers")}>
              {t("Edit servers…")}
            </button>
          </div>
        </>
      )}
      {editing && (
        <LoginForm
          account={account}
          servers={editing === "servers"}
          onCancel={() => setEditing(null)}
          onDone={() => finish(editing === "servers" ? t("Saved: it syncs again with these.") : t("Password changed: it syncs again."))}
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
        {servers ? t("Password, if it changed") : t("New password")}
        <input
          className="input"
          type="password"
          value={input.password ?? ""}
          onChange={(event) => set("password", event.target.value === "" ? null : event.target.value)}
          placeholder={servers ? t("Leave empty to keep the saved one") : t("The password for {account}", { account: account.email })}
          autoComplete="current-password"
          autoFocus={!servers}
        />
        {!servers && (
          <span className="muted small">{t("For an app password, paste the one the provider made for Regards.")}</span>
        )}
      </label>
      {servers && (
        <>
          <fieldset>
            <legend>{t("Incoming mail (IMAP, TLS)")}</legend>
            <label className="grow">
              {t("Server")}
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
              {t("Port")}
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
            <legend>{t("Outgoing mail (SMTP)")}</legend>
            <label className="grow">
              {t("Server")}
              <input
                className="input"
                placeholder={t("same as incoming")}
                value={input.smtpHost}
                onChange={(event) => set("smtpHost", event.target.value)}
                autoComplete="off"
                spellCheck={false}
              />
            </label>
            <label className="port">
              {t("Port")}
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
              {t("Security")}
              <select
                className="input"
                value={input.smtpSecurity}
                onChange={(event) => {
                  const security = event.target.value as ConnectionSecurity;
                  setInput((now) => ({ ...now, smtpSecurity: security, smtpPort: security === "tls" ? 465 : 587 }));
                }}
              >
                <option value="starttls">{t("STARTTLS")}</option>
                <option value="tls">{t("TLS")}</option>
              </select>
            </label>
          </fieldset>
          <label>
            {t("User name, if it is not the email address")}
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
        {checking ? t("Signing in to check…") : error}
      </p>
      <div className="dialog-actions">
        <button type="button" className="btn small" onClick={onCancel} disabled={checking}>
          {t("Cancel")}
        </button>
        <button type="submit" className="btn small primary" disabled={checking || !ready}>
          {servers ? t("Save") : t("Change password")}
        </button>
      </div>
    </form>
  );
}
