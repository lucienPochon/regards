import { useEffect, useRef, useState } from "react";
import { type AccountSummary, removeAccount, syncAccount, updateAccount } from "./accounts";
import { AgentActivity } from "./AgentActivity";
import { AgentConnect } from "./AgentConnect";
import { NotesSection, VoiceEditor, VoiceSection } from "./AgentVoice";
import type { Identity } from "./identities";
import { AgentsGate } from "./AgentsGate";
import { AgentHolds } from "./AgentHolds";
import { leafName } from "./views";
import { AGENT_ACCESS_LEVELS, AGENT_SEND_WINDOW, type AgentAccess, setAgentAccess, setAgentSend } from "./agents";
import { AccountForm } from "./AccountForm";
import { AccountLogin } from "./AccountLogin";
import { IdentitiesEditor } from "./IdentitiesEditor";
import { AssistantPage } from "./AssistantSettings";
import { OauthWaiting, PROVIDERS, useOauth } from "./OauthSignIn";
import { Avatar } from "./Avatar";
import type { ConfirmRequest } from "./Confirm";
import { Icon, type IconName } from "./icons";
import type { Layout } from "./layout";
import { type Screening, setScreening } from "./smart";
import { TemplatesEditor } from "./TemplatesEditor";
import type { AccountMail, MailState, ServiceCheck } from "./mail";
import { MOD_KEY, shortcutList, useShortcuts } from "./shortcuts";
import { ACCOUNT_COLOURS, COLOUR_NAMES, accountColour, setPrefs, usePrefs } from "./prefs";
import { Choice, LinkRow, Option, Select, Toggle } from "./SettingsControls";
import {
  AccessibilityPage,
  AppearancePage,
  ComposerPage,
  EmailGroupsPage,
  GeneralPage,
  NotificationsPage,
  PriorityRows,
  SchedulingPage,
  SupportPage,
} from "./SettingsPages";
import { useDialogFocus } from "./dialogFocus";

export type SettingsTab =
  | "accounts"
  | "agents"
  | "assistant"
  | "general"
  | "appearance"
  | "notifications"
  | "scheduling"
  | "inbox"
  | "composer"
  | "templates"
  | "accessibility"
  | "shortcuts"
  | "support";

/** Grouped as in the nav; each item has its own colour. Accounts is the person's card above them. */
const GROUPS: { id: SettingsTab; label: string; icon: IconName; tone: string }[][] = [
  [
    { id: "agents", label: "Agents", icon: "agent", tone: "tone-indigo" },
    { id: "assistant", label: "Assistant", icon: "bolt", tone: "tone-violet" },
  ],
  [
    { id: "general", label: "General", icon: "settings", tone: "tone-slate" },
    { id: "appearance", label: "Appearance", icon: "sun", tone: "tone-violet" },
    { id: "notifications", label: "Notifications", icon: "bell", tone: "tone-blue" },
    { id: "scheduling", label: "Scheduling", icon: "clock", tone: "tone-blue" },
  ],
  [
    { id: "inbox", label: "Inbox", icon: "inbox", tone: "tone-blue" },
    { id: "composer", label: "Composer", icon: "compose", tone: "tone-blue" },
    { id: "templates", label: "Templates", icon: "text", tone: "tone-rose" },
  ],
  [
    { id: "accessibility", label: "Accessibility", icon: "user", tone: "tone-green" },
    { id: "shortcuts", label: "Shortcuts", icon: "keyboard", tone: "tone-amber" },
  ],
  [{ id: "support", label: "Support", icon: "help", tone: "tone-green" }],
];

