import { useEffect, useState } from "react";
import {
  type AccountSummary,
  type AutoCopy,
  type ReadingSettings,
  prefetchOn,
  readingSettings,
  setAutoCopy,
  setPrefetch,
  setReadingSettings,
} from "./accounts";
import { Avatar } from "./Avatar";
import {
  type BackgroundSettings,
  backgroundSettings,
  notificationsShown,
  openSystemSettings,
  openUrl,
  setCloseToTray,
  setNotificationsShown,
  setStartAtLogin,
} from "./background";
import { type BadgeSettings, badgeSettings, setBadgeSettings } from "./badge";
import { type MailAppStatus, mailAppStatus, makeMailApp } from "./links";
import type { ConfirmRequest } from "./Confirm";
import { DownloadAgain } from "./DownloadAgain";
import { Icon, type IconName } from "./icons";
import type { Layout } from "./layout";
import { type NewMail, type NotificationSettings, notificationSettings, setNotificationSettings } from "./notifications";
import type { ServiceCheck } from "./mail";
import { ACCOUNT_COLOURS, COLOUR_NAMES, type Prefs, ZOOMS, accountColour, accountLabel, setPrefs, usePrefs } from "./prefs";
import { MOD_KEY } from "./shortcuts";
import { LinkRow, Option, Select, SubPageHead, Toggle } from "./SettingsControls";
import { senderImages, setSenderImages } from "./smart";
import { exportDiagnostics, setDetailedLogs, supportSettings } from "./support";
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
  const [background, setBackground] = useState<BackgroundSettings | null>(null);
  const [prefetch, setPrefetchShown] = useState<boolean | null>(null);
  const [reading, setReading] = useState<ReadingSettings | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [senders, setSenders] = useState(false);
  useEffect(() => {
    backgroundSettings().then(setBackground, (failure: unknown) => setError(String(failure)));
    prefetchOn().then(setPrefetchShown, (failure: unknown) => setError(String(failure)));
    readingSettings().then(setReading, (failure: unknown) => setError(String(failure)));
  }, []);
  const changeBackground = (key: keyof BackgroundSettings, on: boolean) => {
    if (!background) return;
    setError(null);
    (key === "start_at_login" ? setStartAtLogin(on) : setCloseToTray(on)).then(
      () => setBackground({ ...background, [key]: on }),
      (failure: unknown) => setError(String(failure)),
    );
  };
  if (senders) return <ImageSendersPage accounts={accounts} onBack={() => setSenders(false)} />;
  return (
    <>
      <h3>General</h3>
      {error && <p className="error" role="alert">{error}</p>}
      <Option
        icon="logo"
        label="Start Regards when you log in"
        description="In the tray, without opening a window, so mail is up to date when you look."
      >
        {background && (
          <Toggle
            label="Start Regards when you log in"
            on={background.start_at_login}
            onChange={(on) => changeBackground("start_at_login", on)}
          />
        )}
      </Option>
      <Option
        icon="clock"
        label="Keep running when the window is closed"
        description={
          background?.close_to_tray === false
            ? "Off: closing the window quits Regards. Snoozed mail, reminders and scheduled messages then wait until it is open again."
            : "Regards stays in the tray, so snoozed mail comes back and scheduled messages go. They need this computer to be on."
        }
      >
        {background && (
          <Toggle
            label="Keep running when the window is closed"
            on={background.close_to_tray}
            onChange={(on) => changeBackground("close_to_tray", on)}
          />
        )}
      </Option>
      <MailApp />
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
          options={[5, 10, 15, 30, 45, 60].map((seconds) => ({ id: seconds, label: seconds === 60 ? "1 minute" : `${seconds} seconds` }))}
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
      <Option
        icon="shield"
        label="Remove tracking from links"
        description="Takes out the parts of a link that only tell who clicked (utm_…, fbclid …); the rest of the link stays. Also for what agents read."
      >
        {reading !== null && (
          <Toggle
            label="Remove tracking from links"
            on={reading.clean_links}
            onChange={(on) => {
              setError(null);
              const next = { ...reading, clean_links: on };
              setReadingSettings(next).then(
                () => setReading(next),
                (failure: unknown) => setError(String(failure)),
              );
            }}
          />
        )}
      </Option>
      <Option
        icon="archive"
        label="Download recent mail ahead of time"
        description={
          prefetch
            ? "Your inboxes' mail from the last 30 days (up to 25 MB each) is downloaded now, then every 10 minutes: it opens at once, even offline, and all its text is searchable. Uses data and disk space."
            : "Off: a message is downloaded when you open it. On: the last 30 days of your inboxes, so they open at once, even offline, and all their text is searchable. Uses data and disk space."
        }
      >
        {prefetch !== null && (
          <Toggle
            label="Download recent mail ahead of time"
            on={prefetch}
            onChange={(on) => {
              setError(null);
              setPrefetch(on).then(
                () => setPrefetchShown(on),
                (failure: unknown) => setError(String(failure)),
              );
            }}
          />
        )}
      </Option>
    </>
  );
}

