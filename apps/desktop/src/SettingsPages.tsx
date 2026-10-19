import { useEffect, useState } from "react";
import type { AccountSummary } from "./accounts";
import { Avatar } from "./Avatar";
import { backgroundMode, notificationsShown, setBackgroundMode, setNotificationsShown } from "./background";
import { Icon } from "./icons";
import type { Layout } from "./layout";
import type { ServiceCheck } from "./mail";
import { ACCOUNT_COLOURS, type Prefs, accountColour, accountLabel, setPrefs, usePrefs } from "./prefs";
import { LinkRow, Option, Select, SubPageHead, Toggle } from "./SettingsControls";
import { senderImages, setSenderImages } from "./smart";
import { type PresetDef, type PresetPrefs, REMINDER_PRESETS, SEND_PRESETS, SNOOZE_PRESETS } from "./snooze";
import { type Theme, applyTheme, savedTheme } from "./theme";
import { AboutUpdates } from "./UpdateCheck";

/** Where the project lives, for help and reports. */
const PROJECT_URL = "https://github.com/lucienPochon/regards";

/**
 * General: how the app runs, what happens after a message leaves the list,
 * sending, and reading. Background mode is kept by the app (desktop.json);
 * the rest on this computer.
 */
export function GeneralPage({ accounts }: { accounts: AccountSummary[] }) {
  const prefs = usePrefs();
  const [background, setBackground] = useState<boolean | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [senders, setSenders] = useState(false);
  useEffect(() => {
    backgroundMode().then(setBackground, (failure: unknown) => setError(String(failure)));
  }, []);
  if (senders) return <ImageSendersPage accounts={accounts} onBack={() => setSenders(false)} />;
  return (
    <>
      <h3>General</h3>
      {error && <p className="error">{error}</p>}
      <Option
        icon="clock"
        label="Start at login and keep running in the tray"
        description={
          background === false
            ? "Off: closing the window quits Regards. Snoozed mail, reminders and scheduled messages then wait until it is open again."
            : "Regards starts when you log in and stays in the tray when you close its window, so snoozed mail comes back and scheduled messages go. They need this computer to be on."
        }
      >
        {background !== null && (
          <Toggle
            label="Start at login and keep running in the tray"
            on={background}
            onChange={(on) => {
              setError(null);
              setBackgroundMode(on).then(
                () => setBackground(on),
                (failure: unknown) => setError(String(failure)),
              );
            }}
          />
        )}
      </Option>
      <Option icon="archive" label="After archiving or deleting a message">
        <Select
          label="After archiving or deleting a message"
          value={prefs.afterMove}
          options={[
            { id: "next", label: "Open the next one" },
            { id: "previous", label: "Open the previous one" },
            { id: "list", label: "Go back to the list" },
          ]}
          onChange={(afterMove) => setPrefs({ afterMove })}
        />
      </Option>
      <Option icon="pin" label="When a pinned message is archived">
        <Select
          label="When a pinned message is archived"
          value={prefs.pinnedArchive}
          options={[
            { id: "keep", label: "Keep it pinned" },
            { id: "unpin", label: "Unpin it" },
          ]}
          onChange={(pinnedArchive) => setPrefs({ pinnedArchive })}
        />
      </Option>
      <Option
        icon="send"
        label="Undo send"
        description="How long a message waits in the Outbox after you send it, so you can call it back."
      >
        <Select
          label="Undo send"
          value={prefs.undoSeconds}
          options={[5, 10, 15, 30].map((seconds) => ({ id: seconds, label: `${seconds} seconds` }))}
          onChange={(undoSeconds) => setPrefs({ undoSeconds })}
        />
      </Option>
      <Option
        icon="mail"
        label="Fold older messages in a conversation"
        description="Older messages show as one line each; the one you opened is shown in full."
      >
        <Toggle label="Fold older messages in a conversation" on={prefs.collapseOlder} onChange={(collapseOlder) => setPrefs({ collapseOlder })} />
      </Option>
      <Option
        icon="image"
        label="Load images from the web"
        description="Senders can tell when you open their message from these images. Off: each message asks before loading them."
      >
        <Toggle label="Load images from the web" on={prefs.remoteImages} onChange={(remoteImages) => setPrefs({ remoteImages })} />
      </Option>
      {!prefs.remoteImages && (
        <LinkRow
          icon="people"
          label="Senders whose images load"
          detail="Allowed from a message with “Always from this sender”"
          onOpen={() => setSenders(true)}
        />
      )}
    </>
  );
}