export function Settings({
  tab,
  onTab,
  mail,
  service,
  onRecheck,
  layout,
  onLayout,
  screening,
  onScreening,
  confirm,
  focusHold = null,
  onClose,
}: {
  tab: SettingsTab;
  onTab: (tab: SettingsTab) => void;
  mail: MailState;
  service: ServiceCheck;
  onRecheck: () => void;
  layout: Layout;
  onLayout: (change: Partial<Layout>) => void;
  screening: Screening;
  onScreening: (mode: Screening) => void;
  confirm: (request: ConfirmRequest) => Promise<boolean>;
  /** A hold a link asked to see (Agents). */
  focusHold?: number | null;
  onClose: () => void;
}) {
  useShortcuts({ Escape: onClose }, { modal: true });
  const sheet = useRef<HTMLElement>(null);
  useDialogFocus(sheet);
  const me = mail.state === "ready" ? mail.accounts[0]?.account : undefined;
  const accounts = mail.state === "ready" ? mail.accounts.map((entry) => entry.account) : [];
  return (
    <div className="backdrop sheet" onMouseDown={onClose}>
      <section
        ref={sheet}
        className="dialog settings"
        role="dialog"
        aria-modal="true"
        aria-label="Settings"
        tabIndex={-1}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <nav className="settings-nav" aria-label="Settings">
          {/* The person, on a card at the top: their accounts page. */}
          <button
            type="button"
            className={tab === "accounts" ? "settings-me active" : "settings-me"}
            aria-current={tab === "accounts" ? "page" : undefined}
            onClick={() => onTab("accounts")}
          >
            {me ? <Avatar person={{ name: me.display_name, address: me.email }} size="md" /> : <Icon name="user" />}
            <span>
              <strong>{me?.display_name || "Regards"}</strong>
              {me && <span className="muted">{me.email}</span>}
            </span>
          </button>
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
          {tab === "assistant" && <AssistantPage />}
          {tab === "general" && <GeneralPage accounts={accounts} />}
          {tab === "accounts" && <AccountsTab mail={mail} confirm={confirm} />}
          {tab === "agents" && <AgentsTab mail={mail} confirm={confirm} focusHold={focusHold} />}
          {tab === "inbox" && (
            <InboxTab layout={layout} onLayout={onLayout} screening={screening} onScreening={onScreening} accounts={accounts} />
          )}
          {tab === "templates" && <TemplatesTab />}
          {tab === "appearance" && <AppearancePage layout={layout} onLayout={onLayout} accounts={accounts} />}
          {tab === "notifications" && <NotificationsPage accounts={accounts} />}
          {tab === "scheduling" && <SchedulingPage />}
          {tab === "composer" && <ComposerPage accounts={accounts} confirm={confirm} onAccounts={() => onTab("accounts")} />}
          {tab === "accessibility" && <AccessibilityPage />}
          {tab === "shortcuts" && <ShortcutsTab />}
          {tab === "support" && <SupportPage service={service} onRecheck={onRecheck} accounts={accounts} confirm={confirm} />}
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
      {mail.state === "failed" && <p className="error" role="alert">{mail.message}</p>}
      {accounts.length > 0 && (
        <ul className="account-list">
          {accounts.map((entry) => (
            <AccountRow key={entry.account.id} entry={entry} confirm={confirm} many={accounts.length > 1} />
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

/** How long an agent's send may wait in the Outbox before it goes, when it needn't be approved (ADR-0012 §3). */
const SEND_WINDOWS = [10, 30, 60, 120, 300, 600].filter(
  (seconds) => seconds >= AGENT_SEND_WINDOW.min && seconds <= AGENT_SEND_WINDOW.max,
);
const windowLabel = (seconds: number) => (seconds < 60 ? `${seconds} s` : `${seconds / 60} min`);

/** Levels kept for each account but not yet giving agents more (ADR-0012); none since sends wait for approval. */
const COMING: ReadonlySet<AgentAccess> = new Set();

/**
 * What agents (AI assistants and other programs using Regards through its
 * command line) may do with each account. Only this page changes it; an
 * agent that is refused is sent here, by this name.
 */
function AgentsTab({
  mail,
  confirm,
  focusHold,
}: {
  mail: MailState;
  confirm: (request: ConfirmRequest) => Promise<boolean>;
  focusHold: number | null;
}) {
  const accounts = mail.state === "ready" ? mail.accounts : [];
  // What was chosen here, until the accounts are read again.
  const [chosen, setChosen] = useState<Record<string, AgentAccess>>({});
  // An address's voice, open in its own page.
  const [voiceFor, setVoiceFor] = useState<Identity | null>(null);
  const [error, setError] = useState<string | null>(null);
  const levelOf = (account: AccountSummary) => chosen[account.id] ?? account.agent_access ?? "off";
  // At Send: each send asked (null), or sent after this many seconds unless called back.
  const [windows, setWindows] = useState<Record<string, number | null>>({});
  const windowOf = (account: AccountSummary) =>
    account.id in windows ? windows[account.id] : (account.agent_send_window ?? null);

  const chooseWindow = async (account: AccountSummary, seconds: number | null) => {
    const before = windowOf(account);
    if (seconds !== null && before === null) {
      const ok = await confirm({
        title: `Let agents' sends from ${account.email} go without asking?`,
        body: `Each send then waits ${windowLabel(seconds)} in the Outbox, and you are told, so you can call it back. After that it goes. At most 20 an hour go this way; more wait for your approval.`,
        confirm: `Send after ${windowLabel(seconds)}`,
      });
      if (!ok) return;
    }
    setWindows((now) => ({ ...now, [account.id]: seconds }));
    setError(null);
    setAgentSend(account.id, seconds).then(
      (stored) => setWindows((now) => ({ ...now, [account.id]: stored })),
      (failure: unknown) => {
        setWindows((now) => ({ ...now, [account.id]: before }));
        setError(String(failure));
      },
    );
  };

  const choose = async (account: AccountSummary, access: AgentAccess) => {
    if (access === "send") {
      const ok = await confirm({
        title: `Let agents send from ${account.email}?`,
        body: "They could then send mail as you from this account. Each send waits for you to approve it, here in Settings › Agents, and nothing goes out until you do.",
        confirm: "Allow sending",
      });
      if (!ok) return;
    }
    const before = levelOf(account);
    setChosen((now) => ({ ...now, [account.id]: access }));
    setError(null);
    setAgentAccess(account.id, access).then(
      (stored) => setChosen((now) => ({ ...now, [account.id]: stored })),
      (failure: unknown) => {
        setChosen((now) => ({ ...now, [account.id]: before }));
        setError(String(failure));
      },
    );
  };

  if (voiceFor) return <VoiceEditor identity={voiceFor} onBack={() => setVoiceFor(null)} />;

  return (
    <>
      <h3>Agents</h3>
      <p className="muted settings-lead">
        Agents are AI assistants and other programs that work with your mail through Regards&apos; command line. Each
        account decides what they may do. Only this page can change it: an agent can&apos;t give itself more.
      </p>
      {error && <p className="error" role="alert">{error}</p>}
      <AgentsGate />
      <AgentHolds accounts={accounts.map(({ account }) => ({ id: account.id, email: account.email }))} focus={focusHold} />
      {accounts.length === 0 ? (
        <p className="muted">Add an account first.</p>
      ) : (
        <ul className="account-list">
          {accounts.map(({ account }) => {
            const level = levelOf(account);
            const info = AGENT_ACCESS_LEVELS.find((entry) => entry.value === level);
            const window = windowOf(account);
            return (
              <li key={account.id} className="account-row agent-row">
                <Avatar person={{ name: account.display_name, address: account.email }} size="md" />
                <span className="account-text">
                  <strong>{account.email}</strong>
                  <span className="muted agent-allows">
                    {info?.allows}
                    {level === "send" &&
                      (window === null
                        ? " Each send waits for you here."
                        : ` Each send goes after ${windowLabel(window)} unless you call it back from the Outbox.`)}
                  </span>
                  {level === "send" && (
                    <label className="agent-send">
                      Sends
                      <select
                        className="agent-level"
                        value={window ?? ""}
                        onChange={(event) =>
                          void chooseWindow(account, event.target.value === "" ? null : Number(event.target.value))
                        }
                      >
                        <option value="">Ask me each time</option>
                        {SEND_WINDOWS.map((seconds) => (
                          <option key={seconds} value={seconds}>
                            Send after {windowLabel(seconds)} unless I call it back
                          </option>
                        ))}
                      </select>
                    </label>
                  )}
                </span>
                <select
                  className="agent-level"
                  aria-label={`What agents may do with ${account.email}`}
                  value={level}
                  onChange={(event) => void choose(account, event.target.value as AgentAccess)}
                >
                  {AGENT_ACCESS_LEVELS.map((entry) => (
                    <option key={entry.value} value={entry.value}>
                      {entry.label}
                    </option>
                  ))}
                </select>
              </li>
            );
          })}
        </ul>
      )}
      <h4 className="agent-heading">The levels</h4>
      <dl className="agent-levels">
        {AGENT_ACCESS_LEVELS.map((entry) => (
          <div key={entry.value}>
            <dt>
              {entry.label}
              {COMING.has(entry.value) && <span className="chip coming">Coming</span>}
            </dt>
            <dd>{entry.allows}</dd>
          </div>
        ))}
      </dl>
      <p className="muted settings-note">
        Each level allows what the one before it does. What agents change can be undone from the activity below for
        ten minutes. Whatever level an account has, changes to more than 50 messages at once and deleting for good wait
        for you at the top of this page, and so does every send, unless an account lets its sends go after a delay.
      </p>
      <VoiceSection accounts={accounts.map(({ account }) => account)} onOpen={setVoiceFor} />
      <NotesSection confirm={confirm} />
      {accounts.length > 0 && (
        <AgentActivity
          accounts={accounts.map(({ account }) => ({ id: account.id, email: account.email }))}
          folderName={(mailboxId) => {
            const mailbox = accounts.flatMap((entry) => entry.mailboxes).find((m) => m.id === mailboxId);
            return mailbox ? leafName(mailbox) : null;
          }}
        />
      )}
      <AgentConnect />
      <AgentsButtonOption />
    </>
  );
}

/** Settings › Agents: the sidebar's Agents button, which its menu can hide. */
function AgentsButtonOption() {
  const prefs = usePrefs();
  return (
    <Option
      icon="agent"
      label="Show Agents in the sidebar"
      description="A button at the foot of the sidebar, with ready-made requests to hand an agent, and the agents on this computer."
    >
      <Toggle label="Show Agents in the sidebar" on={prefs.agentsButton} onChange={(on) => setPrefs({ agentsButton: on })} />
    </Option>
  );
}

/** The account's name: who your mail is from, unless an address has a name of its own. Saved when you leave the field. */
function AccountName({ account }: { account: AccountSummary }) {
  const [name, setName] = useState(account.display_name);
  const [state, setState] = useState<"idle" | "saving" | "saved">("idle");
  const [error, setError] = useState<string | null>(null);
  useEffect(() => setName(account.display_name), [account.display_name]);
  const save = () => {
    const next = name.trim();
    if (next === account.display_name.trim()) return;
    setState("saving");
    setError(null);
    updateAccount(account.id, next).then(
      (saved) => {
        setName(saved.display_name);
        setState("saved");
      },
      (failure: unknown) => {
        setState("idle");
        setError(String(failure));
      },
    );
  };
  return (
    <label className="account-title">
      Your name
      <input
        className="input"
        placeholder={account.email}
        value={name}
        onChange={(event) => {
          setName(event.target.value);
          setState("idle");
        }}
        onBlur={save}
        onKeyDown={(event) => {
          if (event.key === "Enter") event.currentTarget.blur();
        }}
      />
      <span className="muted small" aria-live="polite">
        {error ??
          (state === "saving"
            ? "Saving…"
            : state === "saved"
              ? "Saved."
              : "Who your mail is from. An address with a name of its own (Addresses) keeps it. Empty: the address.")}
      </span>
    </label>
  );
}

function AccountRow({
  entry,
  confirm,
  many,
}: {
  entry: AccountMail;
  confirm: (request: ConfirmRequest) => Promise<boolean>;
  /** Several accounts: the unified inbox can leave this one out. */
  many: boolean;
}) {
  const { account, sync } = entry;
  const [syncing, setSyncing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [addresses, setAddresses] = useState(false);
  const [options, setOptions] = useState(false);
  // "Change password…" from the row: Details open on the password.
  const [password, setPassword] = useState(0);
  const prefs = usePrefs();
  const colour = accountColour(prefs, account.id);
  // A sign-in with Microsoft or Google that expired or was withdrawn: sign in again.
  const oauth = useOauth(() => setError(null));
  const provider = account.sign_in && account.sign_in !== "password" ? account.sign_in : null;

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

  const problem = error ?? oauth.error ?? entry.error ?? sync?.last_error ?? null;
  const status = account.needs_sign_in
    ? "Signed out: sign in again to sync and send"
    : syncing || sync?.running
      ? "Syncing…"
      : problem
        ? `Sync failed: ${problem}`
        : sync?.push_active
          ? "Live: new mail arrives as it comes in"
          : sync?.last_success_at
            ? `Synced at ${new Date(sync.last_success_at).toLocaleTimeString()}`
            : "Not synced yet";

  return (
    <>
    <li className="account-row">
      <Avatar person={{ name: account.display_name, address: account.email }} size="lg" ring />
      <div className="account-text">
        <strong>{account.display_name || account.email}</strong>
        <span className="muted">{account.email}</span>
        {provider && <span className="small muted">Signs in with {PROVIDERS[provider].label}</span>}
        <span className={problem || account.needs_sign_in ? "small error" : "small muted"}>{status}</span>
      </div>
      <div className="account-buttons">
        {account.needs_sign_in && provider && (
          <button type="button" className="btn small primary" onClick={() => void oauth.start(provider, account.email)} disabled={oauth.flow !== null}>
            Sign in again
          </button>
        )}
        {!provider && problem && (
          <button
            type="button"
            className="btn small"
            onClick={() => {
              setOptions(true);
              setPassword((now) => now + 1);
            }}
          >
            Change password…
          </button>
        )}
        <button type="button" className="btn small" onClick={syncNow} disabled={syncing || sync?.running}>
          <Icon name="refresh" size={15} />
          Sync now
        </button>
        <button type="button" className="btn small" aria-expanded={addresses} onClick={() => setAddresses(!addresses)}>
          Addresses
        </button>
        <button type="button" className="btn small" aria-expanded={options} onClick={() => setOptions(!options)}>
          Details
        </button>
        <button type="button" className="btn small danger" onClick={remove}>
          Remove
        </button>
      </div>
    </li>
    {oauth.flow && (
      <li>
        <OauthWaiting flow={oauth.flow} onCancel={oauth.cancel} />
      </li>
    )}
    {addresses && (
      <li>
        <IdentitiesEditor accountId={account.id} />
      </li>
    )}
    {options && (
      <li className="account-options">
        <AccountName account={account} />
        {!provider && <AccountLogin key={password} account={account} open={password > 0 ? "password" : null} />}
        <label className="account-title">
          Label in Regards
          <input
            className="input"
            placeholder={account.email}
            value={prefs.accountTitles[account.id] ?? ""}
            onChange={(event) => setPrefs({ accountTitles: { ...prefs.accountTitles, [account.id]: event.target.value } })}
          />
          <span className="muted small">Shown in the sidebar and above the list instead of the address. Only on this computer.</span>
        </label>
        {prefs.colourCoding ? (
          <div className="account-colour">
            <span>Colour</span>
            <span className="swatches" role="radiogroup" aria-label={`Colour of ${account.email}`}>
              {ACCOUNT_COLOURS.map((swatch) => (
                <button
                  key={swatch}
                  type="button"
                  role="radio"
                  aria-checked={swatch === colour}
                  aria-label={COLOUR_NAMES[swatch] ?? swatch}
                  title={COLOUR_NAMES[swatch]}
                  className={swatch === colour ? "swatch chosen" : "swatch"}
                  style={{ background: swatch }}
                  onClick={() => setPrefs({ accountColours: { ...prefs.accountColours, [account.id]: swatch } })}
                />
              ))}
            </span>
          </div>
        ) : (
          <p className="muted small">To give each account a colour, turn on colour coding in Appearance.</p>
        )}
        {many && (
          <Option
            icon="inbox"
            label="In the unified inbox"
            description="Off: this account's mail shows only in its own inbox, under the sidebar's Inbox."
          >
            <Toggle
              label="In the unified inbox"
              on={!prefs.hiddenAccounts.includes(account.id)}
              onChange={(on) =>
                setPrefs({
                  hiddenAccounts: on
                    ? prefs.hiddenAccounts.filter((id) => id !== account.id)
                    : [...prefs.hiddenAccounts.filter((id) => id !== account.id), account.id],
                })
              }
            />
          </Option>
        )}
      </li>
    )}
    </>
  );
}

/** How the inbox is sorted, and what happens to mail from people it doesn't know yet. */
function InboxTab({
  layout,
  onLayout,
  screening,
  onScreening,
  accounts,
}: {
  layout: Layout;
  onLayout: (change: Partial<Layout>) => void;
  screening: Screening;
  onScreening: (mode: Screening) => void;
  accounts: AccountSummary[];
}) {
  const [problem, setProblem] = useState<string | null>(null);
  const [groups, setGroups] = useState(false);
  if (groups) return <EmailGroupsPage accounts={accounts} onBack={() => setGroups(false)} />;
  const screen = (mode: Screening) => {
    const before = screening;
    onScreening(mode);
    setScreening(mode).then(
      () => setProblem(null),
      (error) => {
        onScreening(before);
        setProblem(String(error));
      },
    );
  };
  return (
    <>
      <h3>Inbox</h3>
      <Option
        icon="bolt"
        label="Inbox view"
        description="By priority puts priority mail first and folds notifications and newsletters into one row each; By category puts unread mail in a card per category, then what you have seen; By date shows everything together, newest first."
      >
        <Choice
          label="Inbox view"
          value={layout.inboxView}
          options={[
            { id: "smart", label: "By priority" },
            { id: "cards", label: "By category" },
            { id: "date", label: "By date" },
          ]}
          onChange={(inboxView) => onLayout({ inboxView })}
        />
      </Option>
      <Option
        icon="people"
        label="New senders"
        description="Mail from someone you haven't written to: with everyone else, in a card at the top to accept or block, or held back until you accept them."
      >
        <Choice
          label="New senders"
          value={screening}
          options={[
            { id: "off", label: "With everyone" },
            { id: "section", label: "In a card" },
            { id: "hold", label: "Held back" },
          ]}
          onChange={screen}
        />
      </Option>
      {problem && <p className="error" role="alert">{problem}</p>}
      <PriorityRows />
      <LinkRow
        icon="newspaper"
        label="Email groups"
        detail="Where Notifications, Newsletters and Pinned sit, and which accounts are in the unified inbox"
        onOpen={() => setGroups(true)}
      />
      <p className="muted settings-note">
        Sorting happens on this computer, from the headers and who you have written to. Nothing is sent anywhere. Open a
        sender from a message to choose where their mail goes, or to see why a message is where it is.
      </p>
    </>
  );
}

/** Text to start a message with: written here, or kept from the composer. */
function TemplatesTab() {
  return (
    <>
      <h3>Templates</h3>
      <p className="muted settings-lead">
        Start a message from one with the templates button in the composer, or {MOD_KEY}+Shift+T. The same list also keeps
        what you have written as a new template.
      </p>
      <TemplatesEditor />
    </>
  );
}

export function ShortcutsTab() {
  const prefs = usePrefs();
  return (
    <>
      <h3>Keyboard shortcuts</h3>
      <Option
        icon="keyboard"
        label="Keys"
        description="Standard: E done, S snooze, R reply all, Shift+R reply, as in most desktop mail apps. Classic: the keys of earlier Regards versions."
      >
        <Select
          label="Keys"
          value={prefs.keySet}
          options={[
            { id: "standard", label: "Standard" },
            { id: "classic", label: "Classic" },
          ]}
          onChange={(keySet) => setPrefs({ keySet })}
        />
      </Option>
      {!prefs.singleKeys && (
        <p className="muted settings-note">
          Single-key shortcuts are off (Settings › Accessibility): only those with {MOD_KEY}, and keys such as Escape and
          the arrows, act.
        </p>
      )}
      <dl className="shortcut-list">
        {shortcutList(prefs.keySet).map((shortcut) => (
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

