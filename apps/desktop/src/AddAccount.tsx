import {
  type FocusEvent,
  type FormEvent,
  type KeyboardEvent,
  type ReactNode,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import {
  type AccountCheckDetails,
  AccountCheckError,
  type AccountInput,
  type AccountSummary,
  type ConnectionSecurity,
  type Discovered,
  type Provider,
  type ServerCheck,
  type ServerSettings,
  addAccount,
  checkServer,
  discoverAccount,
  oauthProviders,
  updateAccount,
} from "./accounts";
import { AppMark } from "./AppMark";
import { useDialogFocus } from "./dialogFocus";
import { Icon } from "./icons";
import { OauthWaiting, PROVIDERS, useOauth } from "./OauthSignIn";
import { useShortcuts } from "./shortcuts";
import { t } from "./i18n";

/** An address worth looking up: something@domain.tld. */
const COMPLETE = /^[^\s@]+@[^\s@]+\.[^\s@]{2,}$/;
/** How long typing pauses before the address is looked up. */
const LOOKUP_PAUSE_MS = 700;

/** Why adding failed: the server's own words, and what it refused (none when no server was asked). */
interface Failure {
  message: string;
  check: AccountCheckDetails | null;
}

const failureOf = (error: unknown): Failure =>
  error instanceof AccountCheckError ? { message: error.message, check: error.check } : { message: String(error), check: null };

/**
 * One question at a time:
 * 1 the address; 2 the password, or the provider's sign-in, or the servers
 * when none were found; 3 checking; 4 what to do when it fails; 5 ready,
 * with the name people see asked last.
 */
type Step =
  | { kind: "address" }
  | { kind: "password"; askUsername?: boolean }
  | { kind: "provider"; provider: Provider }
  | { kind: "servers"; then: "password" | "check"; mark?: "imap" | "smtp" }
  | { kind: "checking" }
  | { kind: "recover"; failure: Failure }
  | { kind: "ready"; account: AccountSummary; sendChecked: boolean };

/** Where a step sits in the three dots. */
const DOT: Record<Step["kind"], 1 | 2 | 3> = {
  address: 1,
  password: 2,
  provider: 2,
  servers: 2,
  checking: 3,
  recover: 3,
  ready: 3,
};

/**
 * Adding an email account, on the first run and in Settings › Accounts. The
 * address decides the way in: a provider's own sign-in (Microsoft, Google)
 * where this version has it, otherwise a password; the servers are asked
 * for only when none could be found. Nothing is saved until the sign-in
 * worked.
 */
export function AddAccount({
  onDone,
  onCancel,
  onClose,
  first = false,
}: {
  /** Finished: with the account added, unless it was given up. */
  onDone: (account?: AccountSummary) => void;
  /** Gives up before an account was added (Settings). */
  onCancel?: () => void;
  /** In a window over the mail: ✕ and Esc close it, except while signing in. */
  onClose?: () => void;
  /** The first run: "Open Inbox" ends it. */
  first?: boolean;
}) {
  const [step, setStep] = useState<Step>({ kind: "address" });
  const [history, setHistory] = useState<Step[]>([]);
  const [email, setEmail] = useState("");
  const [found, setFound] = useState<Discovered | null>(null);
  const [looking, setLooking] = useState(false);
  const [providers, setProviders] = useState<Provider[]>([]);
  const [input, setInput] = useState<AccountInput>(blank(""));
  // Server fields the person typed: the lookup leaves them alone.
  const typed = useRef(new Set<keyof AccountInput>());
  // The last address looked up, and its lookup.
  const asked = useRef<{ address: string; result: Promise<Discovered | null> } | null>(null);
  // The address the fields were filled for.
  const filledFor = useRef("");

  useEffect(() => {
    oauthProviders().then(setProviders, () => setProviders([]));
  }, []);

  const oauth = useOauth((account) => {
    setHistory([]);
    setStep({ kind: "ready", account, sendChecked: true });
  });

  const go = (next: Step) => {
    setHistory((now) => [...now, step]);
    setStep(next);
  };
  const back = () => {
    if (oauth.flow) oauth.cancel();
    const previous = history[history.length - 1];
    if (!previous) return onCancel?.();
    setHistory((now) => now.slice(0, -1));
    setStep(previous);
  };
  const changeAddress = () => {
    if (oauth.flow) oauth.cancel();
    setHistory([]);
    setStep({ kind: "address" });
  };
  const restart = () => {
    changeAddress();
    setEmail("");
    setFound(null);
    asked.current = null;
    filledFor.current = "";
    typed.current = new Set();
    setInput(blank(""));
  };

  // Another address: another domain starts the fields afresh; another
  // address at the same one keeps the servers typed, not the sign-in.
  const startFor = (clean: string): ((now: AccountInput) => AccountInput) => {
    const before = filledFor.current;
    filledFor.current = clean;
    if (before === clean) return (now) => now;
    if (domainOf(before) !== domainOf(clean)) {
      typed.current = new Set();
      return () => blank(clean);
    }
    typed.current.delete("username");
    return (now) => ({ ...now, email: clean, username: null, password: "" });
  };

  // The address is looked up after a pause in typing, and on leaving the
  // field; asked again while it is under way, the same lookup answers.
  // Null when another address was asked for meanwhile.
  const lookUp = useCallback((address: string): Promise<Discovered | null> => {
    const clean = address.trim();
    if (!COMPLETE.test(clean)) return Promise.resolve(null);
    if (asked.current?.address === clean) return asked.current.result;
    const current = () => asked.current?.address === clean;
    const result = (async () => {
      setLooking(true);
      try {
        const discovered = await discoverAccount(clean);
        if (!current()) return null;
        setFound(discovered);
        const start = startFor(clean);
        setInput((now) => fill(start(now), discovered, typed.current));
        return discovered;
      } catch {
        if (!current()) return null;
        const none: Discovered = { source: null, imap: null, smtp: null, oauth_only: false };
        setFound(none);
        setInput(startFor(clean));
        return none;
      } finally {
        if (current()) setLooking(false);
      }
    })();
    asked.current = { address: clean, result };
    return result;
  }, []);
  useEffect(() => {
    if (!COMPLETE.test(email.trim())) return;
    const timer = window.setTimeout(() => void lookUp(email), LOOKUP_PAUSE_MS);
    return () => window.clearTimeout(timer);
  }, [email, lookUp]);

  const known = found && asked.current?.address === email.trim() ? found : null;
  const offered = (result: Discovered | null) => (result?.oauth && providers.includes(result.oauth) ? result.oauth : null);
  const provider = offered(known);
  // No way in: the provider's own sign-in only, which this version lacks, or no mail app at all.
  const shut = (result: Discovered) => (result.oauth_only && !offered(result)) || Boolean(result.no_imap);
  const blocked = known !== null && shut(known);

  const proceed = async () => {
    const clean = email.trim();
    const result = known ?? (await lookUp(clean));
    if (!result || shut(result)) return;
    setInput((now) => ({ ...now, email: clean }));
    const way = offered(result);
    if (way) go({ kind: "provider", provider: way });
    else if (result.source === null || !result.imap) go({ kind: "servers", then: "password" });
    else go({ kind: "password" });
  };

  // Signing in: the daemon checks receiving and sending, and the password,
  // before it saves anything. "Add anyway" skips the sending check.
  const signIn = async (skipSendCheck = false) => {
    go({ kind: "checking" });
    try {
      const account = await addAccount({ ...input, skipSendCheck });
      setHistory([]);
      setStep({ kind: "ready", account, sendChecked: !skipSendCheck });
    } catch (error) {
      setStep({ kind: "recover", failure: failureOf(error) });
    }
  };

  const host = input.imapHost || known?.imap?.host || "";
  // The provider's name, while its servers are the ones in use.
  const named = known?.provider_name && (!known.imap || !input.imapHost || input.imapHost === known.imap.host) ? known.provider_name : "";
  const chip = (
    <AddressChip
      email={email.trim()}
      detail={provider && step.kind === "provider" ? PROVIDERS[provider].label : named || host}
      onChange={changeAddress}
    />
  );

  let body: ReactNode;
  switch (step.kind) {
    case "address":
      body = (
        <form
          className="signup-body"
          onSubmit={(event: FormEvent) => {
            event.preventDefault();
            void proceed();
          }}
        >
          <div className="signup-titles">
            <h1>{first ? t("Add your email") : onClose ? t("Add another account") : t("Add an account")}</h1>
            <p className="muted">
              {onClose
                ? t("The same steps as the first time. Your other accounts keep working meanwhile.")
                : t("Just the address for now. Regards works out your provider and its servers.")}
            </p>
          </div>
          <label className="signup-field">
            {t("Email address")}
            <input
              className="input big"
              type="email"
              value={email}
              autoFocus
              autoComplete="email"
              placeholder="ada@example.org"
              onChange={(event) => setEmail(event.target.value)}
              onBlur={(event) => void lookUp(event.target.value)}
            />
          </label>
          <Found email={email.trim()} found={known} looking={looking} provider={provider} blocked={blocked} />
          {onClose ? (
            <>
              <div className="signup-buttons">
                <button type="button" className="btn big" onClick={onClose}>
                  {t("Cancel")}
                </button>
                <button type="submit" className="btn primary big" disabled={!COMPLETE.test(email.trim()) || blocked}>
                  {t("Continue")}
                </button>
              </div>
              <p className="signup-foot center muted">{t("Esc or ✕ closes it; nothing is saved until the last step.")}</p>
            </>
          ) : (
            <>
              <button type="submit" className="btn primary big" disabled={!COMPLETE.test(email.trim()) || blocked}>
                {t("Continue")}
              </button>
              <p className="signup-foot muted">
                <Icon name="lock" size={14} />
                {t("Your password and your mail stay on this computer.")}
              </p>
            </>
          )}
        </form>
      );
      break;
    case "password":
      body = (
        <form
          className="signup-body"
          onSubmit={(event: FormEvent) => {
            event.preventDefault();
            if (input.password) void signIn();
          }}
        >
          {chip}
          <h1>{named ? t("Your {provider} password", { provider: named }) : t("Your password")}</h1>
          {step.askUsername && (
            <label className="signup-field">
              {t("User name")}
              <input
                className="input big"
                value={input.username ?? ""}
                autoFocus
                spellCheck={false}
                placeholder={email.trim()}
                onChange={(event) => setInput({ ...input, username: event.target.value || null })}
              />
              <span className="muted small">{t("A login or customer number from your host's welcome email, for example.")}</span>
            </label>
          )}
          <PasswordField
            value={input.password}
            autoFocus={!step.askUsername}
            onChange={(password) => setInput({ ...input, password })}
          />
          {known?.password_help && (
            <div className="signup-tip">
              <Icon name="info" size={16} />
              <span>{known.password_help}</span>
            </div>
          )}
          <p className="signup-foot muted">
            <Icon name="lock" size={14} />
            <span>
              {host ? t("Connects securely to {host}", { host }) : t("Connects securely")} ·{" "}
              <button type="button" className="link" onClick={() => go({ kind: "servers", then: "password" })}>
                {t("Change servers")}
              </button>
            </span>
          </p>
          <button type="submit" className="btn primary big" disabled={!input.password}>
            {t("Sign in")}
          </button>
        </form>
      );
      break;
    case "provider": {
      const label = PROVIDERS[step.provider].label;
      body = (
        <div className="signup-body">
          {chip}
          <div className="signup-titles">
            <h1>{t("Sign in with")} {label}</h1>
            <p className="muted">
              {t("Your browser opens")} {label}{t("'s own page. You sign in there, and Regards never sees your password.")}
            </p>
          </div>
          {oauth.flow ? (
            <OauthWaiting flow={oauth.flow} onCancel={oauth.cancel} />
          ) : (
            <>
              <ol className="signup-steps">
                <li>
                  <span>1</span>
                  <span>
                    {t("Pick")} <strong>{email.trim()}</strong> {t("in the browser")}
                  </span>
                </li>
                <li>
                  <span>2</span>
                  <span>{t("Allow Regards to read and send your mail")}</span>
                </li>
                <li>
                  <span>3</span>
                  <span>{t("Come back here: the rest is automatic")}</span>
                </li>
              </ol>
              {oauth.error && (
                <p className="form-status error" role="alert">
                  {oauth.error}
                </p>
              )}
              <button type="button" className="btn primary big" onClick={() => void oauth.start(step.provider, email.trim())}>
                {t("Open {provider} sign-in", { provider: label })}
                <Icon name="external" size={16} />
              </button>
            </>
          )}
          {!known?.oauth_only && !oauth.flow && (
            <p className="signup-alt muted">
              {t("Your workplace blocks this?")}{" "}
              <button type="button" className="link" onClick={() => go({ kind: "password" })}>
                {t("Use an app password instead")}
              </button>
            </p>
          )}
        </div>
      );
      break;
    }
    case "servers":
      body = (
        <ServersForm
          email={email.trim()}
          notFound={known?.source === null}
          input={input}
          mark={step.mark}
          onChange={(next, field) => {
            typed.current.add(field);
            setInput(next);
          }}
          onSecurity={(security) => {
            typed.current.add("smtpSecurity");
            setInput((now) => ({
              ...now,
              smtpSecurity: security,
              // The usual port, unless one was typed.
              smtpPort: typed.current.has("smtpPort") ? now.smtpPort : security === "tls" ? 465 : 587,
            }));
          }}
          onSending={(settings) => {
            typed.current.add("smtpPort");
            typed.current.add("smtpSecurity");
            setInput((now) => ({ ...now, smtpPort: settings.port, smtpSecurity: settings.security }));
          }}
          onContinue={() => (step.then === "check" && input.password ? void signIn() : go({ kind: "password" }))}
        />
      );
      break;
    case "checking":
      body = (
        <div className="signup-body">
          <div className="signup-titles">
            <h1>{t("Connecting {address}", { address: email.trim() })}</h1>
            <p className="muted">{t("A few seconds. Regards signs in to check everything before it saves anything.")}</p>
          </div>
          <ul className="signup-checks" aria-live="polite">
            <li className="done">
              <Icon name="check" size={13} />
              <span>{t("Found the servers")}</span>
              <span className="muted">{host}</span>
            </li>
            <li className="now">
              <span className="spinner small" aria-hidden="true" />
              <span>{t("Signing in to receive and send")}</span>
              <span className="muted">as {input.username || email.trim()}</span>
            </li>
          </ul>
        </div>
      );
      break;
    case "recover":
      body = (
        <Recover
          failure={step.failure}
          email={email.trim()}
          host={host}
          named={named}
          found={known}
          provider={provider}
          onPassword={() => {
            setInput({ ...input, password: "" });
            setStep({ kind: "password" });
          }}
          onUsername={() => setStep({ kind: "password", askUsername: true })}
          onServers={(mark) => setStep({ kind: "servers", then: "check", mark })}
          onProvider={() => provider && setStep({ kind: "provider", provider })}
          onRetry={() => void signIn()}
          onAddAnyway={() => void signIn(true)}
        />
      );
      break;
    case "ready":
      body = <Ready account={step.account} sendChecked={step.sendChecked} first={first} onDone={onDone} onAnother={restart} />;
      break;
  }

  const backable = step.kind !== "address" && step.kind !== "checking" && step.kind !== "ready";
  // In a window: closed while nothing is saved yet; once added, it ends as "Done" would.
  // While the servers check the sign-in it stays, as the account may be about to be added.
  const close =
    onClose && step.kind !== "checking" ? (step.kind === "ready" ? () => onDone(step.account) : onClose) : undefined;
  const escape = (event: KeyboardEvent) => {
    if (event.key !== "Escape" || event.defaultPrevented || !onClose) return;
    event.preventDefault();
    close?.();
  };
  return (
    <section className="signup-card" aria-label={t("Add an email account")} onKeyDown={escape}>
      {step.kind !== "ready" && (
        <div className="signup-top">
          {backable || (step.kind === "address" && onCancel) ? (
            <button type="button" className="signup-back" onClick={step.kind === "recover" ? () => setStep({ kind: "password" }) : back}>
              <Icon name="back" size={16} />
              {step.kind === "address" ? t("Cancel") : t("Back")}
            </button>
          ) : onClose ? (
            <Dots at={DOT[step.kind]} />
          ) : (
            <AppMark size={40} />
          )}
          {(backable || !onClose) && <Dots at={DOT[step.kind]} />}
        </div>
      )}
      {onClose && (
        <button
          type="button"
          className="icon-btn signup-close"
          aria-label={t("Close")}
          title={close ? t("Close (Esc)") : t("Signing in…")}
          disabled={!close}
          onClick={close}
        >
          <Icon name="close" size={17} />
        </button>
      )}
      {body}
    </section>
  );
}

/**
 * Adding another account from the main screen (the ⌄ menu, the sidebar,
 * the composer's From menu): the same steps, in a window over the mail.
 * Other accounts keep syncing meanwhile.
 */
export function AddAccountDialog({ onClose, onAdded }: { onClose: () => void; onAdded: (account: AccountSummary) => void }) {
  const box = useRef<HTMLDivElement>(null);
  useDialogFocus(box);
  // Nothing behind it answers keys while it is open.
  useShortcuts({}, { modal: true });
  return (
    <div className="backdrop">
      <div ref={box} tabIndex={-1} className="signup-dialog" role="dialog" aria-modal="true" aria-label={t("Add another account")}>
        <AddAccount onClose={onClose} onDone={(account) => (account ? onAdded(account) : onClose())} />
      </div>
    </div>
  );
}

const domainOf = (address: string) => address.split("@")[1]?.toLowerCase() ?? "";

/** A fresh form for `email`. */
function blank(email: string): AccountInput {
  return {
    displayName: "",
    email,
    username: null,
    password: "",
    imapHost: "",
    imapPort: 993,
    smtpHost: "",
    smtpPort: 587,
    smtpSecurity: "starttls",
  };
}

/** What the lookup found, put in the fields the person has not typed in. Receiving is TLS only. */
export function fill(input: AccountInput, found: Discovered, typed: Set<keyof AccountInput>): AccountInput {
  const next = { ...input };
  const set = <K extends keyof AccountInput>(key: K, value: AccountInput[K]) => {
    if (!typed.has(key)) next[key] = value;
  };
  if (found.imap?.security === "tls") {
    set("imapHost", found.imap.host);
    set("imapPort", found.imap.port);
  }
  if (found.smtp) {
    set("smtpHost", found.smtp.host);
    set("smtpPort", found.smtp.port);
    set("smtpSecurity", found.smtp.security);
  }
  return next;
}

function Dots({ at }: { at: 1 | 2 | 3 }) {
  return (
    <span className="signup-dots" role="img" aria-label={`Step ${at} of 3`}>
      {[1, 2, 3].map((dot) => (
        <span key={dot} className={dot === at ? "on now" : dot < at ? "on" : ""} />
      ))}
    </span>
  );
}

function AddressChip({ email, detail, onChange }: { email: string; detail: string; onChange: () => void }) {
  return (
    <div className="signup-chip">
      <span className="signup-initial" aria-hidden="true">
        {email.charAt(0).toUpperCase()}
      </span>
      <span className="signup-chip-text">
        <strong>{email}</strong>
        {detail && <span className="muted">{detail}</span>}
      </span>
      <button type="button" className="link" onClick={onChange}>
        {t("Change")}
      </button>
    </div>
  );
}

/** Under the address: what the lookup found, in a line. */
function Found({
  email,
  found,
  looking,
  provider,
  blocked,
}: {
  email: string;
  found: Discovered | null;
  looking: boolean;
  provider: Provider | null;
  blocked: boolean;
}) {
  const domain = email.split("@")[1] ?? "";
  if (looking) {
    return (
      <p className="signup-found" role="status">
        <span className="spinner small" aria-hidden="true" />
        {t("Looking up {domain}…", { domain })}
      </p>
    );
  }
  if (!found) return null;
  if (found.no_imap) {
    return (
      <p className="signup-found bad" role="alert">
        <Icon name="alert" size={15} />
        {found.no_imap}
      </p>
    );
  }
  if (blocked) {
    return (
      <p className="signup-found bad" role="alert">
        <Icon name="alert" size={15} />
        {t("This address signs in with")} {PROVIDERS[found.oauth ?? "microsoft"].label} {t("only, which this version of Regards can't do yet.")}
      </p>
    );
  }
  const [tone, text]: readonly ["good" | "warn", ReactNode] =
    provider !== null
      ? ["good", t("{provider} · you sign in in your browser", { provider: PROVIDERS[provider].label })]
      : found.source !== null && found.provider_name
        ? [
            "good",
            <>
              <strong>{found.provider_name}</strong> {t("· settings found")}
            </>,
          ]
        : found.source === null
          ? ["warn", t("We couldn't find {domain}'s servers: you'll type them next", { domain })]
          : found.source === "guess"
            ? ["good", t("Likely settings for {domain}: signing in checks them", { domain })]
            : // Listed, from the domain's own settings file, its MX or DNS records.
              ["good", t("Settings found for {domain}", { domain })];
  return (
    <p className={`signup-found ${tone}`} role="status">
      <Icon name={tone === "good" ? "check" : "info"} size={15} />
      <span>{text}</span>
    </p>
  );
}

function PasswordField({ value, autoFocus, onChange }: { value: string; autoFocus: boolean; onChange: (value: string) => void }) {
  const [shown, setShown] = useState(false);
  return (
    <label className="signup-field">
      {t("Password")}
      <span className="signup-secret">
        <input
          className="input big"
          type={shown ? "text" : "password"}
          value={value}
          autoFocus={autoFocus}
          autoComplete="current-password"
          onChange={(event) => onChange(event.target.value)}
        />
        <button
          type="button"
          className="icon-btn"
          aria-label={shown ? t("Hide the password") : t("Show the password")}
          aria-pressed={shown}
          onClick={() => setShown(!shown)}
        >
          <Icon name={shown ? "eyeOff" : "eye"} size={17} />
        </button>
      </span>
    </label>
  );
}

type Server = "imap" | "smtp";
/** A server's check: under way, or what it found, for the settings it was made with. */
type Checked = { key: string } & ({ state: "checking" } | ({ state: "done" } & ServerCheck));

const keyOf = (settings: ServerSettings) => `${settings.host.toLowerCase()}:${settings.port}:${settings.security}`;

/**
 * Each server is checked as its fields are left, and once on opening when
 * filled in: does it answer, securely. Nothing signs in; a check that
 * cannot be made says nothing.
 */
function useServerChecks() {
  const [checks, setChecks] = useState<Partial<Record<Server, Checked>>>({});
  const asked = useRef<Partial<Record<Server, string>>>({});
  const check = useCallback((server: Server, settings: ServerSettings) => {
    if (!settings.host || !settings.port) return;
    const key = keyOf(settings);
    if (asked.current[server] === key) return;
    asked.current[server] = key;
    setChecks((now) => ({ ...now, [server]: { key, state: "checking" } }));
    checkServer(server, settings).then(
      (result) => {
        if (asked.current[server] === key) setChecks((now) => ({ ...now, [server]: { key, state: "done", ...result } }));
      },
      () => {
        if (asked.current[server] !== key) return;
        asked.current[server] = undefined;
        setChecks((now) => ({ ...now, [server]: undefined }));
      },
    );
  }, []);
  // Settings already known to answer, such as a suggestion taken.
  const answered = useCallback((server: Server, settings: ServerSettings) => {
    const key = keyOf(settings);
    asked.current[server] = key;
    setChecks((now) => ({ ...now, [server]: { key, state: "done" } }));
  }, []);
  /** The check for these settings, if it is theirs. */
  const of = (server: Server, settings: ServerSettings) => {
    const found = checks[server];
    return found && found.key === keyOf(settings) ? found : null;
  };
  return { check, answered, of };
}

function ServersForm({
  email,
  notFound,
  input,
  mark,
  onChange,
  onSecurity,
  onSending,
  onContinue,
}: {
  email: string;
  notFound: boolean;
  input: AccountInput;
  /** A server sign-in could not reach. */
  mark?: Server;
  onChange: (input: AccountInput, field: keyof AccountInput) => void;
  onSecurity: (security: ConnectionSecurity) => void;
  /** Other settings for sending, on the same server: a check's suggestion. */
  onSending: (settings: ServerSettings) => void;
  onContinue: () => void;
}) {
  const field = <K extends keyof AccountInput>(key: K, value: AccountInput[K]) => onChange({ ...input, [key]: value }, key);
  const settings: Record<Server, ServerSettings> = {
    imap: { host: input.imapHost.trim(), port: input.imapPort, security: "tls" },
    smtp: { host: input.smtpHost.trim() || input.imapHost.trim(), port: input.smtpPort, security: input.smtpSecurity },
  };
  const checks = useServerChecks();
  // Filled in already (found, or back to change them): checked on opening.
  useEffect(() => {
    checks.check("imap", settings.imap);
    checks.check("smtp", settings.smtp);
    // Only on opening; after that, as each server's fields are left.
  }, []);
  const leaving = (server: Server) => (event: FocusEvent<HTMLFieldSetElement>) => {
    if (event.relatedTarget instanceof Node && event.currentTarget.contains(event.relatedTarget)) return;
    checks.check(server, settings[server]);
  };
  const state = (server: Server) => {
    const found = checks.of(server, settings[server]);
    const failed = found?.state === "done" && found.reason !== undefined;
    const fine = found?.state === "done" && found.reason === undefined;
    return { found, className: ["signup-server", failed ? "failed" : "", mark === server && !fine ? "marked" : ""].filter(Boolean).join(" ") };
  };
  const imap = state("imap");
  const smtp = state("smtp");
  return (
    <form
      className="signup-body"
      onSubmit={(event: FormEvent) => {
        event.preventDefault();
        if (input.imapHost.trim()) onContinue();
      }}
    >
      {notFound && (
        <div className="signup-chip warn">
          <span className="signup-initial" aria-hidden="true">
            <Icon name="search" size={15} />
          </span>
          <span className="signup-chip-text">
            <strong>{email}</strong>
            <span>{t("We couldn't find this host's servers on our own")}</span>
          </span>
        </div>
      )}
      <div className="signup-titles">
        <h1>{t("Your mail servers")}</h1>
        <p className="muted">
          {t("Your host's help pages or welcome email list them. Type the names; Regards fills in the usual ports and checks each server as you go.")}
        </p>
      </div>
      <fieldset className={imap.className} onBlur={leaving("imap")}>
        <legend>
          {t("Receiving")} <span className="muted">{t("· IMAP")}</span>
          <span className="signup-always muted">
            <Icon name="lock" size={12} />
            {t("Always encrypted (TLS)")}
          </span>
        </legend>
        <div className="signup-row">
          <label className="signup-field grow">
            {t("Server")}
            <input
              className="input"
              value={input.imapHost}
              required
              autoFocus={mark !== "smtp"}
              spellCheck={false}
              placeholder={`imap.${email.split("@")[1] ?? "example.org"}`}
              onChange={(event) => field("imapHost", event.target.value)}
            />
          </label>
          <label className="signup-field port">
            {t("Port")}
            <input
              className="input"
              inputMode="numeric"
              value={input.imapPort}
              onChange={(event) => field("imapPort", Number(event.target.value) || 0)}
            />
          </label>
        </div>
        <CheckLine server="imap" settings={settings.imap} found={imap.found} />
      </fieldset>
      <fieldset className={smtp.className} onBlur={leaving("smtp")}>
        <legend>
          {t("Sending")} <span className="muted">{t("· SMTP")}</span>
        </legend>
        <div className="signup-row">
          <label className="signup-field grow">
            {t("Server")}
            <input
              className="input"
              value={input.smtpHost}
              autoFocus={mark === "smtp"}
              spellCheck={false}
              placeholder={t("same as receiving")}
              onChange={(event) => field("smtpHost", event.target.value)}
            />
          </label>
          <label className="signup-field port">
            {t("Port")}
            <input
              className="input"
              inputMode="numeric"
              value={input.smtpPort}
              onChange={(event) => field("smtpPort", Number(event.target.value) || 0)}
            />
          </label>
        </div>
        <div className="signup-segmented" role="radiogroup" aria-label={t("Encryption for sending")}>
          {(["starttls", "tls"] as const).map((security) => (
            <button
              key={security}
              type="button"
              role="radio"
              aria-checked={input.smtpSecurity === security}
              onClick={() => onSecurity(security)}
            >
              <strong>{security === "tls" ? t("TLS") : t("STARTTLS")}</strong>
              <span className="muted">{t("· port")} {security === "tls" ? 465 : 587}</span>
            </button>
          ))}
        </div>
        <CheckLine
          server="smtp"
          settings={settings.smtp}
          found={smtp.found}
          onUse={(suggested) => {
            onSending(suggested);
            checks.answered("smtp", suggested);
          }}
        />
      </fieldset>
      <label className="signup-field">
        {t("User name")}
        <input
          className="input"
          value={input.username ?? ""}
          spellCheck={false}
          placeholder={email}
          onChange={(event) => field("username", event.target.value || null)}
        />
        <span className="muted small">{t("Usually your full address. Change it only if your host gave you another one.")}</span>
      </label>
      <button type="submit" className="btn primary big" disabled={!input.imapHost.trim()}>
        {t("Continue")}
      </button>
    </form>
  );
}

const securityName = (security: ConnectionSecurity) => (security === "tls" ? "TLS" : "STARTTLS");

/** Under a server's fields: being checked, answers, or what doesn't, with the settings that do when known. */
function CheckLine({
  server,
  settings,
  found,
  onUse,
}: {
  server: Server;
  settings: ServerSettings;
  found: Checked | null;
  onUse?: (settings: ServerSettings) => void;
}) {
  if (!found) return null;
  if (found.state === "checking") {
    return (
      <p className="signup-check" role="status">
        <span className="spinner small" aria-hidden="true" />
        {t("Checking {host}…", { host: settings.host })}
      </p>
    );
  }
  if (found.reason === undefined) {
    return (
      <p className="signup-check good" role="status">
        <Icon name="check" size={14} />
        {t("It answers, securely")}
      </p>
    );
  }
  const port = settings.port;
  const what =
    found.reason === "tls"
      ? t("No secure connection on port {port}", { port })
      : found.reason === "not_imap"
        ? server === "imap"
          ? t("What answers on port {port} isn't a server for receiving mail", { port })
          : t("What answers on port {port} isn't a server for sending mail", { port })
        : found.reason === "protocol"
          ? t("The server on port {port} answered in a way Regards didn't expect", { port })
          : t("Nothing answered on port {port}", { port });
  const suggestion = found.suggestion;
  return (
    <div className="signup-check bad" role="alert">
      {suggestion ? (
        <span>
          {what}{t(", but this server answers on")}{" "}
          <strong>
            {t("{port} with {security}", { port: suggestion.port, security: securityName(suggestion.security) })}
          </strong>
          .
        </span>
      ) : (
        <span>
          {what}. {found.reason === "tls" ? t("Check the port.") : t("Check the server name and port.")}
        </span>
      )}
      {suggestion && onUse && (
        <button type="button" className="btn small" onClick={() => onUse(suggestion)}>
          {t("Use {port}", { port: suggestion.port })}
        </button>
      )}
    </div>
  );
}

/** A way forward from a failed sign-in. */
function Choice({ icon, title, detail, main = false, onClick }: { icon: Parameters<typeof Icon>[0]["name"]; title: string; detail: ReactNode; main?: boolean; onClick: () => void }) {
  return (
    <button type="button" className={main ? "signup-choice main" : "signup-choice"} onClick={onClick}>
      <Icon name={icon} size={20} />
      <span>
        <strong>{title}</strong>
        <span className="muted">{detail}</span>
      </span>
    </button>
  );
}

/** What went wrong, in words, and what can be done about it: one screen per reason and server. */
function Recover({
  failure,
  email,
  host,
  named,
  found,
  provider,
  onPassword,
  onUsername,
  onServers,
  onProvider,
  onRetry,
  onAddAnyway,
}: {
  failure: Failure;
  email: string;
  host: string;
  /** The provider's name, when discovery knows it. */
  named: string;
  found: Discovered | null;
  provider: Provider | null;
  onPassword: () => void;
  onUsername: () => void;
  onServers: (mark?: "imap" | "smtp") => void;
  onProvider: () => void;
  onRetry: () => void;
  onAddAnyway: () => void;
}) {
  const check = failure.check;
  const sending = check?.server === "smtp";
  const where = host || email.split("@")[1] || t("the server");
  // Who turned the password down: the provider, by name, where known.
  const who = named || where;
  const tried = check?.tried?.length ? check.tried : [email];
  let title: string;
  let lead: ReactNode;
  let choices: ReactNode;
  switch (check?.reason) {
    case "auth_failed":
      title = sending ? t("The sending server didn't accept the password") : t("{who} didn't accept the password", { who });
      lead = (
        <>
          {sending
            ? t("Receiving works; signing in to send failed. ")
            : t("The server was found and the connection is secure; only the sign-in failed. ")}
          {t("We tried")}{" "}
          {tried.map((name, at) => (
            <span key={name}>
              {at > 0 && (at === tried.length - 1 ? ` ${t("and")} ` : ", ")}
              <strong>{name}</strong>
            </span>
          ))}{" "}
          {t("as the user name.")}
        </>
      );
      choices = (
        <>
          <Choice main icon="refresh" title={t("Type the password again")} detail={t("Show the password to check it.")} onClick={onPassword} />
          <Choice
            icon="lock"
            title={t("Use a password made for Regards")}
            detail={
              found?.password_help ??
              (named
                ? t("If two-step sign-in is on, {provider} needs an app password.", { provider: named })
                : t("If two-step sign-in is on, your provider needs an app password."))
            }
            onClick={onPassword}
          />
          {provider && (
            <Choice icon="user" title={t("Sign in with {provider} instead", { provider: PROVIDERS[provider].label })} detail={t("In your browser; no password here.")} onClick={onProvider} />
          )}
          <Choice icon="user" title={t("Sign in with another user name")} detail={t("Some hosts use a login or customer number from your welcome email.")} onClick={onUsername} />
        </>
      );
      break;
    case "login_disabled":
      title = t("{who} doesn't take a password here", { who });
      lead = t("The server turned password sign-in off for mail apps.");
      choices = (
        <>
          {provider && (
            <Choice main icon="user" title={t("Sign in with {provider}", { provider: PROVIDERS[provider].label })} detail={t("In your browser; no password here.")} onClick={onProvider} />
          )}
          <Choice
            main={!provider}
            icon="lock"
            title={t("Use a password made for Regards")}
            detail={found?.password_help ?? t("Your provider's settings can make one, or turn mail-app sign-in on.")}
            onClick={onPassword}
          />
        </>
      );
      break;
    case "tls":
      title = sending ? t("Sending isn't encrypted the way Regards needs") : t("The connection to {where} isn't secure", { where });
      lead = sending
        ? t("The sending server's encryption setting may be the other one: STARTTLS on 587, or TLS on 465.")
        : t("Regards only signs in over an encrypted connection, and this server didn't offer one that checks out.");
      choices = (
        <>
          <Choice main icon="settings" title={t("Check the servers")} detail={sending ? t("Try the other encryption setting.") : t("A name or port may be off.")} onClick={() => onServers(check.server)} />
          <Choice icon="refresh" title={t("Try again")} detail={t("On a public network, a second try may work.")} onClick={onRetry} />
        </>
      );
      break;
    case "connect":
    case "timeout":
    case "not_imap":
      title = sending ? t("The sending server didn't answer") : t("{where} didn't answer", { where });
      lead = sending
        ? t("Receiving works. Some networks block the ports for sending mail; the server name or port may also be off.")
        : t("The server name or port may be off, or the network may be down.");
      choices = (
        <>
          <Choice main icon="settings" title={t("Check the servers")} detail={t("See what Regards tried, and change it.")} onClick={() => onServers(check.server)} />
          <Choice icon="refresh" title={t("Try again")} detail={t("If the network was down for a moment.")} onClick={onRetry} />
        </>
      );
      break;
    default:
      title = t("The account couldn't be added");
      lead = check ? t("Something went wrong while signing in.") : failure.message;
      choices = (
        <>
          <Choice main icon="refresh" title={t("Try again")} detail={t("It may work on a second try.")} onClick={onRetry} />
          <Choice icon="settings" title={t("Check the servers")} detail={t("See what Regards tried, and change it.")} onClick={() => onServers(check?.server)} />
        </>
      );
  }
  return (
    <div className="signup-body">
      <div className="signup-problem">
        <span aria-hidden="true">
          <Icon name={check?.reason === "auth_failed" || check?.reason === "login_disabled" ? "lock" : "alert"} size={18} />
        </span>
        <div>
          <h1>{title}</h1>
          <p className="muted">{lead}</p>
        </div>
      </div>
      <div className="signup-choices">
        {choices}
        {sending && (
          <Choice
            icon="check"
            title={t("Add it anyway")}
            detail={t("Receiving works. The first message you send then says what is wrong.")}
            onClick={onAddAnyway}
          />
        )}
      </div>
      {check && failure.message && (
        <details className="signup-said">
          <summary>{t("What the server said")}</summary>
          <code>{failure.message}</code>
        </details>
      )}
    </div>
  );
}

/** Added: the name people see, asked last and filled in. */
function Ready({
  account,
  sendChecked,
  first,
  onDone,
  onAnother,
}: {
  account: AccountSummary;
  sendChecked: boolean;
  first: boolean;
  onDone: (account: AccountSummary) => void;
  onAnother: () => void;
}) {
  const given = account.display_name && account.display_name !== account.email ? account.display_name : "";
  const [name, setName] = useState(given || guessName(account.email));
  const [saving, setSaving] = useState(false);
  const finish = (then: () => void) => {
    const wanted = name.trim();
    if (!wanted || wanted === account.display_name) return then();
    setSaving(true);
    updateAccount(account.id, wanted).then(then, then);
  };
  return (
    <form
      className="signup-body"
      onSubmit={(event: FormEvent) => {
        event.preventDefault();
        finish(() => onDone(account));
      }}
    >
      <div className="signup-done">
        <span aria-hidden="true">
          <Icon name="check" size={28} />
        </span>
        <h1>{account.email} {t("is ready")}</h1>
        <p className="muted">{sendChecked ? t("Receiving and sending both work.") : t("Receiving works. Sending wasn't checked.")}</p>
      </div>
      <label className="signup-field">
        {t("Name people see on your mail")}
        <input className="input big" value={name} autoFocus onChange={(event) => setName(event.target.value)} autoComplete="name" />
        <span className="muted small">
          {given ? `${t("From your {provider} account.", { provider: account.sign_in === "google" ? "Google" : "Microsoft" })} ` : ""}
          {t("You can change it any time in Settings.")}
        </span>
      </label>
      <button type="submit" className="btn primary big" disabled={saving}>
        {first ? t("Open Inbox") : t("Done")}
      </button>
      <button type="button" className="link signup-another" onClick={() => finish(onAnother)}>
        {t("Add another account")}
      </button>
    </form>
  );
}

/** "ada.lovelace@…" → "Ada Lovelace": a start for the name, to be checked. */
export function guessName(email: string): string {
  const local = email.split("@")[0] ?? "";
  return local
    .split(/[._-]+/)
    .filter((part) => /^[a-z]+$/i.test(part))
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1).toLowerCase())
    .join(" ");
}