/** The senders and domains whose images load without asking, per account; each can be blocked again. */
function ImageSendersPage({ accounts, onBack }: { accounts: AccountSummary[]; onBack: () => void }) {
  const prefs = usePrefs();
  const [lists, setLists] = useState<Record<string, string[]> | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    Promise.all(accounts.map((account) => senderImages(account.id).then((list) => [account.id, list] as const))).then(
      (entries) => setLists(Object.fromEntries(entries)),
      (failure: unknown) => setError(String(failure)),
    );
  }, [accounts]);
  const block = (accountId: string, who: string) => {
    setError(null);
    setSenderImages(accountId, who, false).then(
      () => setLists((now) => now && { ...now, [accountId]: (now[accountId] ?? []).filter((one) => one !== who) }),
      (failure: unknown) => setError(String(failure)),
    );
  };
  const none = lists !== null && accounts.every((account) => (lists[account.id] ?? []).length === 0);
  return (
    <>
      <SubPageHead title="Senders whose images load" onBack={onBack} />
      <p className="muted settings-note">
        Their messages show images from the web without asking, so they can tell when you open them. To add one, open a
        message from them and choose “Always from this sender”.
      </p>
      {error && <p className="error">{error}</p>}
      {lists === null && !error && <p className="muted">Loading…</p>}
      {none && <p className="muted">Nobody yet: every message asks before loading images.</p>}
      {lists !== null &&
        accounts.map((account) => {
          const list = lists[account.id] ?? [];
          if (list.length === 0) return null;
          return (
            <section key={account.id} aria-label={accountLabel(prefs, account)}>
              {accounts.length > 1 && <h4 className="settings-subhead plain">{accountLabel(prefs, account)}</h4>}
              {list.map((who) => (
                <div key={who} className="option">
                  <span className="option-icon">
                    <Icon name={who.startsWith("@") ? "people" : "user"} size={18} />
                  </span>
                  <span className="option-text">
                    <strong>{who.startsWith("@") ? `Anyone at ${who.slice(1)}` : who}</strong>
                  </span>
                  <span className="option-control">
                    <button type="button" className="btn small" onClick={() => block(account.id, who)}>
                      Block again
                    </button>
                  </span>
                </div>
              ))}
            </section>
          );
        })}
    </>
  );
}

const THEMES: { id: Theme; label: string }[] = [
  { id: "system", label: "As this computer" },
  { id: "light", label: "Light" },
  { id: "dark", label: "Dark" },
];

/** Appearance: theme, layout, the list and the sidebar, and each account's colour. */
export function AppearancePage({
  layout,
  onLayout,
  accounts,
}: {
  layout: Layout;
  onLayout: (change: Partial<Layout>) => void;
  accounts: AccountSummary[];
}) {
  const prefs = usePrefs();
  const [theme, setTheme] = useState<Theme>(savedTheme);
  const [colours, setColours] = useState(false);
  if (colours) return <AccountColours accounts={accounts} onBack={() => setColours(false)} />;
  const layoutChoice = layout.mode === "split" ? "split" : layout.handle ? "switch" : "single";
  return (
    <>
      <h3>Appearance</h3>
      <Option icon="sun" label="Theme">
        <Select
          label="Theme"
          value={theme}
          options={THEMES}
          onChange={(next) => {
            setTheme(next);
            applyTheme(next, true);
          }}
        />
      </Option>
      <Option
        icon={layout.mode === "split" ? "split" : "single"}
        label="Layout"
        description="A message opens in place of the list, beside it, or in place of it with a handle on the list's edge to show both."
      >
        <Select
          label="Layout"
          value={layoutChoice}
          options={[
            { id: "single", label: "In place of the list" },
            { id: "split", label: "Beside the list" },
            { id: "switch", label: "In place, with a handle" },
          ]}
          onChange={(choice) =>
            onLayout(choice === "split" ? { mode: "split" } : { mode: "single", handle: choice === "switch" })
          }
        />
      </Option>
      <Option icon="sidebar" label="Sidebar" description={
          <>
            Keep the folders open beside the mail, or fold them to icons. <kbd>/</kbd> does the same.
          </>
        }
      >
        <Select
          label="Sidebar"
          value={layout.locked ? "open" : "icons"}
          options={[
            { id: "open", label: "Open beside the mail" },
            { id: "icons", label: "Icons" },
          ]}
          onChange={(value) => onLayout({ locked: value === "open" })}
        />
      </Option>
      <Option icon="text" label="List density" description="When the list fills the window.">
        <Select
          label="List density"
          value={prefs.density}
          options={[
            { id: "comfortable", label: "Comfortable" },
            { id: "compact", label: "Compact" },
          ]}
          onChange={(density) => setPrefs({ density })}
        />
      </Option>
      <Option icon="user" label="Pictures of people" description="The coloured circles with initials. Regards never fetches people's pictures.">
        <Select
          label="Pictures of people"
          value={prefs.avatars}
          options={[
            { id: "everywhere", label: "Everywhere" },
            { id: "reader", label: "In messages only" },
            { id: "list", label: "In the list only" },
            { id: "nowhere", label: "Nowhere" },
          ]}
          onChange={(avatars) => setPrefs({ avatars })}
        />
      </Option>
      <Option icon="text" label="Preview lines" description="Lines of each message's start in the list, when it is beside the reader.">
        <Select
          label="Preview lines"
          value={prefs.previewLines}
          options={[
            { id: 0, label: "None" },
            { id: 1, label: "1 line" },
            { id: 2, label: "2 lines" },
          ]}
          onChange={(previewLines) => setPrefs({ previewLines: previewLines as Prefs["previewLines"] })}
        />
      </Option>
      <LinkRow
        icon="user"
        label="Colour for each account"
        detail={prefs.colourCoding ? "On: a ring in each account's colour around pictures" : "Off"}
        onOpen={() => setColours(true)}
      />
    </>
  );
}

