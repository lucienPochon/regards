import { type FormEvent, useCallback, useEffect, useRef, useState } from "react";
import {
  type AccountInput,
  type ConnectionSecurity,
  type Discovered,
  type Provider,
  addAccount,
  discoverAccount,
  oauthProviders,
  providerFor,
} from "./accounts";
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

/** The server fields, which the lookup fills unless the person changed them. */
type ServerField = "imapHost" | "imapPort" | "smtpHost" | "smtpPort" | "smtpSecurity";

/** An address worth looking up: something@domain.tld. */
const COMPLETE = /^[^\s@]+@[^\s@]+\.[^\s@]{2,}$/;
/** How long typing pauses before the address is looked up. */
const LOOKUP_PAUSE_MS = 700;

/** What the lookup found, in a sentence. */
function foundText(found: Discovered, email: string): string {
  const domain = email.split("@")[1] ?? "";
  switch (found.source) {
    case "known":
      return `Found the server settings for ${domain}.`;
    case "mx":
      return "Found the server settings from the domain's mail servers.";
    case "dns":
      return "Found the server settings the domain publishes.";
    case "guess":
      return "Guessed the server settings from the domain. If adding fails, check them under Server settings.";
    default:
      return "Couldn't find the server settings: enter them under Server settings.";
  }
}

/**
 * Add an account: with Microsoft or Google in the browser, when this version
 * can (ADR-0018), or with a password. The address is looked up to fill in
 * the servers, which stay editable; Regards signs in to check everything
 * before it saves anything.
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

  // The lookup: for which address, under way or done, and what it found.
  const [lookup, setLookup] = useState<{ email: string; found: Discovered | null; busy: boolean } | null>(null);
  const [serversOpen, setServersOpen] = useState(false);
  const edited = useRef(new Set<ServerField>());
  const asked = useRef<string | null>(null);

  const lookUp = useCallback((email: string) => {
    const address = email.trim();
    if (!COMPLETE.test(address) || asked.current === address) return;
    asked.current = address;
    setLookup({ email: address, found: null, busy: true });
    discoverAccount(address).then(
      (found) => {
        if (asked.current !== address) return;
        setLookup({ email: address, found, busy: false });
        // Fill what the person hasn't changed. The form's incoming mail is TLS only.
        setInput((current) => {
          const next = { ...current };
          const fill = <K extends ServerField>(key: K, value: AccountInput[K]) => {
            if (!edited.current.has(key)) next[key] = value;
          };
          if (found.imap) {
            fill("imapHost", found.imap.host);
            if (found.imap.security === "tls") fill("imapPort", found.imap.port);
          }
          if (found.smtp) {
            fill("smtpHost", found.smtp.host);
            fill("smtpPort", found.smtp.port);
            fill("smtpSecurity", found.smtp.security);
          }
          return next;
        });
        if (found.source === null) setServersOpen(true);
      },
      () => {
        // Not an address the lookup takes: the manual form, as before.
        if (asked.current !== address) return;
        setLookup(null);
        setServersOpen(true);
      },
    );
  }, []);

  // After a pause in typing.
  useEffect(() => {
    if (!COMPLETE.test(input.email.trim())) return;
    const timer = window.setTimeout(() => lookUp(input.email), LOOKUP_PAUSE_MS);
    return () => window.clearTimeout(timer);
  }, [input.email, lookUp]);

  const found = lookup && lookup.email === input.email.trim() ? lookup.found : null;
  const lookingUp = lookup !== null && lookup.email === input.email.trim() && lookup.busy;
  // The provider an address belongs to, to offer its button first.
  const suggested = found?.oauth ?? providerFor(input.email);
  const oauthOnly = found?.oauth_only === true;
  const canOauthOnly = oauthOnly && found?.oauth !== undefined && providers.includes(found.oauth);

  const set = <K extends keyof AccountInput>(key: K, value: AccountInput[K]) =>
    setInput((current) => ({ ...current, [key]: value }));
  const setServer = <K extends ServerField>(key: K, value: AccountInput[K]) => {
    edited.current.add(key);
    set(key, value);
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!input.imapHost.trim()) {
      setServersOpen(true);
      setError("Enter the incoming mail server under Server settings.");
      return;
    }
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

  const offered = oauthOnly ? providers.filter((provider) => provider === found?.oauth) : providers;
  return (
    <form className="account-form" onSubmit={submit}>
      {offered.length > 0 && (
        <div className="oauth-choice">
          {offered.map((provider) => (
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
          {!oauthOnly && (
            <p className="oauth-or">
              <span>or with a password</span>
            </p>
          )}
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
            onBlur={(e) => lookUp(e.target.value)}
            autoComplete="email"
            placeholder="ada@example.org"
          />
        </label>
      </div>
      {(lookingUp || found) && (
        <p className="lookup-status" aria-live="polite">
          {lookingUp ? (
            <>
              <span className="spinner small" aria-hidden="true" />
              Looking up your server…
            </>
          ) : found && !oauthOnly ? (
            <>
              <Icon name={found.source === null || found.source === "guess" ? "info" : "check"} size={14} />
              {foundText(found, lookup?.email ?? "")}
            </>
          ) : null}
        </p>
      )}
      {oauthOnly ? (
        <p className={canOauthOnly ? "oauth-only" : "oauth-only error"}>
          {canOauthOnly
            ? `${PROVIDERS[found.oauth ?? "microsoft"].label} accounts like this one are added by signing in with ${PROVIDERS[found.oauth ?? "microsoft"].label}, above.`
            : "This address can only be added by signing in with Microsoft, which this version of Regards can't do yet."}
        </p>
      ) : (
        <>
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
            {found?.password_help && <span className="password-help">{found.password_help}</span>}
          </label>
          <details
            className="server-settings"
            open={serversOpen}
            onToggle={(event) => setServersOpen((event.currentTarget as HTMLDetailsElement).open)}
          >
            <summary>Server settings</summary>
            <fieldset>
              <legend>Incoming mail (IMAP, TLS)</legend>
              <label className="grow">
                Server
                <input
                  className="input"
                  placeholder="mail.example.org"
                  value={input.imapHost}
                  onChange={(e) => setServer("imapHost", e.target.value)}
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
                  value={input.imapPort}
                  onChange={(e) => setServer("imapPort", Number(e.target.value))}
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
                  onChange={(e) => setServer("smtpHost", e.target.value)}
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
                  onChange={(e) => setServer("smtpPort", Number(e.target.value))}
                />
              </label>
              <label className="security">
                Security
                <select
                  className="input"
                  value={input.smtpSecurity}
                  onChange={(e) => {
                    const security = e.target.value as ConnectionSecurity;
                    edited.current.add("smtpSecurity");
                    edited.current.add("smtpPort");
                    setInput((current) => ({ ...current, smtpSecurity: security, smtpPort: security === "tls" ? 465 : 587 }));
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
                onChange={(e) => set("username", e.target.value === "" ? null : e.target.value)}
                autoComplete="username"
                spellCheck={false}
              />
            </label>
          </details>
          <p aria-live="polite" className={error ? "form-status error" : "form-status"}>
            {checking ? "Signing in to check the details…" : error}
          </p>
        </>
      )}
      <div className="dialog-actions">
        {onCancel && (
          <button type="button" className="btn" onClick={onCancel} disabled={checking}>
            Cancel
          </button>
        )}
        {!oauthOnly && (
          <button type="submit" className="btn primary" disabled={checking || lookingUp}>
            Add account
          </button>
        )}
      </div>
    </form>
  );
}