/**
 * Regards as the mail app: email links (mailto:) on web pages and in other apps
 * open a new message here. Registered only when the person asks; on Windows the
 * system's Default apps then opens to confirm, and on macOS Mail's settings choose.
 */
function MailApp() {
  const [status, setStatus] = useState<MailAppStatus | null>(null);
  const [asked, setAsked] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    const check = () => mailAppStatus().then(setStatus, (failure: unknown) => setError(String(failure)));
    check();
    // Back from the system's settings: see whether it took.
    window.addEventListener("focus", check);
    return () => window.removeEventListener("focus", check);
  }, []);
  const make = () => {
    setError(null);
    makeMailApp().then(
      () => {
        setAsked(true);
        mailAppStatus().then(setStatus, () => {});
      },
      (failure: unknown) => setError(String(failure)),
    );
  };
  if (!status) return null;
  const description = error
    ? error
    : status.registered
      ? "Email links on web pages and in other apps open a new message here. Nothing is sent until you send it."
      : !status.can_register
        ? "On a Mac, the mail app is chosen in Mail, under Settings › General: choose Regards there."
        : asked && /Win/.test(navigator.platform)
          ? "Confirm it in Default apps, which just opened: choose Regards for MAILTO."
          : "Email links on web pages and in other apps would open a new message here.";
  return (
    <Option icon="mail" label="Mail app" description={description}>
      {status.registered ? (
        <span className="muted mail-app-yes">
          <Icon name="check" size={15} />
          Regards
        </span>
      ) : (
        status.can_register && (
          <button type="button" className="btn small" onClick={make}>
            Make Regards your mail app
          </button>
        )
      )}
    </Option>
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
      {error && <p className="error" role="alert">{error}</p>}
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
                    aria-label={COLOUR_NAMES[colour] ?? colour}
                    title={COLOUR_NAMES[colour]}
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

const platform = navigator.platform;
/** Where the unread count shows: the dock on a Mac, the taskbar elsewhere. */
const DOCK = /Mac/.test(platform) ? "dock" : "taskbar";
const BADGE_NOTE = /Win/.test(platform)
  ? "On Windows, a dot on the icon while an inbox has unread mail: its taskbar shows no number. Not mail set aside or snoozed."
  : "The inboxes' unread mail, as in the sidebar. Not mail set aside or snoozed.";

const NEW_MAIL: { id: NewMail; label: string; detail: string }[] = [
  { id: "all", label: "All new mail", detail: "Everything that arrives in the inbox." },
  { id: "smart", label: "People and Priority", detail: "Mail from people and Priority; not new senders, notifications or newsletters." },
  { id: "priority", label: "Priority only", detail: "Only mail in Priority." },
  { id: "none", label: "None", detail: "Nothing for new mail." },
];

/** The notices besides new mail, each its own switch. */
const NOTICES: { id: Exclude<keyof NotificationSettings, "new_mail">; label: string; description: string; icon: IconName }[] = [
  { id: "snooze_ended", label: "Snoozed mail comes back", description: "When its time comes.", icon: "clock" },
  { id: "reminder_due", label: "Nobody has replied", description: "By the time a reminder was set for.", icon: "bell" },
  {
    id: "sending",
    label: "A message did not go",
    description: "Not sent, perhaps not sent, or held because this computer was off when it was due.",
    icon: "alert",
  },
  { id: "agents", label: "Agents", description: "An agent waits for your answer, or is sending a message.", icon: "agent" },
];

/**
 * Notifications: the switch for all of them (the tray's), which new mail
 * of each account, the other notices one by one, and the sidebar's counts.
 * Notices show who and the subject, never the message.
 */
export function NotificationsPage({ accounts }: { accounts: AccountSummary[] }) {
  const prefs = usePrefs();
  const [shown, setShown] = useState<boolean | null>(null);
  const [settings, setSettings] = useState<NotificationSettings | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    notificationsShown().then(setShown, (failure: unknown) => setError(String(failure)));
    notificationSettings().then(setSettings, (failure: unknown) => setError(String(failure)));
  }, []);
  const change = (next: NotificationSettings) => {
    const before = settings;
    setError(null);
    setSettings(next);
    setNotificationSettings(next).catch((failure: unknown) => {
      setSettings(before);
      setError(String(failure));
    });
  };
  const off = shown === false;
  const [badge, setBadge] = useState<BadgeSettings | null>(null);
  useEffect(() => {
    badgeSettings().then(setBadge, (failure: unknown) => setError(String(failure)));
  }, []);
  const changeBadge = (next: BadgeSettings) => {
    const before = badge;
    setError(null);
    setBadge(next);
    setBadgeSettings(next).catch((failure: unknown) => {
      setBadge(before);
      setError(String(failure));
    });
  };
  const systemSettings = () => {
    setError(null);
    openSystemSettings("notifications").catch((failure: unknown) => setError(String(failure)));
  };
  return (
    <>
      <h3>Notifications</h3>
      {error && <p className="error" role="alert">{error}</p>}
      <Option
        icon="bell"
        label="Show notifications"
        description={
          off
            ? "Off: Regards shows none, whatever is chosen below. The tray icon's menu has the same switch."
            : "They show who and the subject, never the message. The tray icon's menu has the same switch."
        }
      >
        {shown !== null && (
          <Toggle
            label="Show notifications"
            on={shown}
            onChange={(on) => {
              setError(null);
              setNotificationsShown(on).then(
                () => setShown(on),
                (failure: unknown) => setError(String(failure)),
              );
            }}
          />
        )}
      </Option>
      {settings && (
        <div className={off ? "notice-choices dimmed" : "notice-choices"} aria-disabled={off || undefined}>
          <h4 className="settings-subhead">New mail</h4>
          {accounts.map((account) => {
            const choice = settings.new_mail[account.id] ?? "smart";
            return (
              <div key={account.id} className="option">
                <Avatar
                  person={{ name: account.display_name, address: account.email }}
                  size="sm"
                  colour={accountColour(prefs, account.id)}
                />
                <span className="option-text">
                  <strong>{accounts.length > 1 ? accountLabel(prefs, account) : "New mail"}</strong>
                  <span>{NEW_MAIL.find((option) => option.id === choice)?.detail}</span>
                </span>
                <span className="option-control">
                  <Select
                    label={`New mail in ${account.email}`}
                    value={choice}
                    options={NEW_MAIL}
                    onChange={(next) => change({ ...settings, new_mail: { ...settings.new_mail, [account.id]: next } })}
                  />
                </span>
              </div>
            );
          })}
          <p className="muted settings-note">
            Mail that came while Regards was off is told of when it starts. Not the mail already there when an account is
            added, nor what you already read elsewhere. When several arrive at once, they come as one.
          </p>
          <h4 className="settings-subhead">Also tell me when</h4>
          {NOTICES.map((notice) => (
            <Option key={notice.id} icon={notice.icon} label={notice.label} description={notice.description}>
              <Toggle
                label={notice.label}
                on={settings[notice.id]}
                onChange={(on) => change({ ...settings, [notice.id]: on })}
              />
            </Option>
          ))}
        </div>
      )}
      <LinkRow
        icon="settings"
        label="This computer's notification settings"
        detail="Sounds, banners and quiet hours are set there, for Regards as for any app"
        onOpen={systemSettings}
      />
      {badge && (
        <>
          <h4 className="settings-subhead">On the {DOCK}</h4>
          <Option icon="inbox" label={`Unread count on the ${DOCK}`} description={BADGE_NOTE}>
            <Toggle label={`Unread count on the ${DOCK}`} on={badge.on} onChange={(on) => changeBadge({ ...badge, on })} />
          </Option>
          {badge.on &&
            accounts.length > 1 &&
            accounts.map((account) => {
              const counted = badge.accounts === null || badge.accounts.includes(account.id);
              return (
                <div key={account.id} className="option badge-account">
                  <Avatar
                    person={{ name: account.display_name, address: account.email }}
                    size="sm"
                    colour={accountColour(prefs, account.id)}
                  />
                  <span className="option-text">
                    <strong>{accountLabel(prefs, account)}</strong>
                    <span>{counted ? "Counted" : "Not counted"}</span>
                  </span>
                  <span className="option-control">
                    <Toggle
                      label={`Count ${account.email}`}
                      on={counted}
                      onChange={(on) => {
                        const now = badge.accounts ?? accounts.map((one) => one.id);
                        const next = on ? [...now.filter((id) => id !== account.id), account.id] : now.filter((id) => id !== account.id);
                        // Every account counted: say so as "all", so accounts added later count too.
                        const all = accounts.every((one) => next.includes(one.id));
                        changeBadge({ ...badge, accounts: all ? null : next });
                      }}
                    />
                  </span>
                </div>
              );
            })}
        </>
      )}
      <h4 className="settings-subhead">In Regards</h4>
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
/** One account's automatic copy: off, Cc or Bcc, to one address. Kept by the daemon. */
function AutoCopyRow({ account, label }: { account: AccountSummary; label: string | null }) {
  const [kind, setKind] = useState<AutoCopy["kind"] | "off">(account.auto_copy?.kind ?? "off");
  const [address, setAddress] = useState(account.auto_copy?.address ?? "");
  const [state, setState] = useState<"idle" | "saving" | "saved">("idle");
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    setKind(account.auto_copy?.kind ?? "off");
    setAddress(account.auto_copy?.address ?? "");
  }, [account.auto_copy?.kind, account.auto_copy?.address]);
  const save = (nextKind: AutoCopy["kind"] | "off", nextAddress: string) => {
    const trimmed = nextAddress.trim();
    const wanted: AutoCopy | null = nextKind === "off" || !trimmed ? null : { kind: nextKind, address: trimmed };
    const saved = account.auto_copy ?? null;
    if (wanted?.kind === saved?.kind && wanted?.address === saved?.address) return;
    // Cc or Bcc chosen, no address yet: nothing to save until there is one.
    if (nextKind !== "off" && !trimmed) return;
    setState("saving");
    setError(null);
    setAutoCopy(account.id, wanted).then(
      () => setState("saved"),
      (failure: unknown) => {
        setState("idle");
        setError(String(failure));
      },
    );
  };
  return (
    <div className="option auto-copy">
      <span className="option-icon">
        <Icon name="mail" size={18} />
      </span>
      <span className="option-text">
        <strong>{label ?? "Every new message"}</strong>
        <span aria-live="polite" className={error ? "error-text" : undefined}>
          {error ??
            (state === "saving"
              ? "Saving…"
              : state === "saved"
                ? "Saved."
                : kind === "off"
                  ? "No copy."
                  : address.trim()
                    ? `${kind === "cc" ? "A copy" : "A blind copy"} to ${address.trim()}.`
                    : "Enter the address to copy.")}
        </span>
      </span>
      <span className="option-control auto-copy-control">
        <Select
          label={`Always copy, ${account.email}`}
          value={kind}
          options={[
            { id: "off", label: "Off" },
            { id: "cc", label: "Cc" },
            { id: "bcc", label: "Bcc" },
          ]}
          onChange={(next) => {
            setKind(next);
            setState("idle");
            save(next, address);
          }}
        />
        {kind !== "off" && (
          <input
            className="input auto-copy-address"
            type="email"
            aria-label={`Address to copy, ${account.email}`}
            placeholder="name@example.org"
            value={address}
            onChange={(event) => {
              setAddress(event.target.value);
              setState("idle");
            }}
            onBlur={() => save(kind, address)}
            onKeyDown={(event) => {
              if (event.key === "Enter") event.currentTarget.blur();
            }}
          />
        )}
      </span>
    </div>
  );
}

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
              { id: "last", label: "The last one used" },
              ...accounts.map((account) => ({ id: account.id, label: accountLabel(prefs, account) })),
            ]}
            onChange={(defaultFrom) => setPrefs({ defaultFrom: defaultFrom || null })}
          />
        </Option>
      )}
      <Option icon="reply" label="Replies and forwards open" description="Over the conversation, to keep it in view, or full size.">
        <Select
          label="Replies and forwards open"
          value={prefs.replyOpens}
          options={[
            { id: "panel", label: "Over the conversation" },
            { id: "full", label: "Full size" },
          ]}
          onChange={(replyOpens) => setPrefs({ replyOpens })}
        />
      </Option>
      <Option
        icon="check"
        label="Done after replying"
        description="Once a reply or forward is sent, its conversation leaves the inbox, with Undo, as when you mark it done."
      >
        <Toggle label="Done after replying" on={prefs.doneAfterReply} onChange={(doneAfterReply) => setPrefs({ doneAfterReply })} />
      </Option>
      <Option
        icon="people"
        label="Suggest people as you type"
        description="In To, Cc and Bcc: the people you write to first, from your own mail."
      >
        <Toggle label="Suggest people as you type" on={prefs.suggestPeople} onChange={(suggestPeople) => setPrefs({ suggestPeople })} />
      </Option>
      <LinkRow
        icon="user"
        label="Addresses and signatures"
        detail="Send as an alias, and the signature each address adds"
        onOpen={onAccounts}
      />
      <h4 className="settings-subhead">Always copy</h4>
      {accounts.map((account) => (
        <AutoCopyRow key={account.id} account={account} label={accounts.length > 1 ? accountLabel(prefs, account) : null} />
      ))}
      <p className="muted settings-note">
        Added to every new message, reply and forward from that account, those agents and the command line write too,
        unless the address is already there. Remove it from one message in the composer like any other. Drafts already
        written stay as they are.
      </p>
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
      <Option
        icon="search"
        label="Zoom"
        description={`Makes everything in the window larger or smaller. ${MOD_KEY} + and ${MOD_KEY} − change it from anywhere; ${MOD_KEY} 0 goes back to 100 %.`}
      >
        <Select
          label="Zoom"
          value={prefs.zoom}
          options={ZOOMS.map((zoom) => ({ id: zoom, label: `${Math.round(zoom * 100)} %` }))}
          onChange={(zoom) => setPrefs({ zoom })}
        />
      </Option>
      <Option
        icon="keyboard"
        label="Single-key shortcuts"
        description={
          prefs.singleKeys
            ? "Keys such as e (archive) and # (delete) act on their own."
            : `Off: only shortcuts with ${MOD_KEY}, and keys such as Escape and the arrows, act. Speech input or a stray key can't archive or delete mail.`
        }
      >
        <Toggle label="Single-key shortcuts" on={prefs.singleKeys} onChange={(singleKeys) => setPrefs({ singleKeys })} />
      </Option>
    </>
  );
}

