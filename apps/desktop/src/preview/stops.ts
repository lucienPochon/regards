// Every screen and state of the UI, one click each, for the preview's Tour
// panel. Each stop reloads the preview with its parameters (read by mock.ts
// and main.tsx), its settings, then plays its steps (see `play` in main.tsx).

import type { Prefs } from "../prefs";

export interface Stop {
  id: string;
  label: string;
  /** What to look at. */
  note: string;
  /** Preview parameters; null drops one of the defaults. */
  params?: Record<string, string | null>;
  /** Settings kept on this computer, as they would be after choosing them. */
  prefs?: Partial<Prefs>;
  /** Played after loading: wait:ms, click:css, text:css=words, see:css=words, right:css, key:Mod+Shift+k, type:words, settings:Page, call:drop, call:link=hold:2. */
  steps?: string[];
}

export interface Group {
  title: string;
  stops: Stop[];
}

const NEWSLETTER = "message:4";
const settings = (page: string, ...more: string[]) => ["wait:600", `settings:${page}`, ...more];
const openRow = (words: string) => ["wait:700", `text:.row-main=${words}`];
/** The first run's address typed, and looked up. */
const signUp = (address: string) => ["wait:500", `type:${address}`, "wait:1300"];
const signIn = (address: string, password: string) => [...signUp(address), "text:button=Continue", "wait:300", `type:${password}`, "text:button=Sign in"];