function AccountColours({ accounts, onBack }: { accounts: AccountSummary[]; onBack: () => void }) {
  const prefs = usePrefs();
  return (
    <>
      <SubPageHead title="Colour for each account" onBack={onBack} />
      <Option
        icon="user"
        label="Colour-code accounts"
        description="A ring in the account's colour around pictures in the list and the sidebar, so you can tell accounts apart in the unified inbox."
      >
        <Toggle
          label="Colour-code accounts"
          on={prefs.colourCoding}
          onChange={(colourCoding) => {
            // Each account gets a colour of its own at once; the person can change it below.
            const accountColours = { ...prefs.accountColours };
            accounts.forEach((account, index) => {
              accountColours[account.id] ??= ACCOUNT_COLOURS[index % ACCOUNT_COLOURS.length];
            });
            setPrefs({ colourCoding, accountColours });
          }}
        />
      </Option>
      {prefs.colourCoding &&
        accounts.map((account) => {
          const current = accountColour(prefs, account.id);
          return (
            <div key={account.id} className="option">
              <Avatar person={{ name: account.display_name, address: account.email }} size="sm" colour={current} />
              <span className="option-text">
                <strong>{accountLabel(prefs, account)}</strong>
              </span>
              <span className="swatches" role="radiogroup" aria-label={`Colour of ${account.email}`}>
                {ACCOUNT_COLOURS.map((colour) => (
                  <button
                    key={colour}
                    type="button"
                    role="radio"
                    aria-checked={colour === current}
                    aria-label={colour}
                    className={colour === current ? "swatch chosen" : "swatch"}
                    style={{ background: colour }}
                    onClick={() => setPrefs({ accountColours: { ...prefs.accountColours, [account.id]: colour } })}
                  />
                ))}
              </span>
            </div>
          );
        })}
    </>
  );
}

/** Notifications: what Regards tells you, and the sidebar's counts. */
export function NotificationsPage() {
  const prefs = usePrefs();
  const [notifications, setNotifications] = useState<boolean | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    notificationsShown().then(setNotifications, (failure: unknown) => setError(String(failure)));
  }, []);
  return (
    <>
      <h3>Notifications</h3>
      {error && <p className="error">{error}</p>}
      <Option
        icon="bell"
        label="Snoozes, reminders and sending problems"
        description="When a snooze ends, nobody has replied by a reminder, an agent is waiting for you, or a message could not be sent. They show who and the subject, never the message."
      >
        {notifications !== null && (
          <Toggle
            label="Snoozes, reminders and sending problems"
            on={notifications}
            onChange={(on) => {
              setError(null);
              setNotificationsShown(on).then(
                () => setNotifications(on),
                (failure: unknown) => setError(String(failure)),
              );
            }}
          />
        )}
      </Option>
      <Option icon="inbox" label="Unread counts in the sidebar" description="Beside the inbox and each account's inbox.">
        <Toggle label="Unread counts in the sidebar" on={prefs.sidebarCounts} onChange={(sidebarCounts) => setPrefs({ sidebarCounts })} />
      </Option>
    </>
  );
}

