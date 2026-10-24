import { type FocusEvent, type FormEvent, type ReactNode, useCallback, useEffect, useRef, useState } from "react";
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
import { Icon } from "./icons";
import { OauthWaiting, PROVIDERS, useOauth } from "./OauthSignIn";

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
  first = false,
}: {
  /** Finished, with or without an account added. */
  onDone: () => void;
  /** Gives up before an account was added (Settings). */
  onCancel?: () => void;
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
            <h1>{first ? "Add your email" : "Add an account"}</h1>
            <p className="muted">Just the address for now. Regards works out your provider and its servers.</p>
          </div>
          <label className="signup-field">
            Email address
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
          <button type="submit" className="btn primary big" disabled={!COMPLETE.test(email.trim()) || blocked}>
            Continue
          </button>
          <p className="signup-foot muted">
            <Icon name="lock" size={14} />
            Your password and your mail stay on this computer.
          </p>
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
          <h1>{named ? `Your ${named} password` : "Your password"}</h1>
          {step.askUsername && (
            <label className="signup-field">
              User name
              <input
                className="input big"
                value={input.username ?? ""}
                autoFocus
                spellCheck={false}
                placeholder={email.trim()}
                onChange={(event) => setInput({ ...input, username: event.target.value || null })}
              />
              <span className="muted small">A login or customer number from your host's welcome email, for example.</span>
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
              {host ? `Connects securely to ${host}` : "Connects securely"} ·{" "}
              <button type="button" className="link" onClick={() => go({ kind: "servers", then: "password" })}>
                Change servers
              </button>
            </span>
          </p>
          <button type="submit" className="btn primary big" disabled={!input.password}>
            Sign in
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
            <h1>Sign in with {label}</h1>
            <p className="muted">
              Your browser opens {label}&apos;s own page. You sign in there, and Regards never sees your password.
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
                    Pick <strong>{email.trim()}</strong> in the browser
                  </span>
                </li>
                <li>
                  <span>2</span>
                  <span>Allow Regards to read and send your mail</span>
                </li>
                <li>
                  <span>3</span>
                  <span>Come back here: the rest is automatic</span>
                </li>
              </ol>
              {oauth.error && (
                <p className="form-status error" role="alert">
                  {oauth.error}
                </p>
              )}
              <button type="button" className="btn primary big" onClick={() => void oauth.start(step.provider, email.trim())}>
                Open {label} sign-in
                <Icon name="external" size={16} />
              </button>
            </>
          )}
          {!known?.oauth_only && !oauth.flow && (
            <p className="signup-alt muted">
              Your workplace blocks this?{" "}
              <button type="button" className="link" onClick={() => go({ kind: "password" })}>
                Use an app password instead
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
            <h1>Connecting {email.trim()}</h1>
            <p className="muted">A few seconds. Regards signs in to check everything before it saves anything.</p>
          </div>
          <ul className="signup-checks" aria-live="polite">
            <li className="done">
              <Icon name="check" size={13} />
              <span>Found the servers</span>
              <span className="muted">{host}</span>
            </li>
            <li className="now">
              <span className="spinner small" aria-hidden="true" />
              <span>Signing in to receive and send</span>
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
  return (
    <section className="signup-card" aria-label="Add an email account">
      {step.kind !== "ready" && (
        <div className="signup-top">
          {backable || (step.kind === "address" && onCancel) ? (
            <button type="button" className="signup-back" onClick={step.kind === "recover" ? () => setStep({ kind: "password" }) : back}>
              <Icon name="back" size={16} />
              {step.kind === "address" ? "Cancel" : "Back"}
            </button>
          ) : (
            <AppMark size={40} />
          )}
          <Dots at={DOT[step.kind]} />
        </div>
      )}
      {body}
    </section>
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
        Change
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
        Looking up {domain}…
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
        This address signs in with {PROVIDERS[found.oauth ?? "microsoft"].label} only, which this version of Regards can&apos;t do yet.
      </p>
    );
  }
  const [tone, text]: readonly ["good" | "warn", ReactNode] =
    provider !== null
      ? ["good", `${PROVIDERS[provider].label} · you sign in in your browser`]
      : found.source !== null && found.provider_name
        ? [
            "good",
            <>
              <strong>{found.provider_name}</strong> · settings found
            </>,
          ]
        : found.source === "known" || found.source === "mx" || found.source === "dns"
          ? ["good", `Settings found for ${domain}`]
          : found.source === "guess"
            ? ["good", `Likely settings for ${domain}: signing in checks them`]
            : ["warn", `We couldn't find ${domain}'s servers: you'll type them next`];
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
      Password
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
          aria-label={shown ? "Hide the password" : "Show the password"}
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
            <span>We couldn&apos;t find this host&apos;s servers on our own</span>
          </span>
        </div>
      )}
      <div className="signup-titles">
        <h1>Your mail servers</h1>
        <p className="muted">
          Your host&apos;s help pages or welcome email list them. Type the names; Regards fills in the usual ports and checks
          each server as you go.
        </p>
      </div>
      <fieldset className={imap.className} onBlur={leaving("imap")}>
        <legend>
          Receiving <span className="muted">· IMAP</span>
          <span className="signup-always muted">
            <Icon name="lock" size={12} />
            Always encrypted (TLS)
          </span>
        </legend>
        <div className="signup-row">
          <label className="signup-field grow">
            Server
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
            Port
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
          Sending <span className="muted">· SMTP</span>
        </legend>
        <div className="signup-row">
          <label className="signup-field grow">
            Server
            <input
              className="input"
              value={input.smtpHost}
              autoFocus={mark === "smtp"}
              spellCheck={false}
              placeholder="same as receiving"
              onChange={(event) => field("smtpHost", event.target.value)}
            />
          </label>
          <label className="signup-field port">
            Port
            <input
              className="input"
              inputMode="numeric"
              value={input.smtpPort}
              onChange={(event) => field("smtpPort", Number(event.target.value) || 0)}
            />
          </label>
        </div>
        <div className="signup-segmented" role="radiogroup" aria-label="Encryption for sending">
          {(["starttls", "tls"] as const).map((security) => (
            <button
              key={security}
              type="button"
              role="radio"
              aria-checked={input.smtpSecurity === security}
              onClick={() => onSecurity(security)}
            >
              <strong>{security === "tls" ? "TLS" : "STARTTLS"}</strong>
              <span className="muted">· port {security === "tls" ? 465 : 587}</span>
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
        User name
        <input
          className="input"
          value={input.username ?? ""}
          spellCheck={false}
          placeholder={email}
          onChange={(event) => field("username", event.target.value || null)}
        />
        <span className="muted small">Usually your full address. Change it only if your host gave you another one.</span>
      </label>
      <button type="submit" className="btn primary big" disabled={!input.imapHost.trim()}>
        Continue
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
        Checking {settings.host}…
      </p>
    );
  }
  if (found.reason === undefined) {
    return (
      <p className="signup-check good" role="status">
        <Icon name="check" size={14} />
        It answers, securely
      </p>
    );
  }
  const port = settings.port;
  const what =
    found.reason === "tls"
      ? `No secure connection on port ${port}`
      : found.reason === "not_imap"
        ? `What answers on port ${port} isn't a server for ${server === "imap" ? "receiving" : "sending"} mail`
        : found.reason === "protocol"
          ? `The server on port ${port} answered in a way Regards didn't expect`
          : `Nothing answered on port ${port}`;
  const suggestion = found.suggestion;
  return (
    <div className="signup-check bad" role="alert">
      {suggestion ? (
        <span>
          {what}, but this server answers on{" "}
          <strong>
            {suggestion.port} with {securityName(suggestion.security)}
          </strong>
          .
        </span>
      ) : (
        <span>
          {what}. {found.reason === "tls" ? "Check the port." : "Check the server name and port."}
        </span>
      )}
      {suggestion && onUse && (
        <button type="button" className="btn small" onClick={() => onUse(suggestion)}>
          Use {suggestion.port}
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
  const where = host || email.split("@")[1] || "the server";
  // Who turned the password down: the provider, by name, where known.
  const who = named || where;
  const tried = check?.tried?.length ? check.tried : [email];
  let title: string;
  let lead: ReactNode;
  let choices: ReactNode;
  switch (check?.reason) {
    case "auth_failed":
      title = sending ? "The sending server didn't accept the password" : `${who} didn't accept the password`;
      lead = (
        <>
          {sending
            ? "Receiving works; signing in to send failed. "
            : "The server was found and the connection is secure; only the sign-in failed. "}
          We tried{" "}
          {tried.map((name, at) => (
            <span key={name}>
              {at > 0 && (at === tried.length - 1 ? " and " : ", ")}
              <strong>{name}</strong>
            </span>
          ))}{" "}
          as the user name.
        </>
      );
      choices = (
        <>
          <Choice main icon="refresh" title="Type the password again" detail="Show the password to check it." onClick={onPassword} />
          <Choice
            icon="lock"
            title="Use a password made for Regards"
            detail={found?.password_help ?? `If two-step sign-in is on, ${named || "your provider"} needs an app password.`}
            onClick={onPassword}
          />
          {provider && (
            <Choice icon="user" title={`Sign in with ${PROVIDERS[provider].label} instead`} detail="In your browser; no password here." onClick={onProvider} />
          )}
          <Choice icon="user" title="Sign in with another user name" detail="Some hosts use a login or customer number from your welcome email." onClick={onUsername} />
        </>
      );
      break;
    case "login_disabled":
      title = `${who} doesn't take a password here`;
      lead = "The server turned password sign-in off for mail apps.";
      choices = (
        <>
          {provider && (
            <Choice main icon="user" title={`Sign in with ${PROVIDERS[provider].label}`} detail="In your browser; no password here." onClick={onProvider} />
          )}
          <Choice
            main={!provider}
            icon="lock"
            title="Use a password made for Regards"
            detail={found?.password_help ?? "Your provider's settings can make one, or turn mail-app sign-in on."}
            onClick={onPassword}
          />
        </>
      );
      break;
    case "tls":
      title = sending ? "Sending isn't encrypted the way Regards needs" : `The connection to ${where} isn't secure`;
      lead = sending
        ? "The sending server's encryption setting may be the other one: STARTTLS on 587, or TLS on 465."
        : "Regards only signs in over an encrypted connection, and this server didn't offer one that checks out.";
      choices = (
        <>
          <Choice main icon="settings" title="Check the servers" detail={sending ? "Try the other encryption setting." : "A name or port may be off."} onClick={() => onServers(check.server)} />
          <Choice icon="refresh" title="Try again" detail="On a public network, a second try may work." onClick={onRetry} />
        </>
      );
      break;
    case "connect":
    case "timeout":
    case "not_imap":
      title = sending ? "The sending server didn't answer" : `${where} didn't answer`;
      lead = sending
        ? "Receiving works. Some networks block the ports for sending mail; the server name or port may also be off."
        : "The server name or port may be off, or the network may be down.";
      choices = (
        <>
          <Choice main icon="settings" title="Check the servers" detail="See what Regards tried, and change it." onClick={() => onServers(check.server)} />
          <Choice icon="refresh" title="Try again" detail="If the network was down for a moment." onClick={onRetry} />
        </>
      );
      break;
    default:
      title = "The account couldn't be added";
      lead = check ? "Something went wrong while signing in." : failure.message;
      choices = (
        <>
          <Choice main icon="refresh" title="Try again" detail="It may work on a second try." onClick={onRetry} />
          <Choice icon="settings" title="Check the servers" detail="See what Regards tried, and change it." onClick={() => onServers(check?.server)} />
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
            title="Add it anyway"
            detail="Receiving works. The first message you send then says what is wrong."
            onClick={onAddAnyway}
          />
        )}
      </div>
      {check && failure.message && (
        <details className="signup-said">
          <summary>What the server said</summary>
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
  onDone: () => void;
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
        finish(onDone);
      }}
    >
      <div className="signup-done">
        <span aria-hidden="true">
          <Icon name="check" size={28} />
        </span>
        <h1>{account.email} is ready</h1>
        <p className="muted">{sendChecked ? "Receiving and sending both work." : "Receiving works. Sending wasn't checked."}</p>
      </div>
      <label className="signup-field">
        Name people see on your mail
        <input className="input big" value={name} autoFocus onChange={(event) => setName(event.target.value)} autoComplete="name" />
        <span className="muted small">
          {given ? `From your ${account.sign_in === "google" ? "Google" : "Microsoft"} account. ` : ""}You can change it any time in Settings.
        </span>
      </label>
      <button type="submit" className="btn primary big" disabled={saving}>
        {first ? "Open Inbox" : "Done"}
      </button>
      <button type="button" className="link signup-another" onClick={() => finish(onAnother)}>
        Add another account
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
