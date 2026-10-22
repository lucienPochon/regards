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

export const TOUR: Group[] = [
  {
    title: "Inbox",
    stops: [
      {
        id: "smart-inbox",
        label: "Smart Inbox",
        note: "New senders to accept or block, the Priority card, then today's mail. Notifications and Newsletters each sit in one row.",
      },
      { id: "unified", label: "Two accounts, one inbox", note: "Both accounts' mail together; the sidebar lists each account.", params: { accounts: "2" } },
      { id: "folded-sidebar", label: "Sidebar folded to icons", note: "The / key does the same.", params: { sidebar: "icons" } },
      { id: "folders-more", label: "More folders", note: "Folders beyond the usual ones.", steps: ["wait:700", "text:button=More"] },
      { id: "group-open", label: "Notifications, opened", note: "A group row opens its own list.", steps: openRow("Notifications") },
      { id: "pinned", label: "Pinned", note: "Pinned mail, from the sidebar.", steps: ["wait:700", "text:button=Pinned"] },
      { id: "screen-hold", label: "New senders held aside", note: "Settings › Inbox › new senders: held until accepted.", params: { screen: "hold" } },
      { id: "screen-off", label: "New senders not screened", note: "New senders' mail goes straight into the inbox.", params: { screen: "off" } },
      { id: "reminder-due", label: "A reminder came due", note: "Nobody replied in time: the message you sent comes back to the inbox, with a bell.", params: { reminder: "1" }, steps: ["wait:900", "see:.reminder.due="] },
      { id: "context-menu", label: "Right-click on a message", note: "Every action on a message, with its key.", steps: ["wait:700", "right:.row-main"] },
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
        prefs: { grouped: { notifications: false, newsletters: true, pinned: true } },
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
      { id: "did-you-mean", label: "Did you mean", note: "A misspelt word, mended.", steps: ["wait:700", "key:Mod+f", "type:gardn"] },
      { id: "server-search", label: "Search on the server", note: "Nothing here: the server can look further back.", steps: ["wait:700", "key:Mod+f", "type:zebra"] },
      { id: "indexing", label: "Still indexing", note: "Recent mail may be missing while it is indexed.", params: { indexing: "40" }, steps: ["wait:700", "key:Mod+f", "type:zebra"] },
    ],
  },
  {
    title: "Writing",
    stops: [
      { id: "compose", label: "New message", note: "The c key.", steps: ["wait:700", "key:c"] },
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
    ],
  },
  {
    title: "Agents",
    stops: [
      { id: "holds", label: "Agents waiting for you", note: "What agents asked to do, to allow or refuse.", steps: ["wait:800", "text:button=Agents are waiting"] },
      { id: "hold-link", label: "A hold opened from a link", note: "regards://hold/2 from a notification.", steps: ["wait:800", "call:link=hold:2"] },
      { id: "agents-off", label: "Agents turned off", note: "One switch for every agent; each account keeps its level.", steps: settings("Agents", "wait:300", "click:.agents-gate [role=switch]", "wait:300") },
      { id: "agent-connect", label: "Connect an agent", note: "The command to run, or the lines to paste, for Claude Code, Codex, Cursor and others.", steps: settings("Agents", "wait:300", "see:.agent-connect=Connect") },
      { id: "agents", label: "Agent settings", note: "Access per account, sending, limits, activity and undo.", steps: settings("Agents") },
    ],
  },
  {
    title: "Accounts",
    stops: [
      { id: "first-run", label: "First run", note: "No account yet.", params: { accounts: "0" } },
      { id: "accounts", label: "Accounts", note: "Sync, addresses and signatures, details, remove.", params: { accounts: "2" }, steps: settings("Accounts") },
      { id: "account-details", label: "An account's details", note: "Your name, its label in Regards, colour, and the unified inbox.", params: { accounts: "2" }, steps: settings("Accounts", "text:button=Details") },
      { id: "identities", label: "Addresses and signatures", note: "Send as an alias, each with its signature.", steps: settings("Accounts", "text:button=Addresses") },
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
      { id: "s-auto-copy", label: "Composer › Always copy", note: "A blind copy of every new message to an archive address.", params: { autocopy: "bcc" }, steps: settings("Composer") },
      { id: "s-templates", label: "Templates", note: "Write, rename and delete templates.", steps: settings("Templates") },
      { id: "s-accessibility", label: "Accessibility", note: "Zoom up to 200 % (also Ctrl + / − / 0), and single-key shortcuts on or off.", steps: settings("Accessibility") },
      { id: "s-shortcuts", label: "Shortcuts", note: "Every key.", steps: settings("Shortcuts") },
      { id: "s-support", label: "Support", note: "Updates, the background service, troubleshooting, help.", steps: settings("Support") },
      { id: "s-download-again", label: "Support › Download an account again", note: "For when an account's mail looks wrong here; asks first, and says what stays.", params: { accounts: "2" }, steps: settings("Support", "text:.settings-pane button=Download again") },
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