type Picker = "snooze" | "send" | "reminders";
const PICKERS: Record<Picker, { title: string; detail: string; defs: PresetDef[]; key: "snoozePresets" | "sendPresets" | "reminderPresets" }> = {
  snooze: { title: "Snooze", detail: "The times offered when you snooze a message", defs: SNOOZE_PRESETS, key: "snoozePresets" },
  send: { title: "Send later", detail: "The times offered when you send a message later", defs: SEND_PRESETS, key: "sendPresets" },
  reminders: {
    title: "Reminders",
    detail: "The times offered for a reminder if nobody replies",
    defs: REMINDER_PRESETS,
    key: "reminderPresets",
  },
};

/** Scheduling: the quick choices of each time picker, shown or not and at what time. */
export function SchedulingPage() {
  const [picker, setPicker] = useState<Picker | null>(null);
  if (picker) return <PresetsPage picker={picker} onBack={() => setPicker(null)} />;
  return (
    <>
      <h3>Scheduling</h3>
      {(Object.keys(PICKERS) as Picker[]).map((id) => (
        <LinkRow key={id} icon={id === "send" ? "sendLater" : id === "snooze" ? "clock" : "bell"} label={PICKERS[id].title} detail={PICKERS[id].detail} onOpen={() => setPicker(id)} />
      ))}
      <p className="muted settings-note">
        Every picker also takes any date and time you choose. Snoozes, reminders and scheduled messages are carried out by
        this computer, so it must be on at that time.
      </p>
    </>
  );
}

function PresetsPage({ picker, onBack }: { picker: Picker; onBack: () => void }) {
  const prefs = usePrefs();
  const { title, defs, key } = PICKERS[picker];
  const current: PresetPrefs = prefs[key];
  const change = (id: string, part: { on?: boolean; time?: string }) =>
    setPrefs({ [key]: { ...current, [id]: { ...current[id], ...part } } } as Partial<Prefs>);
  return (
    <>
      <SubPageHead title={title} onBack={onBack} />
      {defs.map((def) => {
        const on = current[def.id]?.on !== false;
        return (
          <div key={def.id} className="option preset-row">
            <span className="option-text">
              <strong>{def.label}</strong>
              <span>{def.when}</span>
            </span>
            {def.time && (
              <input
                type="time"
                className="settings-select preset-time"
                aria-label={`Time for ${def.label}`}
                value={current[def.id]?.time ?? def.time}
                disabled={!on}
                onChange={(event) => event.target.value && change(def.id, { time: event.target.value })}
              />
            )}
            <Toggle label={`Offer ${def.label}`} on={on} onChange={(value) => change(def.id, { on: value })} />
          </div>
        );
      })}
      <button type="button" className="btn small" onClick={() => setPrefs({ [key]: {} } as Partial<Prefs>)}>
        Back to the usual times
      </button>
    </>
  );
}

/** Inbox › Email groups: where groups sit, which mail is grouped, and accounts in the unified inbox. */
export function EmailGroupsPage({ accounts, onBack }: { accounts: AccountSummary[]; onBack: () => void }) {
  const prefs = usePrefs();
  const grouped = (key: keyof Prefs["grouped"], value: boolean) => setPrefs({ grouped: { ...prefs.grouped, [key]: value } });
  const hidden = new Set(prefs.hiddenAccounts);
  return (
    <>
      <SubPageHead title="Email groups" onBack={onBack} />
      <Option
        icon="clock"
        label="Where groups sit"
        description="Notifications, Newsletters and Pinned, each as one row of the Smart Inbox."
      >
        <Select
          label="Where groups sit"
          value={prefs.groupsPosition}
          options={[
            { id: "today", label: "With today's mail" },
            { id: "chronological", label: "At their newest message's day" },
          ]}
          onChange={(groupsPosition) => setPrefs({ groupsPosition })}
        />
      </Option>
      <h4 className="settings-subhead">Categories</h4>
      {(
        [
          { key: "notifications", label: "Notifications", icon: "bell" },
          { key: "newsletters", label: "Newsletters", icon: "newspaper" },
          { key: "pinned", label: "Pinned", icon: "pin" },
        ] as const
      ).map((category) => (
        <Option key={category.key} icon={category.icon} label={category.label}>
          <Select
            label={category.label}
            value={prefs.grouped[category.key] ? "group" : "all"}
            options={[
              { id: "group", label: "As one row" },
              {
                id: "all",
                label: category.key === "pinned" ? "Only in Pinned, in the sidebar" : "With everything else",
              },
            ]}
            onChange={(value) => grouped(category.key, value === "group")}
          />
        </Option>
      ))}
      {accounts.length > 1 && (
        <>
          <h4 className="settings-subhead">Accounts in the unified inbox</h4>
          {accounts.map((account) => (
            <div key={account.id} className="option">
              <Avatar person={{ name: account.display_name, address: account.email }} size="sm" />
              <span className="option-text">
                <strong>{accountLabel(prefs, account)}</strong>
                <span>{hidden.has(account.id) ? "Only in its own inbox, under the sidebar's Inbox" : "In the unified inbox"}</span>
              </span>
              <Select
                label={`Where ${account.email}'s mail shows`}
                value={hidden.has(account.id) ? "hide" : "show"}
                options={[
                  { id: "show", label: "In the unified inbox" },
                  { id: "hide", label: "Only in its own inbox" },
                ]}
                onChange={(value) =>
                  setPrefs({
                    hiddenAccounts:
                      value === "hide"
                        ? [...prefs.hiddenAccounts.filter((id) => id !== account.id), account.id]
                        : prefs.hiddenAccounts.filter((id) => id !== account.id),
                  })
                }
              />
            </div>
          ))}
        </>
      )}
    </>
  );
}

