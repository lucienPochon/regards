import { useEffect, useState } from "react";
import { type SavedSearch, listSavedSearches } from "./search";
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
import { AppMark } from "./AppMark";
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
import { t } from "./i18n";
import { MOD_KEY, commandHint } from "./shortcuts";
import { LinkRow, Option, Select, SubPageHead, Toggle } from "./SettingsControls";
import { SignaturesPage } from "./SignaturesPage";
import { blockSender, blockedSenders, senderImages, setSenderImages } from "./smart";
import { exportDiagnostics, setConnectionLog, setDetailedLogs, supportSettings } from "./support";
import { type PresetDef, type PresetPrefs, REMINDER_PRESETS, SEND_PRESETS, SNOOZE_PRESETS } from "./snooze";
import { type Theme, applyTheme, savedTheme } from "./theme";
import { AboutUpdates } from "./UpdateCheck";
import { LicencesPage } from "./LicencesPage";
import { EraseEverything } from "./EraseEverything";

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
      <h3>{t("General")}</h3>
      {error && <p className="error" role="alert">{error}</p>}
      <Option icon="text" label={t("Language")} description={t("Regards in English or in French. Like this computer: French on a computer set to French, English otherwise.")}>
        <Select
          label={t("Language")}
          value={prefs.language}
          options={[
            { id: "system", label: t("Like this computer") },
            { id: "en", label: "English" },
            { id: "fr", label: "Français" },
          ]}
          onChange={(language) => {
            setPrefs({ language });
            // Every screen in the new language at once.
            window.location.reload();
          }}
        />
      </Option>
      <Option
        icon="logo"
        label={t("Start Regards when you log in")}
        description={t("In the tray, without opening a window, so mail is up to date when you look.")}
      >
        {background && (
          <Toggle
            label={t("Start Regards when you log in")}
            on={background.start_at_login}
            onChange={(on) => changeBackground("start_at_login", on)}
          />
        )}
      </Option>
      <Option
        icon="clock"
        label={t("Keep running when the window is closed")}
        description={
          background?.close_to_tray === false
            ? t("Off: closing the window quits Regards. Snoozed mail, reminders and scheduled messages then wait until it is open again.")
            : t("Regards stays in the tray, so snoozed mail comes back and scheduled messages go. They need this computer to be on.")
        }
      >
        {background && (
          <Toggle
            label={t("Keep running when the window is closed")}
            on={background.close_to_tray}
            onChange={(on) => changeBackground("close_to_tray", on)}
          />
        )}
      </Option>
      <MailApp />
      <Option icon="archive" label={t("After archiving or deleting a message")}>
        <Select
          label={t("After archiving or deleting a message")}
          value={prefs.afterMove}
          options={[
            { id: "next", label: t("Open the next one") },
            { id: "previous", label: t("Open the previous one") },
            { id: "list", label: t("Go back to the list") },
          ]}
          onChange={(afterMove) => setPrefs({ afterMove })}
        />
      </Option>
      <Option icon="pin" label={t("When a pinned message is archived")}>
        <Select
          label={t("When a pinned message is archived")}
          value={prefs.pinnedArchive}
          options={[
            { id: "keep", label: t("Keep it pinned") },
            { id: "unpin", label: t("Unpin it") },
          ]}
          onChange={(pinnedArchive) => setPrefs({ pinnedArchive })}
        />
      </Option>
      <Option
        icon="send"
        label={t("Undo send")}
        description={t("How long a message waits in the Outbox after you send it, so you can call it back.")}
      >
        <Select
          label={t("Undo send")}
          value={prefs.undoSeconds}
          options={[5, 10, 15, 30, 45, 60].map((seconds) => ({ id: seconds, label: seconds === 60 ? t("1 minute") : t("{count} seconds", { count: seconds }) }))}
          onChange={(undoSeconds) => setPrefs({ undoSeconds })}
        />
      </Option>
      <Option
        icon="mailOpen"
        label={t("Mark as read")}
        description={t("When a message you open is marked read. Passing messages with the arrow keys beside the list leaves them unread.")}
      >
        <Select
          label={t("Mark as read")}
          value={prefs.markRead}
          options={[
            { id: "now", label: t("When opened") },
            { id: "2s", label: t("After 2 seconds") },
            { id: "5s", label: t("After 5 seconds") },
            { id: "never", label: t("Never: I mark it myself") },
          ]}
          onChange={(markRead) => setPrefs({ markRead })}
        />
      </Option>
      <Option
        icon="mail"
        label={t("Fold older messages in a conversation")}
        description={t("Older messages show as one line each; the one you opened is shown in full.")}
      >
        <Toggle label={t("Fold older messages in a conversation")} on={prefs.collapseOlder} onChange={(collapseOlder) => setPrefs({ collapseOlder })} />
      </Option>
      <Option
        icon="image"
        label={t("Load images from the web")}
        description={t("Senders can tell when you open their message from these images. Off: each message asks before loading them.")}
      >
        <Toggle label={t("Load images from the web")} on={prefs.remoteImages} onChange={(remoteImages) => setPrefs({ remoteImages })} />
      </Option>
      {!prefs.remoteImages && (
        <LinkRow
          icon="people"
          label={t("Senders whose images load")}
          detail={t("Allowed from a message with “Always from this sender”")}
          onOpen={() => setSenders(true)}
        />
      )}
      <Option
        icon="shield"
        label={t("Remove tracking from links")}
        description={t("Takes out the parts of a link that only tell who clicked (utm_…, fbclid …); the rest of the link stays. Also for what agents read.")}
      >
        {reading !== null && (
          <Toggle
            label={t("Remove tracking from links")}
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
        label={t("Download recent mail ahead of time")}
        description={
          prefetch
            ? t("Your inboxes' mail from the last 30 days (up to 25 MB each) is downloaded now, then every 10 minutes: it opens at once, even offline, and all its text is searchable. Uses data and disk space.")
            : t("Off: a message is downloaded when you open it. On: the last 30 days of your inboxes, so they open at once, even offline, and all their text is searchable. Uses data and disk space.")
        }
      >
        {prefetch !== null && (
          <Toggle
            label={t("Download recent mail ahead of time")}
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
      ? t("Email links on web pages and in other apps open a new message here. Nothing is sent until you send it.")
      : !status.can_register
        ? t("On a Mac, the mail app is chosen in Mail, under Settings › General: choose Regards there.")
        : asked && /Win/.test(navigator.platform)
          ? t("Confirm it in Default apps, which just opened: choose Regards for MAILTO.")
          : t("Email links on web pages and in other apps would open a new message here.");
  return (
    <Option icon="mail" label={t("Mail app")} description={description}>
      {status.registered ? (
        <span className="muted mail-app-yes">
          <Icon name="check" size={15} />
          {t("Regards")}
        </span>
      ) : (
        status.can_register && (
          <button type="button" className="btn small" onClick={make}>
            {t("Make Regards your mail app")}
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
      <SubPageHead title={t("Senders whose images load")} onBack={onBack} />
      <p className="muted settings-note">
        {t("Their messages show images from the web without asking, so they can tell when you open them. To add one, open a message from them and choose “Always from this sender”.")}
      </p>
      {error && <p className="error" role="alert">{error}</p>}
      {lists === null && !error && <p className="muted">{t("Loading…")}</p>}
      {none && <p className="muted">{t("Nobody yet: every message asks before loading images.")}</p>}
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
                    <strong>{who.startsWith("@") ? t("Anyone at {domain}", { domain: who.slice(1) }) : who}</strong>
                  </span>
                  <span className="option-control">
                    <button type="button" className="btn small" onClick={() => block(account.id, who)}>
                      {t("Block again")}
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
  { id: "system", label: t("As this computer") },
  { id: "light", label: t("Light") },
  { id: "dark", label: t("Dark") },
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
      <h3>{t("Appearance")}</h3>
      <Option icon="sun" label={t("Theme")}>
        <Select
          label={t("Theme")}
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
        label={t("Layout")}
        description={t("A message opens in place of the list, beside it, or in place of it with a handle on the list's edge to show both.")}
      >
        <Select
          label={t("Layout")}
          value={layoutChoice}
          options={[
            { id: "single", label: t("In place of the list") },
            { id: "split", label: t("Beside the list") },
            { id: "switch", label: t("In place, with a handle") },
          ]}
          onChange={(choice) =>
            onLayout(choice === "split" ? { mode: "split" } : { mode: "single", handle: choice === "switch" })
          }
        />
      </Option>
      <Option icon="sidebar" label={t("Sidebar")} description={
          <>
            {t("Keep the folders open beside the mail, or fold them to icons.")} <kbd>/</kbd> {t("does the same.")}
          </>
        }
      >
        <Select
          label={t("Sidebar")}
          value={layout.locked ? "open" : "icons"}
          options={[
            { id: "open", label: t("Open beside the mail") },
            { id: "icons", label: t("Icons") },
          ]}
          onChange={(value) => onLayout({ locked: value === "open" })}
        />
      </Option>
      <Option icon="text" label={t("List density")} description={t("When the list fills the window.")}>
        <Select
          label={t("List density")}
          value={prefs.density}
          options={[
            { id: "comfortable", label: t("Comfortable") },
            { id: "compact", label: t("Compact") },
          ]}
          onChange={(density) => setPrefs({ density })}
        />
      </Option>
      <Option icon="user" label={t("Pictures of people")} description={t("The coloured circles with initials. Regards never fetches people's pictures.")}>
        <Select
          label={t("Pictures of people")}
          value={prefs.avatars}
          options={[
            { id: "everywhere", label: t("Everywhere") },
            { id: "reader", label: t("In messages only") },
            { id: "list", label: t("In the list only") },
            { id: "nowhere", label: t("Nowhere") },
          ]}
          onChange={(avatars) => setPrefs({ avatars })}
        />
      </Option>
      <Option icon="text" label={t("Preview lines")} description={t("Lines of each message's start in the list, when it is beside the reader.")}>
        <Select
          label={t("Preview lines")}
          value={prefs.previewLines}
          options={[
            { id: 0, label: t("None") },
            { id: 1, label: t("1 line") },
            { id: 2, label: t("2 lines") },
          ]}
          onChange={(previewLines) => setPrefs({ previewLines: previewLines as Prefs["previewLines"] })}
        />
      </Option>
      <LinkRow
        icon="user"
        label={t("Colour for each account")}
        detail={prefs.colourCoding ? t("On: a ring in each account's colour around pictures") : t("Off")}
        onOpen={() => setColours(true)}
      />
    </>
  );
}

function AccountColours({ accounts, onBack }: { accounts: AccountSummary[]; onBack: () => void }) {
  const prefs = usePrefs();
  return (
    <>
      <SubPageHead title={t("Colour for each account")} onBack={onBack} />
      <Option
        icon="user"
        label={t("Colour-code accounts")}
        description={t("A ring in the account's colour around pictures in the list and the sidebar, so you can tell accounts apart in the unified inbox.")}
      >
        <Toggle
          label={t("Colour-code accounts")}
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
const ON_MAC = /Mac/.test(platform);
const BADGE_NOTE = /Win/.test(platform)
  ? t("The inboxes' unread mail, as in the sidebar, up to 9+. Not mail set aside or snoozed.")
  : t("The inboxes' unread mail, as in the sidebar. Not mail set aside or snoozed.");

const NEW_MAIL: { id: NewMail; label: string; detail: string }[] = [
  { id: "all", label: t("All new mail"), detail: t("Everything that arrives in the inbox.") },
  { id: "smart", label: t("People and Priority"), detail: t("Mail from people and Priority; not new senders, notifications or newsletters.") },
  { id: "priority", label: t("Priority only"), detail: t("Only mail in Priority.") },
  { id: "none", label: t("None"), detail: t("Nothing for new mail.") },
];

/** The notices besides new mail, each its own switch. */
const NOTICES: { id: Exclude<keyof NotificationSettings, "new_mail">; label: string; description: string; icon: IconName }[] = [
  { id: "snooze_ended", label: t("Snoozed mail comes back"), description: t("When its time comes."), icon: "clock" },
  { id: "reminder_due", label: t("Nobody has replied"), description: t("By the time a reminder was set for."), icon: "bell" },
  {
    id: "sending",
    label: t("A message did not go"),
    description: t("Not sent, perhaps not sent, or held because this computer was off when it was due."),
    icon: "alert",
  },
  { id: "agents", label: t("Agents"), description: t("An agent waits for your answer, or is sending a message."), icon: "agent" },
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
      <h3>{t("Notifications")}</h3>
      {error && <p className="error" role="alert">{error}</p>}
      <Option
        icon="bell"
        label={t("Show notifications")}
        description={
          off
            ? t("Off: Regards shows none, whatever is chosen below. The tray icon's menu has the same switch.")
            : t("They show who and the subject, never the message. The tray icon's menu has the same switch.")
        }
      >
        {shown !== null && (
          <Toggle
            label={t("Show notifications")}
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
          <h4 className="settings-subhead">{t("New mail")}</h4>
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
                  <strong>{accounts.length > 1 ? accountLabel(prefs, account) : t("New mail")}</strong>
                  <span>{NEW_MAIL.find((option) => option.id === choice)?.detail}</span>
                </span>
                <span className="option-control">
                  <Select
                    label={t("New mail in {account}", { account: account.email })}
                    value={choice}
                    options={NEW_MAIL}
                    onChange={(next) => change({ ...settings, new_mail: { ...settings.new_mail, [account.id]: next } })}
                  />
                </span>
              </div>
            );
          })}
          <p className="muted settings-note">
            {t("Mail that came while Regards was off is told of when it starts. Not the mail already there when an account is added, nor what you already read elsewhere. When several arrive at once, they come as one.")}
          </p>
          <h4 className="settings-subhead">{t("Also tell me when")}</h4>
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
        label={t("This computer's notification settings")}
        detail={t("Sounds, banners and quiet hours are set there, for Regards as for any app")}
        onOpen={systemSettings}
      />
      {badge && (
        <>
          <h4 className="settings-subhead">{ON_MAC ? t("On the dock") : t("On the taskbar")}</h4>
          <Option icon="inbox" label={ON_MAC ? t("Unread count on the dock") : t("Unread count on the taskbar")} description={BADGE_NOTE}>
            <Toggle
              label={ON_MAC ? t("Unread count on the dock") : t("Unread count on the taskbar")}
              on={badge.on}
              onChange={(on) => changeBadge({ ...badge, on })}
            />
          </Option>
          {badge.on && (
            <Option icon="people" label={t("What it counts")} description={t("Unread mail in the inbox: all of it, or only some.")}>
              <Select
                label={t("What it counts")}
                value={badge.count}
                options={[
                  { id: "inbox", label: t("All unread mail") },
                  { id: "people", label: t("Unread from people") },
                  { id: "priority", label: t("Unread priority mail") },
                ]}
                onChange={(count) => changeBadge({ ...badge, count })}
              />
            </Option>
          )}
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
                    <span>{counted ? t("Counted") : t("Not counted")}</span>
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
      <h4 className="settings-subhead">{t("In Regards")}</h4>
      <Option icon="inbox" label={t("Unread counts in the sidebar")} description={t("Beside the inbox and each account's inbox.")}>
        <Toggle label={t("Unread counts in the sidebar")} on={prefs.sidebarCounts} onChange={(sidebarCounts) => setPrefs({ sidebarCounts })} />
      </Option>
    </>
  );
}

type Picker = "snooze" | "send" | "reminders";
const PICKERS: Record<Picker, { title: string; detail: string; defs: PresetDef[]; key: "snoozePresets" | "sendPresets" | "reminderPresets" }> = {
  snooze: { title: t("Snooze"), detail: t("The times offered when you snooze a message"), defs: SNOOZE_PRESETS, key: "snoozePresets" },
  send: { title: t("Send later"), detail: t("The times offered when you send a message later"), defs: SEND_PRESETS, key: "sendPresets" },
  reminders: {
    title: t("Reminders"),
    detail: t("The times offered for a reminder if nobody replies"),
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
      <h3>{t("Scheduling")}</h3>
      {(Object.keys(PICKERS) as Picker[]).map((id) => (
        <LinkRow key={id} icon={id === "send" ? "sendLater" : id === "snooze" ? "clock" : "bell"} label={PICKERS[id].title} detail={PICKERS[id].detail} onOpen={() => setPicker(id)} />
      ))}
      <p className="muted settings-note">
        {t("Every picker also takes any date and time you choose. Snoozes, reminders and scheduled messages are carried out by this computer, so it must be on at that time.")}
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
                aria-label={t("Time for {choice}", { choice: def.label })}
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
        {t("Back to the usual times")}
      </button>
    </>
  );
}

/** Inbox › Blocked senders: the senders and domains whose mail goes to Junk, per account; each can be unblocked. */
export function BlockedSendersPage({ accounts, onBack }: { accounts: AccountSummary[]; onBack: () => void }) {
  const prefs = usePrefs();
  const [lists, setLists] = useState<Record<string, string[]> | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    Promise.all(accounts.map((account) => blockedSenders(account.id).then((list) => [account.id, list] as const))).then(
      (entries) => setLists(Object.fromEntries(entries)),
      (failure: unknown) => setError(String(failure)),
    );
  }, [accounts]);
  const unblock = (accountId: string, who: string) => {
    setError(null);
    blockSender(accountId, who, false).then(
      () => setLists((now) => now && { ...now, [accountId]: (now[accountId] ?? []).filter((one) => one !== who) }),
      (failure: unknown) => setError(String(failure)),
    );
  };
  const none = lists !== null && accounts.every((account) => (lists[account.id] ?? []).length === 0);
  return (
    <>
      <SubPageHead title={t("Blocked senders")} onBack={onBack} />
      <p className="muted settings-note">
        {t("Their mail goes to Junk, and so does what they send later. Unblocking lets their new mail in; what is already in Junk stays there. To block someone, open a message from them and choose Block.")}
      </p>
      {error && <p className="error" role="alert">{error}</p>}
      {lists === null && !error && <p className="muted">{t("Loading…")}</p>}
      {none && <p className="muted">{t("Nobody is blocked.")}</p>}
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
                    <strong>{who.startsWith("@") ? t("Anyone at {domain}", { domain: who.slice(1) }) : who}</strong>
                  </span>
                  <span className="option-control">
                    <button type="button" className="btn small" onClick={() => unblock(account.id, who)}>
                      {t("Unblock")}
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

/** Inbox › Email groups: where groups sit, which mail is grouped, and accounts in the unified inbox. */
export function EmailGroupsPage({ accounts, onBack }: { accounts: AccountSummary[]; onBack: () => void }) {
  const prefs = usePrefs();
  const grouped = (key: keyof Prefs["grouped"], value: boolean) => setPrefs({ grouped: { ...prefs.grouped, [key]: value } });
  const hidden = new Set(prefs.hiddenAccounts);
  const [searches, setSearches] = useState<SavedSearch[]>([]);
  useEffect(() => {
    listSavedSearches().then(setSearches, () => setSearches([]));
  }, []);
  return (
    <>
      <SubPageHead title={t("Email groups")} onBack={onBack} />
      <Option
        icon="clock"
        label={t("Where groups sit")}
        description={t("Notifications, Newsletters, Invitations and Pinned, each as one row of the Smart Inbox.")}
      >
        <Select
          label={t("Where groups sit")}
          value={prefs.groupsPosition}
          options={[
            { id: "today", label: t("With today's mail") },
            { id: "chronological", label: t("At their newest message's day") },
          ]}
          onChange={(groupsPosition) => setPrefs({ groupsPosition })}
        />
      </Option>
      <h4 className="settings-subhead">{t("Categories")}</h4>
      {(
        [
          { key: "notifications", label: t("Notifications"), icon: "bell" },
          { key: "newsletters", label: t("Newsletters"), icon: "newspaper" },
          { key: "invitations", label: t("Invitations"), icon: "calendar" },
          { key: "invitation_replies", label: t("Invitation replies"), icon: "calendar" },
          { key: "pinned", label: t("Pinned"), icon: "pin" },
        ] as const
      ).map((category) => (
        <Option key={category.key} icon={category.icon} label={category.label}>
          <Select
            label={category.label}
            value={prefs.grouped[category.key] ? "group" : "all"}
            options={[
              { id: "group", label: t("As one row") },
              {
                id: "all",
                label: category.key === "pinned" ? t("Only in Pinned, in the sidebar") : t("With everything else"),
              },
            ]}
            onChange={(value) => grouped(category.key, value === "group")}
          />
        </Option>
      ))}
      <h4 className="settings-subhead">{t("Your groups")}</h4>
      {searches.length > 0 && (
        <p className="muted settings-note">{t("What a group finds shows in its row only, not in the list below as well.")}</p>
      )}
      {searches.length === 0 ? (
        <p className="muted settings-note">
          {t("Save a search (\"Save this search\", under its results) to make a group of your own: everything it finds in the inbox, as one row.")}
        </p>
      ) : (
        searches.map((saved) => (
          <Option key={saved.id} icon="search" label={saved.name} description={saved.query}>
            <Select
              label={saved.name}
              value={prefs.searchGroups.includes(saved.id) ? "group" : "sidebar"}
              options={[
                { id: "group", label: t("As one row") },
                { id: "sidebar", label: t("Only in the sidebar") },
              ]}
              onChange={(value) =>
                setPrefs({
                  searchGroups:
                    value === "group"
                      ? [...prefs.searchGroups.filter((id) => id !== saved.id), saved.id]
                      : prefs.searchGroups.filter((id) => id !== saved.id),
                })
              }
            />
          </Option>
        ))
      )}
      {accounts.length > 1 && (
        <>
          <h4 className="settings-subhead">{t("Accounts in the unified inbox")}</h4>
          {accounts.map((account) => (
            <div key={account.id} className="option">
              <Avatar person={{ name: account.display_name, address: account.email }} size="sm" />
              <span className="option-text">
                <strong>{accountLabel(prefs, account)}</strong>
                <span>{hidden.has(account.id) ? t("Only in its own inbox, under the sidebar's Inbox") : t("In the unified inbox")}</span>
              </span>
              <Select
                label={t("Where {account}'s mail shows", { account: account.email })}
                value={hidden.has(account.id) ? "hide" : "show"}
                options={[
                  { id: "show", label: t("In the unified inbox") },
                  { id: "hide", label: t("Only in its own inbox") },
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
    <Option icon="boltFilled" label={t("Priority card")} description={t("How many priority messages it shows before “View all”.")}>
      <Select
        label={t("Priority card")}
        value={prefs.priorityRows}
        options={[
          ...Array.from({ length: 10 }, (_, index) => ({ id: index + 1, label: `${index + 1}` })),
          { id: 0, label: t("All") },
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
        <strong>{label ?? t("Every new message")}</strong>
        <span aria-live="polite" className={error ? "error-text" : undefined}>
          {error ??
            (state === "saving"
              ? t("Saving…")
              : state === "saved"
                ? t("Saved.")
                : kind === "off"
                  ? t("No copy.")
                  : address.trim()
                    ? kind === "cc"
                      ? t("A copy to {address}.", { address: address.trim() })
                      : t("A blind copy to {address}.", { address: address.trim() })
                    : t("Enter the address to copy."))}
        </span>
      </span>
      <span className="option-control auto-copy-control">
        <Select
          label={t("Always copy, {account}", { account: account.email })}
          value={kind}
          options={[
            { id: "off", label: t("Off") },
            { id: "cc", label: t("Cc") },
            { id: "bcc", label: t("Bcc") },
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
            aria-label={t("Address to copy, {account}", { account: account.email })}
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

export function ComposerPage({
  accounts,
  confirm,
  onAccounts,
}: {
  accounts: AccountSummary[];
  confirm: (request: ConfirmRequest) => Promise<boolean>;
  onAccounts: () => void;
}) {
  const prefs = usePrefs();
  const [signatures, setSignatures] = useState(false);
  if (signatures) return <SignaturesPage accounts={accounts} confirm={confirm} onBack={() => setSignatures(false)} />;
  return (
    <>
      <h3>{t("Composer")}</h3>
      {accounts.length > 1 && (
        <Option
          icon="compose"
          label={t("New messages are from")}
          description={t("Unless you are in one account's folder, or replying: then it is that account.")}
        >
          <Select
            label={t("New messages are from")}
            value={prefs.defaultFrom ?? ""}
            options={[
              { id: "", label: t("The first account") },
              { id: "last", label: t("The last one used") },
              ...accounts.map((account) => ({ id: account.id, label: accountLabel(prefs, account) })),
            ]}
            onChange={(defaultFrom) => setPrefs({ defaultFrom: defaultFrom || null })}
          />
        </Option>
      )}
      <Option icon="reply" label={t("Replies and forwards open")} description={t("Over the conversation, to keep it in view, or full size.")}>
        <Select
          label={t("Replies and forwards open")}
          value={prefs.replyOpens}
          options={[
            { id: "panel", label: t("Over the conversation") },
            { id: "full", label: t("Full size") },
          ]}
          onChange={(replyOpens) => setPrefs({ replyOpens })}
        />
      </Option>
      <Option
        icon="check"
        label={t("Done after replying")}
        description={t("Once a reply or forward is sent, its conversation leaves the inbox, with Undo, as when you mark it done.")}
      >
        <Toggle label={t("Done after replying")} on={prefs.doneAfterReply} onChange={(doneAfterReply) => setPrefs({ doneAfterReply })} />
      </Option>
      <Option
        icon="people"
        label={t("Suggest people as you type")}
        description={t("In To, Cc and Bcc: the people you write to first, from your own mail.")}
      >
        <Toggle label={t("Suggest people as you type")} on={prefs.suggestPeople} onChange={(suggestPeople) => setPrefs({ suggestPeople })} />
      </Option>
      <LinkRow icon="text" label={t("Signatures")} detail={t("Which one each address adds, and every signature")} onOpen={() => setSignatures(true)} />
      <LinkRow icon="user" label={t("Addresses")} detail={t("Send as an alias: in Accounts")} onOpen={onAccounts} />
      <h4 className="settings-subhead">{t("Always copy")}</h4>
      {accounts.map((account) => (
        <AutoCopyRow key={account.id} account={account} label={accounts.length > 1 ? accountLabel(prefs, account) : null} />
      ))}
      <p className="muted settings-note">
        {t("Added to every new message, reply and forward from that account, those agents and the command line write too, unless the address is already there. Remove it from one message in the composer like any other. Drafts already written stay as they are.")}
      </p>
      <p className="muted settings-note">{t("How long you can call a message back after sending it is under General, “Undo send”.")}</p>
    </>
  );
}

/** Accessibility: the whole window larger or smaller. */
export function AccessibilityPage() {
  const prefs = usePrefs();
  const [larger, smaller, actual] = [commandHint("zoomIn"), commandHint("zoomOut"), commandHint("zoomReset")];
  return (
    <>
      <h3>{t("Accessibility")}</h3>
      <Option
        icon="search"
        label={t("Zoom")}
        description={
          larger && smaller && actual
            ? t("Makes everything in the window larger or smaller. {larger} and {smaller} change it from anywhere; {actual} goes back to 100 %.", { larger, smaller, actual })
            : t("Makes everything in the window larger or smaller.")
        }
      >
        <Select
          label={t("Zoom")}
          value={prefs.zoom}
          options={ZOOMS.map((zoom) => ({ id: zoom, label: `${Math.round(zoom * 100)} %` }))}
          onChange={(zoom) => setPrefs({ zoom })}
        />
      </Option>
      <Option
        icon="keyboard"
        label={t("Single-key shortcuts")}
        description={
          prefs.singleKeys
            ? t("Keys such as e (archive) and # (delete) act on their own.")
            : t("Off: only shortcuts with {key}, and keys such as Escape and the arrows, act. Speech input or a stray key can't archive or delete mail.", { key: MOD_KEY })
        }
      >
        <Toggle label={t("Single-key shortcuts")} on={prefs.singleKeys} onChange={(singleKeys) => setPrefs({ singleKeys })} />
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
  const [connections, setConnections] = useState<boolean | null>(null);
  const [changed, setChanged] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    supportSettings().then(
      (settings) => {
        setDetailed(settings.detailed_logs);
        setConnections(settings.connection_log);
      },
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
      <h4 className="settings-subhead">{t("Troubleshooting")}</h4>
      {error && <p className="error" role="alert">{error}</p>}
      <Option
        icon="text"
        label={t("Detailed logs")}
        description={
          changed
            ? t("Takes effect the next time Regards starts.")
            : t("The service writes down more of what it does, to find a problem. Takes effect the next time Regards starts.")
        }
      >
        {detailed !== null && (
          <Toggle
            label={t("Detailed logs")}
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
      <Option
        icon="refresh"
        label={t("Log connections")}
        description={
          connections
            ? t("Logging now: the commands and replies between Regards and your mail servers, never your mail, addresses or passwords. Saved diagnostics include it, and so name your mail servers.")
            : t("The commands and replies between Regards and your mail servers, never your mail, addresses or passwords. Starts and stops at once.")
        }
      >
        {connections !== null && (
          <Toggle
            label={t("Log connections")}
            on={connections}
            onChange={(on) => {
              setError(null);
              setConnectionLog(on).then(
                () => setConnections(on),
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
          <strong>{t("Diagnostics")}</strong>
          <span>
            {saved
              ? t("Saved to {path}. Attach it to your report.", { path: saved })
              : t("Versions, the service's state, how many accounts of each kind, and its log, in a file you choose. No mail, addresses, passwords or tokens, and nothing is sent.")}
          </span>
        </span>
        <span className="option-control">
          <button type="button" className="btn small" onClick={save} disabled={saving}>
            {saving ? t("Saving…") : t("Save diagnostics…")}
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
  const [licences, setLicences] = useState(false);
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
    { label: t("Report a problem or ask for a feature"), url: `${PROJECT_URL}/issues` },
    { label: t("Read the documentation"), url: `${PROJECT_URL}#readme` },
  ];
  if (licences) return <LicencesPage onBack={() => setLicences(false)} />;
  return (
    <>
      <div className="about-head">
        <AppMark size={56} />
        <div>
          <h3>{t("Regards")}</h3>
          <p className="muted">{t("Pre-alpha · local-first email")}</p>
        </div>
      </div>
      <AboutUpdates />
      <h4 className="settings-subhead">{t("Background service")}</h4>
      <ServiceStatus check={service} />
      <button type="button" className="btn" onClick={onRecheck} disabled={service.state === "checking"}>
        <Icon name="refresh" size={15} />
        {t("Check again")}
      </button>
      <Troubleshooting accounts={accounts} confirm={confirm} />
      <h4 className="settings-subhead">{t("Get help")}</h4>
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
              {t("Open")}
            </button>
            <button
              type="button"
              className="icon-btn small"
              title={copied === link.url ? t("Copied") : t("Copy link")}
              aria-label={copied === link.url ? t("Copied") : t("Copy link")}
              onClick={() => copy(link.url)}
            >
              <Icon name={copied === link.url ? "check" : "copy"} size={14} />
            </button>
          </span>
        </div>
      ))}
      <LinkRow
        icon="text"
        label={t("Open-source licences")}
        detail={t("Regards' own, and those of the projects it is built on")}
        onOpen={() => setLicences(true)}
      />
      <h4 className="settings-subhead">{t("This computer")}</h4>
      <EraseEverything confirm={confirm} />
    </>
  );
}

function ServiceStatus({ check }: { check: ServiceCheck }) {
  if (check.state === "checking") return <p className="muted">{t("Checking…")}</p>;
  if (check.state === "failed") return <p className="error" role="alert">{t("Could not check the service: {problem}", { problem: check.message })}</p>;
  const { status } = check;
  if (status.error || !status.running) {
    return (
      <p className="error" role="alert">
        <span className="status-dot bad" aria-hidden="true" /> {t("The service could not be started.")} {status.error}
      </p>
    );
  }
  return (
    <>
      <p>
        <span className="status-dot ok" aria-hidden="true" />{" "}
        {t("Running · version {version} · pid {pid} · up {seconds} s", { version: status.version ?? "?", pid: status.pid ?? "?", seconds: status.uptimeSecs ?? 0 })}
      </p>
      <p className="muted small">
        {status.desktopAccess ? t("This app has desktop access.") : t("This app has agent access only.")}
        {status.started && t(" Started by this app.")}
        {status.replaced && t(" An older or separately started service was replaced.")}
      </p>
    </>
  );
}
