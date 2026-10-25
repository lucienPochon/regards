import { spoken, t, tc, tn } from "./i18n";

/**
 * The background service writes its texts in English: what an undo takes
 * back ("Archived (3 messages)"), what an agent asks for ("Send “Plans” to
 * Ada and Bob"). These are their usual forms in the app's language; any
 * other text stays as the service wrote it.
 */
export function fromService(text: string): string {
  if (spoken() === "en") return text;
  // "Archived (3 messages)": what was done, then how many.
  const many = /^(.*) \((\d+) messages\)$/.exec(text);
  if (many) return `${done(many[1])} ${t("({count} messages)", { count: Number(many[2]) })}`;
  return done(text);
}

/** The count in "1 message", "12 messages". */
const countOf = (words: string) => (words === "1 message" ? 1 : Number(/^(\d+) messages$/.exec(words)?.[1] ?? Number.NaN));

/** "1 message", "12 messages": the count, in the app's language. */
const messages = (words: string) => {
  const count = countOf(words);
  return Number.isNaN(count) ? words : tn(count, "1 message", "{count} messages");
};

/** "Ada", "Ada and Bob", "Ada, Bob and 3 others". */
const people = (names: string) => {
  const others = /^(.*), (.*) and (\d+) others$/.exec(names);
  if (others) return t("{first}, {second} and {count} others", { first: others[1], second: others[2], count: Number(others[3]) });
  const two = /^(.*) and (.*)$/.exec(names);
  return two ? t("{first} and {second}", { first: two[1], second: two[2] }) : names;
};

/** What an undo takes back, in a word or two: kept as `undo|…` in the tables. */
export const UNDONE = [
  "Archived",
  "Moved to Trash",
  "Pinned",
  "Unpinned",
  "Muted",
  "Unmuted",
  "Set aside",
  "Back in the inbox",
  "Snoozed",
  "Reminder set",
  "Reminder off",
  "Deleted a draft",
];

/** The service's sentence patterns, each with what says it here. */
const PATTERNS: [RegExp, (match: RegExpExecArray) => string][] = [
  [/^Moved to (.+)$/, (m) => t("Moved to {folder}", { folder: m[1] })],
  [/^Marked (1 message|\d+ messages) read$/, (m) => tn(countOf(m[1]), "Marked 1 message read", "Marked {count} messages read")],
  [/^Marked (1 message|\d+ messages) unread$/, (m) => tn(countOf(m[1]), "Marked 1 message unread", "Marked {count} messages unread")],
  [/^Flagged (1 message|\d+ messages)$/, (m) => t("Flagged {messages}", { messages: messages(m[1]) })],
  [/^Unflagged (1 message|\d+ messages)$/, (m) => t("Unflagged {messages}", { messages: messages(m[1]) })],
  [/^Changed the flags of (1 message|\d+ messages)$/, (m) => t("Changed the flags of {messages}", { messages: messages(m[1]) })],
  [/^Blocked (.+)$/, (m) => t("Blocked {who}", { who: m[1] })],
  [/^Saved a note about (.+)$/, (m) => t("Saved a note about {who}", { who: m[1] })],
  [/^Removed a note about (.+)$/, (m) => t("Removed a note about {who}", { who: m[1] })],
  // What an agent asks for.
  [/^Send “(.*)” to (.+)$/, (m) => t("Send “{subject}” to {people}", { subject: m[1], people: people(m[2]) })],
  [/^Schedule “(.*)” to (.+)$/, (m) => t("Schedule “{subject}” to {people}", { subject: m[1], people: people(m[2]) })],
  [/^Delete (1 message|\d+ messages) for good$/, (m) => t("Delete {messages} for good", { messages: messages(m[1]) })],
  [/^Archive (1 message|\d+ messages)$/, (m) => t("Archive {messages}", { messages: messages(m[1]) })],
  [/^Move (1 message|\d+ messages) to the Trash$/, (m) => t("Move {messages} to the Trash", { messages: messages(m[1]) })],
  [/^Move (1 message|\d+ messages) to another folder$/, (m) => t("Move {messages} to another folder", { messages: messages(m[1]) })],
  [/^Pin or unpin (1 message|\d+ messages)$/, (m) => t("Pin or unpin {messages}", { messages: messages(m[1]) })],
  [/^Set aside or bring back (1 message|\d+ messages)$/, (m) => t("Set aside or bring back {messages}", { messages: messages(m[1]) })],
  [/^Snooze (1 message|\d+ messages)$/, (m) => t("Snooze {messages}", { messages: messages(m[1]) })],
  [/^Set reminders on (1 message|\d+ messages)$/, (m) => t("Set reminders on {messages}", { messages: messages(m[1]) })],
];

function done(text: string): string {
  if (UNDONE.includes(text)) return tc("undo", text);
  for (const [pattern, say] of PATTERNS) {
    const match = pattern.exec(text);
    if (match) return say(match);
  }
  return text;
}