/** The Priority card's size, for Inbox. */
export function PriorityRows() {
  const prefs = usePrefs();
  return (
    <Option icon="boltFilled" label="Priority card" description="How many priority messages it shows before “View all”.">
      <Select
        label="Priority card"
        value={prefs.priorityRows}
        options={[
          ...Array.from({ length: 10 }, (_, index) => ({ id: index + 1, label: `${index + 1}` })),
          { id: 0, label: "All" },
        ]}
        onChange={(priorityRows) => setPrefs({ priorityRows })}
      />
    </Option>
  );
}

/** Composer: who a new message is from, and where addresses and signatures live. */
export function ComposerPage({ accounts, onAccounts }: { accounts: AccountSummary[]; onAccounts: () => void }) {
  const prefs = usePrefs();
  return (
    <>
      <h3>Composer</h3>
      {accounts.length > 1 && (
        <Option
          icon="compose"
          label="New messages are from"
          description="Unless you are in one account's folder, or replying: then it is that account."
        >
          <Select
            label="New messages are from"
            value={prefs.defaultFrom ?? ""}
            options={[
              { id: "", label: "The first account" },
              ...accounts.map((account) => ({ id: account.id, label: accountLabel(prefs, account) })),
            ]}
            onChange={(defaultFrom) => setPrefs({ defaultFrom: defaultFrom || null })}
          />
        </Option>
      )}
      <LinkRow
        icon="user"
        label="Addresses and signatures"
        detail="Send as an alias, and the signature each address adds"
        onOpen={onAccounts}
      />
      <p className="muted settings-note">How long you can call a message back after sending it is under General, “Undo send”.</p>
    </>
  );
}

/** Accessibility: the whole window larger or smaller. */
export function AccessibilityPage() {
  const prefs = usePrefs();
  return (
    <>
      <h3>Accessibility</h3>
      <Option icon="text" label="Zoom" description="Makes everything in the window larger or smaller.">
        <Select
          label="Zoom"
          value={prefs.zoom}
          options={[0.9, 1, 1.1, 1.25, 1.5].map((zoom) => ({ id: zoom, label: `${Math.round(zoom * 100)} %` }))}
          onChange={(zoom) => setPrefs({ zoom })}
        />
      </Option>
    </>
  );
}

/** Support: the version and updates, the background service, and where to get help. */
export function SupportPage({ service, onRecheck }: { service: ServiceCheck; onRecheck: () => void }) {
  const [copied, setCopied] = useState<string | null>(null);
  const copy = (text: string) => {
    navigator.clipboard?.writeText(text).then(
      () => setCopied(text),
      () => {},
    );
  };
  const links = [
    { label: "Report a problem or ask for a feature", url: `${PROJECT_URL}/issues` },
    { label: "Read the documentation", url: `${PROJECT_URL}#readme` },
  ];
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
      <AboutUpdates />
      <h4 className="settings-subhead">Background service</h4>
      <ServiceStatus check={service} />
      <button type="button" className="btn" onClick={onRecheck} disabled={service.state === "checking"}>
        <Icon name="refresh" size={15} />
        Check again
      </button>
      <h4 className="settings-subhead">Get help</h4>
      {links.map((link) => (
        <div key={link.url} className="option help-row">
          <span className="option-icon">
            <Icon name="help" size={18} />
          </span>
          <span className="option-text">
            <strong>{link.label}</strong>
            <span className="help-url">{link.url}</span>
          </span>
          <button type="button" className="btn small" onClick={() => copy(link.url)}>
            <Icon name={copied === link.url ? "check" : "copy"} size={14} />
            {copied === link.url ? "Copied" : "Copy link"}
          </button>
        </div>
      ))}
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
