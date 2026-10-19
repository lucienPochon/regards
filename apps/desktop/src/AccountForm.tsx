import { type FormEvent, useEffect, useState } from "react";
import { type AccountInput, type ConnectionSecurity, type Provider, addAccount, oauthProviders, providerFor } from "./accounts";
import { Icon } from "./icons";
import { OauthWaiting, PROVIDERS, useOauth } from "./OauthSignIn";

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

/**
 * Add an account: with Microsoft or Google in the browser, when this version
 * can (ADR-0018), or with a password, where Regards signs in to check the
 * details before it saves anything.
 */
export function AccountForm({ onDone, onCancel }: { onDone: () => void; onCancel?: () => void }) {
  const [input, setInput] = useState<AccountInput>(EMPTY);
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [providers, setProviders] = useState<Provider[]>([]);
  useEffect(() => {
    oauthProviders().then(setProviders, () => setProviders([]));
  }, []);
  const oauth = useOauth(() => {
    setInput(EMPTY);
    onDone();
  });
  // The provider an address belongs to, to offer its button first.
  const suggested = providerFor(input.email);

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

  if (oauth.flow) return <OauthWaiting flow={oauth.flow} onCancel={oauth.cancel} />;

  return (
    <form className="account-form" onSubmit={submit}>
      {providers.length > 0 && (
        <div className="oauth-choice">
          {providers.map((provider) => (
            <button
              key={provider}
              type="button"
              className={provider === suggested ? "oauth-button suggested" : "oauth-button"}
              onClick={() => void oauth.start(provider, input.email.trim() || null)}
            >
              <Icon name="user" size={18} />
              <span>
                <strong>Sign in with {PROVIDERS[provider].label}</strong>
                <span className="muted">{PROVIDERS[provider].covers}</span>
              </span>
              {provider === suggested && <span className="chip">For this address</span>}
            </button>
          ))}
          {oauth.error && <p className="form-status error">{oauth.error}</p>}
          <p className="oauth-or">
            <span>or with a password</span>
          </p>
        </div>
      )}
      <div className="form-row">
        <label>
          Your name
          <input
            className="input"
            value={input.displayName}
            onChange={(e) => set("displayName", e.target.value)}
            autoComplete="name"
            placeholder="Ada Lovelace"
          />
        </label>
        <label>
          Email address
          <input
            className="input"
            type="email"
            required
            value={input.email}
            onChange={(e) => set("email", e.target.value)}
            autoComplete="email"
            placeholder="ada@example.org"
          />
        </label>
      </div>
      <label>
        Password or app password
        <input
          className="input"
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
            className="input"
            required
            placeholder="mail.example.org"
            value={input.imapHost}
            onChange={(e) => set("imapHost", e.target.value)}
            autoComplete="off"
            spellCheck={false}
          />
        </label>
        <label className="port">
          Port
          <input
            className="input"
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
            className="input"
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
            className="input"
            type="number"
            required
            min={1}
            max={65535}
            value={input.smtpPort}
            onChange={(e) => set("smtpPort", Number(e.target.value))}
          />
        </label>
        <label className="security">
          Security
          <select
            className="input"
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
            className="input"
            value={input.username ?? ""}
            onChange={(e) => set("username", e.target.value === "" ? null : e.target.value)}
            autoComplete="username"
            spellCheck={false}
          />
        </label>
      </details>
      <p aria-live="polite" className={error ? "form-status error" : "form-status"}>
        {checking ? "Signing in to check the details…" : error}
      </p>
      <div className="dialog-actions">
        {onCancel && (
          <button type="button" className="btn" onClick={onCancel} disabled={checking}>
            Cancel
          </button>
        )}
        <button type="submit" className="btn primary" disabled={checking}>
          Add account
        </button>
      </div>
    </form>
  );
}