/** Support: the version and updates, the background service, and where to get help. */
/** Detailed logs for a problem, and diagnostics saved to a file to attach to a report. */
function Troubleshooting({
  accounts,
  confirm,
}: {
  accounts: AccountSummary[];
  confirm: (request: ConfirmRequest) => Promise<boolean>;
}) {
  const [detailed, setDetailed] = useState<boolean | null>(null);
  const [changed, setChanged] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    supportSettings().then(
      (settings) => setDetailed(settings.detailed_logs),
      (failure: unknown) => setError(String(failure)),
    );
  }, []);
  const save = () => {
    setSaving(true);
    setSaved(null);
    setError(null);
    exportDiagnostics()
      .then(setSaved, (failure: unknown) => setError(String(failure)))
      .finally(() => setSaving(false));
  };
  return (
    <>
      <h4 className="settings-subhead">Troubleshooting</h4>
      {error && <p className="error" role="alert">{error}</p>}
      <Option
        icon="text"
        label="Detailed logs"
        description={
          changed
            ? "Takes effect the next time Regards starts."
            : "The service writes down more of what it does, to find a problem. Takes effect the next time Regards starts."
        }
      >
        {detailed !== null && (
          <Toggle
            label="Detailed logs"
            on={detailed}
            onChange={(on) => {
              setError(null);
              setDetailedLogs(on).then(
                () => {
                  setDetailed(on);
                  setChanged(true);
                },
                (failure: unknown) => setError(String(failure)),
              );
            }}
          />
        )}
      </Option>
      <div className="option">
        <span className="option-icon">
          <Icon name="paperclip" size={18} />
        </span>
        <span className="option-text">
          <strong>Diagnostics</strong>
          <span>
            {saved
              ? `Saved to ${saved}. Attach it to your report.`
              : "Versions, the service's state, how many accounts of each kind, and its log, in a file you choose. No mail, addresses, passwords or tokens, and nothing is sent."}
          </span>
        </span>
        <span className="option-control">
          <button type="button" className="btn small" onClick={save} disabled={saving}>
            {saving ? "Saving…" : "Save diagnostics…"}
          </button>
        </span>
      </div>
      <DownloadAgain accounts={accounts} confirm={confirm} />
    </>
  );
}