export const TOUR: Group[] = [
  {
    title: "Inbox",
    stops: [
      {
        id: "smart-inbox",
        label: "Inbox by priority",
        note: "New senders to accept or block, Priority, then today's mail. Notifications, Newsletters, Invitations and Pinned each sit in one row.",
        params: { view: "smart" },
      },
      {
        id: "by-category",
        label: "Inbox by category",
        note: "Unread mail in a card per category, each with its check to mark it read; then what has been seen.",
        params: { view: "cards" },
      },
      { id: "by-date", label: "Inbox by date", note: "The default: everything together, newest first.", params: { view: "date" } },
      { id: "unified", label: "Two accounts, one inbox", note: "Both accounts' mail together; the sidebar lists each account.", params: { accounts: "2" } },
      { id: "folded-sidebar", label: "Sidebar folded to icons", note: "The / key does the same.", params: { sidebar: "icons" } },
      { id: "folders-more", label: "More folders", note: "Folders beyond the usual ones.", steps: ["wait:700", "text:button=More"] },
      { id: "search-group", label: "A group of your own", note: "A saved search shown as one row of the Smart Inbox (its right-click menu, or Settings › Email groups): what it finds in the inbox.", params: { view: "smart" }, prefs: { searchGroups: [9002] }, steps: ["wait:900", "see:.row.bundle=Garden"] },
      { id: "group-open", label: "Notifications, opened", note: "A group row opens its own list.", params: { view: "smart" }, steps: openRow("Notifications") },
      { id: "pinned", label: "Pinned", note: "Pinned mail, from the sidebar.", steps: ["wait:700", "text:button=Pinned"] },
      { id: "invitations-open", label: "Invitations, opened", note: "Invitations not answered yet, as one row of the Smart Inbox; answering one takes it off.", params: { view: "smart" }, steps: openRow("Invitations") },
      { id: "screen-hold", label: "New senders held aside", note: "Settings › Inbox › new senders: held until accepted.", params: { screen: "hold" } },
      { id: "screen-off", label: "New senders not screened", note: "New senders' mail goes straight into the inbox.", params: { screen: "off" } },
      { id: "reminder-due", label: "A reminder came due", note: "Nobody replied in time: the message you sent comes back to the inbox, with a bell.", params: { reminder: "1" }, steps: ["wait:900", "see:.reminder.due="] },
      { id: "context-menu", label: "Right-click on a message", note: "Every action on a message, with its key.", steps: ["wait:700", "right:.row-main"] },
      { id: "muted", label: "A muted conversation", note: "Right-click › Mute conversation: its new mail tells of nothing, and the row says so.", steps: ["wait:700", "right:.row-main", "wait:300", "text:.menu-item=Mute conversation", "wait:600"] },
      { id: "palette", label: "Commands", note: "Everything by name (Ctrl+K).", steps: ["wait:700", "key:Mod+k"] },
      { id: "shortcuts", label: "Keyboard shortcuts", note: "The ? key.", steps: ["wait:700", "key:?"] },
      { id: "drop", label: "Files dropped on the window", note: "Dropping files starts a new message with them attached.", steps: ["wait:700", "call:drop"] },
      { id: "unreachable", label: "Server can't be reached", note: "The list says so and keeps what it has.", params: { unreachable: "1" } },
    ],
  },
  {
    title: "List display",
    stops: [
      { id: "chronological", label: "Groups in time order", note: "Settings › Inbox › Email groups: groups sit at their newest message's day, among the other mail (in the sample, that is today).", prefs: { groupsPosition: "chronological" }, steps: ["wait:900", "see:.row-main=Notifications"] },
      {
        id: "ungrouped",
        label: "Notifications with everything else",
        note: "Settings › Inbox › Email groups: Notifications not grouped into one row.",
        prefs: { grouped: { notifications: false, newsletters: true, invitations: true, pinned: true } },
        steps: ["wait:900", "see:.row-main=Northline Rail"],
      },
      { id: "compact", label: "Compact, no pictures", note: "Settings › Appearance: compact density, pictures nowhere, no preview line.", prefs: { density: "compact", avatars: "nowhere", previewLines: 0 } },
      { id: "two-lines", label: "Two preview lines", note: "Settings › Appearance › Preview lines.", prefs: { previewLines: 2 } },
      { id: "colours", label: "A colour for each account", note: "Settings › Appearance › Colour for each account: a ring around pictures.", params: { accounts: "2" }, prefs: { colourCoding: true } },
      { id: "hidden-account", label: "An account left out of the inbox", note: "Settings › Inbox › Email groups: the work account stays in its own inbox.", params: { accounts: "2" }, prefs: { hiddenAccounts: ["work"] } },
      { id: "no-counts", label: "No unread counts", note: "Settings › Notifications › Unread counts in the sidebar, off.", prefs: { sidebarCounts: false } },
      { id: "zoom", label: "Zoom at 125 %", note: "Settings › Accessibility › Zoom.", prefs: { zoom: 1.25 } },
      { id: "dark", label: "Dark theme", note: "Settings › Appearance › Theme. The switch at the top of this panel keeps it for every stop.", params: { theme: "dark" } },
    ],
  },
  {
    title: "Reading",
    stops: [
      { id: "reader", label: "A message beside the list", note: "Blocked images: Load once, or Always from this sender.", params: { link: NEWSLETTER } },
      { id: "reader-single", label: "A message in place of the list", note: "Settings › Appearance › Layout: in place of the list.", params: { link: NEWSLETTER, layout: "single" } },
      { id: "thread", label: "A conversation", note: "Older messages fold to one line each.", steps: openRow("Garden plot") },
      { id: "thread-open", label: "A conversation, unfolded", note: "Settings › General › Fold older messages, off.", prefs: { collapseOlder: false }, steps: openRow("Garden plot") },
      { id: "invitation", label: "An invitation", note: "When, where and from whom, in your time; Accept, Maybe or Decline answers the organizer by email.", steps: openRow("Plot planning") },
      { id: "thread-draft", label: "A reply being written", note: "An unsent reply shows under its conversation; it opens to go on writing.", steps: openRow("Book club") },
      { id: "find", label: "Find in this message", note: "Ctrl+Shift+F, or the message's ⋯ menu. Enter and Shift+Enter step through the matches.", steps: [...openRow("Garden plot"), "wait:700", "key:Mod+Shift+F", "wait:300", "type:the"] },
      { id: "sender-card", label: "About the sender", note: "Why it is here, and what to do with this sender's mail.", params: { link: NEWSLETTER }, steps: ["wait:900", "click:.mail-avatar"] },
      { id: "images-always", label: "Images always from this sender", note: "After choosing it once, this sender's images load without asking.", params: { link: NEWSLETTER }, steps: ["wait:900", "text:.images-notice button=Always"] },
      { id: "snooze", label: "Snooze", note: "The b key. Its times come from Settings › Scheduling.", params: { link: NEWSLETTER }, steps: ["wait:900", "key:b"] },
      { id: "remind", label: "Remind me if nobody replies", note: "Shift+R.", params: { link: NEWSLETTER }, steps: ["wait:900", "key:Shift+R"] },
      { id: "move", label: "Move to a folder", note: "The v key.", params: { link: NEWSLETTER }, steps: ["wait:900", "key:v"] },
      { id: "undo", label: "Undo after archiving", note: "The undo bar; z undoes.", params: { link: NEWSLETTER }, steps: ["wait:900", "key:e"] },
      {
        id: "assistant-summary",
        label: "Summarise with the assistant",
        note: "First what would be sent and where; nothing goes before you say so.",
        params: { assistant: "on" },
        steps: [...openRow("Garden plot"), "wait:600", "click:.mail-card:not(.collapsed) .icon-btn[aria-label='More']", "wait:400", "text:.menu-item=Summarise"],
      },
      {
        id: "assistant-reply",
        label: "Draft a reply with the assistant",
        note: "Say what it should say; the draft opens for you to check.",
        params: { assistant: "on" },
        steps: [...openRow("Garden plot"), "wait:600", "click:.mail-card:not(.collapsed) .icon-btn[aria-label='More']", "wait:400", "text:.menu-item=Draft a reply"],
      },
      { id: "source", label: "See the original message", note: "The message as it came, headers and all, as plain text.", params: { link: "message:4" }, steps: ["wait:900", "click:.mail-card:not(.collapsed) .icon-btn[aria-label='More']", "wait:400", "text:.menu-item=See the original"] },
      { id: "link", label: "A regards:// link opened", note: "Another app opened a message in Regards.", params: { link: "message:5" } },
      {
        id: "mailto",
        label: "An email link clicked",
        note: "With Regards as the mail app, a mailto: link starts a new message to look over. Nothing is sent.",
        steps: ["wait:800", "call:link=mailto:ada@example.org?cc=grace@example.org&subject=Lunch on Friday?&body=Hi Ada,%0A%0AAre you free on Friday?"],
      },
    ],
  },
  {
    title: "Search",
    stops: [
      { id: "mark-all-read", label: "Mark everything read", note: "Every unread message here, read at once, with Undo.", steps: ["wait:800", "click:[aria-label='Mark everything here read']", "wait:400"] },
      { id: "search", label: "Search the list", note: "Ctrl+F, then words; subjects, people and text.", steps: ["wait:700", "key:Mod+f", "type:garden"] },
      { id: "search-chips", label: "Narrow a search", note: "The chips under the field add is:unread, has:attachment and the like to the search; a second click takes them out.", steps: ["wait:700", "key:Mod+f", "type:garden", "wait:400", "text:.search-chips button=With files"] },
      { id: "did-you-mean", label: "Did you mean", note: "A misspelt word, mended.", steps: ["wait:700", "key:Mod+f", "type:gardn"] },
      { id: "server-search", label: "Search on the server", note: "Nothing here: the server can look further back.", steps: ["wait:700", "key:Mod+f", "type:zebra"] },
      { id: "indexing", label: "Still indexing", note: "Recent mail may be missing while it is indexed.", params: { indexing: "40" }, steps: ["wait:700", "key:Mod+f", "type:zebra"] },
    ],
  },
  {
    title: "Writing",
    stops: [
      { id: "compose", label: "New message", note: "The c key.", steps: ["wait:700", "key:c"] },
      { id: "compose-colour", label: "Text colour", note: "In the formatting bar, with highlight, headings and Clear.", steps: ["wait:700", "key:c", "wait:600", "click:[aria-label='Text colour']"] },
      { id: "reply", label: "Reply", note: "The r key, in a conversation.", steps: [...openRow("Garden plot"), "wait:600", "key:r"] },
      { id: "suggest", label: "People suggested as you type", note: "Those you write to first; arrows and Enter put one in.", steps: ["wait:700", "key:c", "wait:600", "type:ma", "wait:500"] },
      { id: "template", label: "Insert a template", note: "Ctrl+Shift+T in the composer.", steps: ["wait:700", "key:c", "wait:500", "click:[aria-label='Insert a template']"] },
      {
        id: "send-later",
        label: "Send later",
        note: "Once there is someone to send to. Its times come from Settings › Scheduling.",
        steps: [...openRow("Garden plot"), "wait:600", "key:r", "wait:600", "click:[aria-label='Send later']"],
      },
      { id: "compose-remind", label: "Remind me, from the composer", note: "If nobody replies by a time.", steps: ["wait:700", "key:c", "wait:500", "click:[aria-label='Remind me if nobody replies']"] },
      { id: "compose-copy", label: "With an automatic copy", note: "Settings › Composer › Always copy: the Bcc is already there, and can be removed.", params: { autocopy: "bcc" }, steps: ["wait:700", "key:c"] },
      { id: "compose-drop", label: "Pictures dropped into a message", note: "Images go in the text; other files are attached.", steps: ["wait:700", "key:c", "wait:500", "call:drop"] },
      { id: "sent-undo", label: "Sent, with Undo", note: "The message waits in the Outbox for the undo time (Settings › General).", steps: [...openRow("Garden plot"), "wait:600", "key:r", "wait:600", "key:Mod+Enter"] },
    ],
  },
  {
    title: "Outbox",
    stops: [
      { id: "outbox", label: "Scheduled and held", note: "One for tomorrow morning; one held because the computer was off.", params: { outbox: "1" }, steps: ["wait:800", "text:button=Outbox"] },
      { id: "outbox-agent", label: "Sent by an agent", note: "Its countdown, and Call it back.", params: { agentsend: "1" }, steps: ["wait:800", "text:button=Outbox"] },
      { id: "outbox-signin", label: "Waiting for a sign-in", note: "Sends once you sign in again.", params: { signin: "1" }, steps: ["wait:800", "text:button=Outbox"] },
      { id: "labels", label: "Labels (Gmail)", note: "Chips before the subject; L (Label…) adds or removes them.", params: { gmail: "1" }, steps: ["wait:900"] },
      { id: "send-failed", label: "A send that failed", note: "Said in the app too: the system notification is quiet while the window is in front.", params: { sendfail: "1" }, steps: ["wait:3200"] },
    ],
  },
  {
    title: "Agents",
    stops: [
      { id: "holds", label: "Agents waiting for you", note: "What agents asked to do, allowed or refused over the mail; Agent settings has the rest.", steps: ["wait:800", "text:button=Agents are waiting"] },
      { id: "hold-link", label: "A hold opened from a link", note: "regards://hold/2 from a notification.", steps: ["wait:800", "call:link=hold:2"] },
      { id: "agents-off", label: "Agents turned off", note: "One switch for every agent; each account keeps its level.", steps: settings("Agents", "wait:300", "click:.agents-gate [role=switch]", "wait:300") },
      { id: "agent-connect", label: "Connect an agent", note: "The command to run, or the lines to paste, for Claude Code, Codex, Cursor and others.", steps: settings("Agents", "wait:300", "see:.agent-connect=Connect") },
      { id: "agents", label: "Agent settings", note: "Access per account, sending, limits, activity and undo.", steps: settings("Agents") },
    ],
  },
  {
    title: "Accounts",
    stops: [
      { id: "first-run", label: "First run", note: "No account yet: the address first, nothing else.", params: { accounts: "0" } },
      { id: "signup-found", label: "Sign-up › Settings found", note: "Looked up a moment after typing stops.", params: { accounts: "0" }, steps: signUp("ada@ik.me") },
      { id: "signup-password", label: "Sign-up › Password", note: "The address in a chip; the servers a click away.", params: { accounts: "0" }, steps: [...signUp("ada@ik.me"), "text:button=Continue"] },
      { id: "signup-checking", label: "Sign-up › Checking", note: "Receiving and sending are both checked before anything is saved.", params: { accounts: "0" }, steps: [...signIn("ada@ik.me", "correct horse"), "wait:300"] },
      { id: "signup-wrong", label: "Sign-up › Password turned down", note: "Both user names tried; ways forward, with the server's own words folded away.", params: { accounts: "0" }, steps: [...signIn("ada@ik.me", "wrong"), "wait:2200"] },
      { id: "signup-no-send", label: "Sign-up › Sending fails", note: "Receiving works: check the servers, or add it anyway.", params: { accounts: "0" }, steps: [...signIn("ada@ik.me", "nosend"), "wait:2200"] },
      { id: "signup-ready", label: "Sign-up › Ready", note: "The name people see is asked last, and filled in.", params: { accounts: "0" }, steps: [...signIn("ada.lovelace@ik.me", "correct horse"), "wait:2200"] },
      { id: "signup-servers", label: "Sign-up › Servers not found", note: "Typed by hand: receiving is TLS only; sending STARTTLS or TLS sets the port.", params: { accounts: "0" }, steps: [...signUp("ada@unknown-host.ch"), "text:button=Continue"] },
      { id: "signup-server-check", label: "Sign-up › Servers checked", note: "Each server checked as it is left; sending offers the port that answers.", params: { accounts: "0" }, steps: [...signUp("ada@hosting-alpes.ch"), "text:button=Continue", "wait:400", "text:button=Change servers", "wait:1200"] },
      { id: "signup-google", label: "Sign-up › Google", note: "In the browser; an app password as the way around.", params: { accounts: "0" }, steps: [...signUp("ada@gmail.com"), "text:button=Continue"] },
      { id: "signup-oauth-only", label: "Sign-up › Outlook.com", note: "Microsoft's own sign-in only.", params: { accounts: "0" }, steps: [...signUp("ada@outlook.com"), "text:button=Continue"] },
      { id: "signup-no-imap", label: "Sign-up › No mail apps", note: "Proton Mail, Tuta, HEY: said under the address, before any password.", params: { accounts: "0" }, steps: signUp("ada@proton.me") },
      { id: "accounts", label: "Accounts", note: "Sync, addresses and signatures, details, remove.", params: { accounts: "2" }, steps: settings("Accounts") },
      { id: "account-details", label: "An account's details", note: "Your name, its label in Regards, colour, and the unified inbox.", params: { accounts: "2" }, steps: settings("Accounts", "text:button=Details") },
      { id: "identities", label: "Addresses", note: "Send as an alias.", steps: settings("Accounts", "text:button=Addresses") },
      { id: "add-from-menu", label: "Add an account › From the ⌄ menu", note: "Under Accounts…: the same steps, in a window over the mail.", steps: ["wait:600", "click:.caption-btn[aria-label=Menu]"] },
      { id: "add-from-sidebar", label: "Add an account › From the sidebar", note: "The last row under the Inbox chevron; with one account too.", steps: ["wait:600", "click:.side-chevron"] },
      { id: "add-window", label: "Add an account › The window", note: "Esc or ✕ closes it; nothing is saved until the last step.", steps: ["wait:600", "click:.side-chevron", "wait:300", "click:.side-add", "wait:400", "type:ada@ik.me", "wait:1300"] },
      { id: "add-done", label: "Add an account › Added", note: "In the sidebar, its mail coming in; the toast opens its Inbox.", steps: ["wait:600", "click:.side-chevron", "wait:300", "click:.side-add", "wait:400", "type:ada@ik.me", "wait:1300", "text:button=Continue", "wait:300", "type:correct horse", "text:button=Sign in", "wait:2300", "text:button=Done", "wait:300"] },
      { id: "add-account", label: "Add an account", note: "Settings are found from the address; Microsoft and Google sign in.", steps: settings("Accounts", "text:button=Add account") },
      { id: "sign-in-again", label: "Sign in again", note: "Microsoft asked for a new sign-in.", params: { signin: "1" }, steps: settings("Accounts") },
      { id: "sign-in-ok", label: "Signing in again", note: "The browser step, then back in.", params: { signin: "1", oauth: "ok" }, steps: settings("Accounts", "text:.settings-pane button=Sign in again") },
    ],
  },
  {
    title: "Settings",
    stops: [
      { id: "s-assistant", label: "Assistant", note: "Your own AI key, or a model on this computer. Off until you choose.", steps: settings("Assistant") },
      { id: "s-assistant-on", label: "Assistant, with Anthropic", note: "The key is kept in the credential store and never shown again.", params: { assistant: "on" }, steps: settings("Assistant") },
      { id: "s-general", label: "General", note: "Login, the tray, after archiving, pins, undo send, folding, images.", steps: settings("General") },
      { id: "s-mail-app", label: "General › Mail app", note: "Make Regards your mail app: only when you click; Windows then asks to confirm.", steps: settings("General", "text:.settings-pane button=Make Regards") },
      { id: "s-mail-app-mac", label: "General › Mail app, on a Mac", note: "Mail's own settings choose the mail app there.", params: { platform: "macos" }, steps: settings("General") },
      { id: "s-clean-links", label: "General › Remove tracking from links", note: "On by default: utm_…, fbclid and the like leave links before they open.", steps: settings("General", "see:.option=Remove tracking") },
      { id: "s-prefetch", label: "General › Download recent mail ahead", note: "The last 30 days, so it opens at once and offline. Off by default.", steps: settings("General", "see:.option=Download recent mail") },
      { id: "s-image-senders", label: "General › Senders whose images load", note: "Each with Block again.", steps: settings("General", "text:.link-row=Senders") },
      { id: "s-appearance", label: "Appearance", note: "Theme, layout, sidebar, density, pictures, preview lines.", steps: settings("Appearance") },
      { id: "s-colours", label: "Appearance › Colour for each account", note: "Colour coding, and a colour per account.", params: { accounts: "2" }, prefs: { colourCoding: true }, steps: settings("Appearance", "text:.link-row=Colour") },
      { id: "s-notifications", label: "Notifications", note: "New mail per account, each notice, the system's settings.", params: { accounts: "2" }, steps: settings("Notifications") },
      { id: "s-badge", label: "Notifications › Count on the taskbar", note: "On or off, and which accounts count. On Windows, a dot.", params: { accounts: "2" }, steps: settings("Notifications", "see:.settings-subhead=On the") },
      { id: "s-scheduling", label: "Scheduling", note: "Snooze, send later and reminder times.", steps: settings("Scheduling") },
      { id: "s-snooze", label: "Scheduling › Snooze", note: "Change or hide each time.", steps: settings("Scheduling", "text:.link-row=Snooze") },
      { id: "s-send", label: "Scheduling › Send later", note: "Change or hide each time.", steps: settings("Scheduling", "text:.link-row=Send later") },
      { id: "s-reminders", label: "Scheduling › Reminders", note: "Change or hide each time.", steps: settings("Scheduling", "text:.link-row=Reminders") },
      { id: "s-inbox", label: "Inbox", note: "Smart Inbox, new senders, Priority card size.", steps: settings("Inbox") },
      { id: "s-groups", label: "Inbox › Email groups", note: "Where groups sit, each as one row or not, accounts in the unified inbox.", params: { accounts: "2" }, steps: settings("Inbox", "text:.link-row=Email groups") },
      { id: "s-composer", label: "Composer", note: "The account new messages are from, and copies every new message gets.", params: { accounts: "2" }, steps: settings("Composer") },
      { id: "s-signatures", label: "Composer › Signatures", note: "Each address's default, and every signature.", params: { accounts: "2" }, steps: settings("Composer", "text:button=Signatures") },
      { id: "s-signature-edit", label: "A signature's editor", note: "Its name, for you, and its text.", steps: settings("Composer", "text:button=Signatures", "wait:700", "text:.link-row=Studio", "wait:900") },
      { id: "s-auto-copy", label: "Composer › Always copy", note: "A blind copy of every new message to an archive address.", params: { autocopy: "bcc" }, steps: settings("Composer") },
      { id: "s-templates", label: "Templates", note: "Write, rename and delete templates.", steps: settings("Templates") },
      { id: "s-accessibility", label: "Accessibility", note: "Zoom up to 200 % (also Ctrl + / − / 0), and single-key shortcuts on or off.", steps: settings("Accessibility") },
      { id: "s-shortcuts", label: "Shortcuts", note: "Every key.", steps: settings("Shortcuts") },
      { id: "s-support", label: "Support", note: "Updates, the background service, troubleshooting, help.", steps: settings("Support") },
      { id: "s-download-again", label: "Support › Download an account again", note: "For when an account's mail looks wrong here; asks first, and says what stays.", params: { accounts: "2" }, steps: settings("Support", "text:.settings-pane button=Download again") },
      { id: "s-connection-log", label: "Support › Log connections", note: "Commands and replies with your mail servers, never your mail or passwords; at once.", steps: settings("Support", "see:.option=Log connections", "click:.settings-pane [aria-label='Log connections']") },
      { id: "s-licences", label: "Support › Open-source licences", note: "Regards' own, then those of what it is built on, by licence.", steps: settings("Support", "text:.link-row=Open-source licences", "wait:400", "click:.licence-group summary") },
      { id: "s-erase", label: "Support › Remove everything from this computer", note: "Asks first, naming what goes; mail on the servers stays.", steps: settings("Support", "text:.settings-pane button=Remove everything") },
      { id: "s-diagnostics", label: "Support › Save diagnostics", note: "A file to attach to a report: no mail, addresses or passwords.", steps: settings("Support", "text:.settings-pane button=Save diagnostics") },
    ],
  },
  {
    title: "Updates",
    stops: [
      { id: "update-ask", label: "Asked once", note: "Whether to check for updates: off until you say yes.", params: { updates: null } },
      { id: "update-found", label: "An update is out", note: "Checks on: a newer version was found.", params: { updates: "on" }, steps: settings("Support", "text:button=Check now") },
      { id: "update-latest", label: "Already the latest", note: "Check now finds nothing newer.", params: { updates: "none" }, steps: settings("Support", "text:button=Check now") },
    ],
  },
];

export const STOPS = TOUR.flatMap((group) => group.stops);