export function SupportPage({
  service,
  onRecheck,
  accounts,
  confirm,
}: {
  service: ServiceCheck;
  onRecheck: () => void;
  accounts: AccountSummary[];
  confirm: (request: ConfirmRequest) => Promise<boolean>;
}) {
  const [copied, setCopied] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const open = (url: string) => {
    setError(null);
    openUrl(url).catch((failure: unknown) => setError(String(failure)));
  };
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
      <Troubleshooting accounts={accounts} confirm={confirm} />
      <h4 className="settings-subhead">Get help</h4>
      {error && <p className="error" role="alert">{error}</p>}
      {links.map((link) => (
        <div key={link.url} className="option help-row">
          <span className="option-icon">
            <Icon name="help" size={18} />
          </span>
          <span className="option-text">
            <strong>{link.label}</strong>
            <span className="help-url">{link.url}</span>
          </span>
          <span className="help-buttons">
            <button type="button" className="btn small" onClick={() => open(link.url)}>
              Open
            </button>
            <button
              type="button"
              className="icon-btn small"
              title={copied === link.url ? "Copied" : "Copy link"}
              aria-label={copied === link.url ? "Copied" : "Copy link"}
              onClick={() => copy(link.url)}
            >
              <Icon name={copied === link.url ? "check" : "copy"} size={14} />
            </button>
          </span>
        </div>
      ))}
    </>
  );
}

function ServiceStatus({ check }: { check: ServiceCheck }) {
  if (check.state === "checking") return <p className="muted">Checking…</p>;
  if (check.state === "failed") return <p className="error" role="alert">Could not check the service: {check.message}</p>;
  const { status } = check;
  if (status.error || !status.running) {
    return (
      <p className="error" role="alert">
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
