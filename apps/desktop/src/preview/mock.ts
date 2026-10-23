// A stand-in for the desktop bridge and daemon, so the UI runs in a plain
// browser with sample mail (`npm run dev`, then open /preview.html). Nothing
// here is part of the app build.

import { emit } from "@tauri-apps/api/event";
import { mockIPC } from "@tauri-apps/api/mocks";
import type { MessageSummary, Person } from "../accounts";
import type { Draft, OutboxEntry } from "../compose";
import type { Identity } from "../identities";
import type { Note } from "../notes";
import type { Signature } from "../signatures";
import type { Template } from "../templates";
import type { AgentAccess, AgentActivity, Hold } from "../agents";
import { type Explanation, type Policy, type Screening, SECTIONS, type Section } from "../smart";
import { type FixtureAccount, type FixtureMessage, ME, PERSONAL, WORK, fixtureMessages } from "./fixtures";

type Args = Record<string, unknown>;

const params = new URLSearchParams(window.location.search);
const accountCount = Number(params.get("accounts") ?? "1");
const accounts: FixtureAccount[] = accountCount === 0 ? [] : accountCount >= 2 ? [PERSONAL, WORK] : [PERSONAL];
const started = Date.now();
let messages: FixtureMessage[] = fixtureMessages(started, accounts);
const drafts: Draft[] = [];
/** Images in drafts' text, by Content-ID. */
const draftImages = new Map<string, { mime: string; data: string }>();
const outbox: OutboxEntry[] = [];
const undos = new Map<
  number,
  { description: string; restore: Map<number, number>; marks?: Map<number, Marks>; also?: () => void }
>();
const SOMEDAY = 32_503_680_000_000;
type Marks = Pick<FixtureMessage, "pinned" | "aside" | "snoozed_until" | "remind_at" | "reminded" | "surfaced_at">;
const parked = (m: FixtureMessage) => m.aside === true || (m.snoozed_until ?? 0) > Date.now();
/** Its place in the inbox: what came back is listed by when it did, as the daemon does. */
const inboxPlace = (m: FixtureMessage) =>
  m.surfaced_at && (m.reminded || m.surfaced_at > m.received_at) ? m.surfaced_at : m.received_at;
let nextId = 10_000;
// Checking for updates: not asked yet (`?updates=on` or `off` answers it), and what a check found.
let updateChecks: boolean | null = params.get("updates") === "on" ? true : params.get("updates") === "off" ? false : null;
let updateFound: { version: string; url: string; published_at: string | null } | null = null;
let updateCheckedAt: number | null = null;
// `?agentsend=1`: an agent's send waits out its delay in the Outbox, to be called back.
if (params.get("agentsend") === "1" && accounts[0]) {
  window.setTimeout(() => {
    outbox.push({
      id: 901,
      account_id: accounts[0].account.id,
      draft_id: null,
      subject: "Re: Book club: next pick",
      recipients: [{ name: "Hannah Weiss", address: "hannah.weiss@example.org" }],
      state: "queued",
      undo_until: Date.now() + 30_000,
      send_at: null,
      next_attempt_at: Date.now() + 30_000,
      attempts: 0,
      last_error: null,
      executor: "device",
      needs_this_computer: true,
      by_agent: true,
    });
  });
}
// `?sendfail=1`: a message in the Outbox is refused by the server a moment after the app opens.
if (params.get("sendfail") === "1" && accounts[0]) {
  const entry: OutboxEntry = {
    id: 902,
    account_id: accounts[0].account.id,
    draft_id: null,
    subject: "Quote for the bathroom tiles",
    recipients: [{ name: "Tomás Herrera", address: "tomas@herrera-tiles.example" }],
    state: "sending",
    undo_until: Date.now() - 1000,
    send_at: null,
    next_attempt_at: Date.now(),
    attempts: 1,
    last_error: null,
    executor: "device",
    needs_this_computer: true,
  };
  outbox.push(entry);
  window.setTimeout(() => {
    Object.assign(entry, { state: "failed", last_error: "the server refused tomas@herrera-tiles.example (no such mailbox)" });
    changed();
  }, 2500);
}
// `?autocopy=bcc` (or cc): the first account copies every new message to an archive address.
if ((params.get("autocopy") === "bcc" || params.get("autocopy") === "cc") && accounts[0]) {
  (accounts[0].account as { auto_copy?: unknown }).auto_copy = { kind: params.get("autocopy"), address: "archive@example.org" };
}
// The address a sign-in was started for; `?signin=1` signs the first account out of Microsoft.
let oauthEmail: string | null = null;
if (params.get("signin") === "1" && accounts[0]) {
  accounts[0].account = { ...accounts[0].account, sign_in: "microsoft", needs_sign_in: true };
  // A message sent meanwhile waits in the Outbox for the sign-in.
  window.setTimeout(() => {
    outbox.push({
      id: 900,
      account_id: accounts[0].account.id,
      draft_id: null,
      subject: "Re: Garden plot schedule",
      recipients: [{ name: "Lena Okafor", address: "lena.okafor@example.net" }],
      state: "queued",
      undo_until: Date.now() - 60_000,
      send_at: null,
      next_attempt_at: Date.now() + 60_000,
      attempts: 1,
      last_error: "Microsoft needs you to sign in again",
      executor: "device",
      needs_this_computer: true,
      waiting_for_sign_in: true,
    });
  });
}
// Two sample templates; the daemon keeps them by name.
const templates: Template[] = [
  {
    id: 1,
    name: "Out of office",
    subject: "Away until Monday",
    body: "Thanks for your message. I'm away until Monday and will reply then.",
    html: null,
    updated_at: started,
  },
  {
    id: 2,
    name: "Thanks, will read",
    subject: "",
    body: "Thank you, I'll read it this week and come back to you.",
    html: null,
    updated_at: started,
  },
];
let background = true;
let startAtLogin = true;
let closeToTray = true;
let notifications = true;

// The Smart Inbox, sorted roughly as the daemon's rules do. New senders get
// their own card unless `?screen=off|hold` says otherwise.
let screeningMode = (params.get("screen") ?? "section") as Screening;
type Choice = { policy: Policy | null; priority: boolean; blocked: boolean };
const choices = new Map<string, Choice>();
/** Per account, the senders (addresses and `@domain`s) whose remote images load. */
const imageSenders = new Map<string, Set<string>>();
const imagesFrom = (accountId: string, address: string | null | undefined) => {
  const allowed = imageSenders.get(accountId);
  if (!allowed || !address) return false;
  const lower = address.toLowerCase();
  return allowed.has(lower) || allowed.has(`@${lower.split("@")[1]}`);
};
const choiceKey = (accountId: string, address: string) => `${accountId} ${address.toLowerCase()}`;
// People the sample accounts have written to, besides what is in Sent.
const KNOWN = new Set([
  "marco@example.com",
  "hannah.weiss@example.org",
  "yuki@example.net",
  "ola@residents.example",
  "grace.liu@example.com",
  "omar.haddad@example.com",
  "ines.moreau@example.com",
]);
for (const account of accounts) {
  for (const address of ["lena.okafor@example.net", "marco@example.com", "grace.liu@example.com"]) {
    choices.set(choiceKey(account.account.id, address), { policy: null, priority: true, blocked: false });
  }
}

function explanationOf(message: FixtureMessage): Explanation {
  const account = accountOfMailbox(message.mailboxId);
  const accountId = account?.account.id ?? "";
  const address = (message.from.address ?? "").toLowerCase();
  const choice = choices.get(choiceKey(accountId, address)) ?? choices.get(choiceKey(accountId, `@${address.split("@")[1]}`));
  const sent = account?.mailboxes.find((m) => m.role === "sent")?.id;
  const written =
    KNOWN.has(address) ||
    messages.some((m) => m.mailboxId === sent && [...m.to, ...m.cc].some((p) => p.address?.toLowerCase() === address));
  const sender = {
    address,
    policy: choice?.policy ?? null,
    priority: choice?.priority ?? false,
    written_to: written,
    blocked: choice?.blocked ?? false,
  };
  const local = address.split("@")[0];
  const placed = (section: Section, reason: Explanation["reason"]): Explanation => ({ section, reason, sender });
  if (address === account?.account.email.toLowerCase()) return placed("people", "from_you");
  if (sender.priority) return placed("priority", "priority_sender");
  if (sender.policy) return placed(sender.policy, "your_choice");
  if (/^no-?reply$/.test(local)) return placed("notifications", "no_reply_address");
  if (/^(updates|tickets|bookings|win|alerts)$/.test(local)) return placed("notifications", "sent_automatically");
  if (/^(hello|info|news)$/.test(local)) return placed("newsletters", "mailing_list");
  if (written) return placed("people", "you_wrote_to_them");
  return placed("new_senders", "new_sender");
}
// Search, roughly as the daemon reads a query: words (folded, so accents
// need not be typed), "phrases", -word, and from: to: subject: in: has:
// is: before: after:. `?indexing=3` pretends changes are still to index.
const fold = (text: string) =>
  text
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase();
const stillIndexing = Number(params.get("indexing") ?? "0");
// `?unreachable=1` pretends the second account's server can't be reached.
const unreachableServer = params.get("unreachable") === "1";

/** `server`: as a mail server would, also in the files' names, which the index here doesn't hold. */
function searchSamples(query: string, accountId: string | null, server = false) {
  const terms = query.match(/-?[a-z]+:"[^"]*"|-?"[^"]*"?|\S+/gi) ?? [];
  const tests: ((m: FixtureMessage) => boolean)[] = [];
  const words: string[] = [];
  let folder: string | null = null;
  const day = (value: string) => new Date(`${value}T00:00:00`).getTime();
  for (const raw of terms) {
    const negate = raw.startsWith("-");
    const term = negate ? raw.slice(1) : raw;
    const field = term.match(/^([a-z]+):(.*)$/i);
    const value = fold((field ? field[2] : term).replace(/"/g, ""));
    if (!value) continue;
    let test: (m: FixtureMessage) => boolean;
    const people = (list: { name: string | null; address: string | null }[]) =>
      list.some((p) => fold(`${p.name ?? ""} ${p.address ?? ""}`).includes(value));
    switch (field?.[1].toLowerCase()) {
      case "from":
        test = (m) => people([m.from]);
        break;
      case "to":
        test = (m) => people([...m.to, ...m.cc]);
        break;
      case "subject":
        test = (m) => fold(m.subject).includes(value);
        break;
      case "has":
        test = (m) => m.attachments.length > 0;
        break;
      case "is":
        test = (m) => (value === "unread" ? !m.seen : value === "read" ? m.seen : value === "flagged" ? m.flagged : true);
        break;
      case "in":
        folder = value;
        continue;
      case "before":
        test = (m) => m.received_at < day(value);
        break;
      case "after":
        test = (m) => m.received_at >= day(value);
        break;
      default:
        if (!negate) words.push(value);
        test = (m) =>
          fold(
            `${m.subject} ${m.from.name} ${m.from.address} ${m.to.map((p) => p.address).join(" ")} ${m.text} ${server ? m.attachments.map((a) => a.name).join(" ") : ""}`,
          ).includes(value);
    }
    tests.push(negate ? (m) => !test(m) : test);
  }
  const wanted = accounts.filter((a) => !accountId || a.account.id === accountId);
  const boxes = new Map(wanted.flatMap((a) => a.mailboxes.map((m) => [m.id, m] as const)));
  const found = messages
    .filter((m) => {
      const box = boxes.get(m.mailboxId);
      if (!box) return false;
      if (folder === null || folder === "anywhere") {
        if (folder === null && (box.role === "trash" || box.role === "junk")) return false;
      } else if (box.role !== folder && fold(box.name) !== folder) return false;
      return tests.every((test) => test(m));
    })
    .sort((a, b) => b.received_at - a.received_at || b.id - a.id);
  // The best: those with the first word in the subject, as the daemon weighs it most.
  const top = words.length > 0 && found.length <= 2000 ? found.filter((m) => fold(m.subject).includes(words[0])).slice(0, 3) : [];
  return { found, top };
}

/** Sample agent sessions: the command line two hours ago, an assistant just now (one request refused), and a script during it. */
/** What an assistant asked for and waits for the person: a send (its draft is made here) and a bulk archive. */
const holdDecisions = new Map<number, "approved" | "rejected">();
let heldDraft: number | null = null;
function sampleHolds(): Hold[] {
  const owner = accounts[0];
  if (!owner) return [];
  if (heldDraft === null) {
    heldDraft = newDraftFor(owner.account.id, {
      to: [{ name: "Lena Okafor", address: "lena.okafor@example.net" }],
      subject: "Re: Garden plot schedule",
      body: "Hi Lena,\n\nSaturdays work for me too. I'll put the rota on the shed door on Friday.\n\nSam",
    }).id;
  }
  const hold = (id: number, minutesAgo: number, reason: Hold["reason"], method: string, description: string, draft: number | null): Hold => ({
    id,
    created_at: started - minutesAgo * 60_000,
    expires_at: started - minutesAgo * 60_000 + 24 * 3_600_000,
    session: "mcp-1",
    client: "mcp",
    account_id: owner.account.id,
    method,
    params: draft !== null ? { id: draft } : { ids: Array.from({ length: 120 }, (_, index) => index + 1) },
    description,
    reason,
    draft_id: draft,
    status: holdDecisions.get(id) ?? "pending",
    decided_at: null,
    result: null,
    error: null,
  });
  const email = owner.account.email;
  return [
    hold(2, 1, "sending", "draft.send", "Send “Re: Garden plot schedule” to Lena Okafor", heldDraft),
    {
      ...hold(3, 4, "voice", "voice.suggest", `Change your voice for ${email}`, null),
      params: {
        address: email,
        text: "Writes in English, or in French when the other person does.\nFriendly and short: the answer first, then one clear next step.\nOpens with “Hi,” and signs “Sam”.\nNever promises a date or a price: leaves [TO CONFIRM].",
      },
    },
    {
      ...hold(
        4,
        5,
        "note",
        "note.add",
        "Save a note about anna@client.example. It has bank details in it, and comes from a message that anna@client.example did not send (billing@client-payments.example).",
        null,
      ),
      params: {
        account_id: owner.account.id,
        address: "anna@client.example",
        text: "Invoices now go to IBAN CH93 0076 2011 6238 5295 7, not the old account.",
      },
    },
    hold(1, 6, "bulk", "message.archive", "Archive 120 messages", null),
  ];
}

/** The sample archive and flag change an agent made: each undone from Settings › Agents once. */
const AGENT_UNDOS = new Set([9001, 9002, 9003]);
const agentUndone = new Set<number>();

function agentActivity(): AgentActivity[] {
  const account = accounts[0]?.account.id ?? null;
  const inbox = accounts[0]?.mailboxes.find((m) => m.role === "inbox")?.id ?? 1;
  const was = (id: number, subject: string, seen: boolean) => ({
    id,
    mailbox_id: inbox,
    subject,
    seen,
    flagged: false,
    pinned: false,
    aside: false,
    snoozed_until: null,
    remind_at: null,
  });
  const at = (minutesAgo: number) => started - minutesAgo * 60_000;
  const entry = (
    id: number,
    minutesAgo: number,
    session: string,
    client: string,
    method: string,
    args: unknown,
    denied = false,
    before: unknown = null,
    undo: number | null = null,
  ): AgentActivity => ({
    id,
    at: at(minutesAgo),
    session,
    client,
    method,
    args,
    account_id: method === "account.list" ? null : account,
    access: method === "account.list" ? null : before !== null ? "triage" : "read",
    decision: denied ? "denied" : "allowed",
    error_code: denied ? -32010 : null,
    error: denied ? "Agents may only read sam@example.org. Allow more in Settings › Agents." : null,
    duration_ms: 3 + (id % 7),
    before,
    undo_id: undo,
    undoable: undo !== null && !agentUndone.has(undo),
  });
  // A script on the command line ran while the assistant was connected: their requests interleave.
  return [
    entry(16, 0.2, "mcp-1", "mcp", "message.set_flags", { ids: [104], seen: true }, false, [was(104, "Book club: next pick", false)], 9002),
    entry(
      15,
      0.3,
      "mcp-1",
      "mcp",
      "message.archive",
      { ids: [106, 107] },
      false,
      [was(106, "Your parcel is out for delivery", true), was(107, "Your October statement is ready", true)],
      9001,
    ),
    entry(14, 0.5, "mcp-1", "mcp", "message.get", { id: 105 }),
    entry(13, 1, "cli-2", "cli", "search.query", { query: "is:unread" }),
    entry(12, 2, "mcp-1", "mcp", "draft.create", { account_id: account }, true),
    {
      ...entry(17, 2.5, "mcp-1", "mcp", "message.get", { id: 108 }, true),
      error_code: -32015,
      error: "too many requests: try again in 0.4 s",
    },
    entry(11, 3, "mcp-1", "mcp", "message.get", { id: 101 }),
    entry(10, 3, "mcp-1", "mcp", "search.query", { query: "garden rota" }),
    entry(9, 4, "mcp-1", "mcp", "account.list", {}),
    entry(8, 118, "cli-1", "cli", "message.get", { id: 104 }),
    entry(7, 118, "cli-1", "cli", "message.get", { id: 103 }),
    entry(6, 119, "cli-1", "cli", "message.get", { id: 102 }),
    entry(5, 119, "cli-1", "cli", "search.query", { query: "invoice has:attachment" }),
    entry(4, 120, "cli-1", "cli", "message.list", { mailbox_id: 1, limit: 20 }),
    entry(3, 120, "cli-1", "cli", "mailbox.list", { account_id: account }),
    entry(2, 121, "cli-1", "cli", "account.list", {}),
  ];
}

/**
 * "Did you mean": each word that finds nothing mended to the closest word of
 * the samples (two edits at most), filters kept; offered only if it finds something.
 */
function suggest(query: string, accountId: string | null): string | null {
  const words = new Set(
    messages.flatMap((m) => fold(`${m.subject} ${m.from.name} ${m.text}`).split(/[^a-z0-9]+/).filter((w) => w.length > 2)),
  );
  const distance = (a: string, b: string) => {
    const row = Array.from({ length: b.length + 1 }, (_, j) => j);
    for (let i = 1; i <= a.length; i++) {
      let diagonal = row[0];
      row[0] = i;
      for (let j = 1; j <= b.length; j++) {
        const above = row[j];
        row[j] = Math.min(row[j] + 1, row[j - 1] + 1, diagonal + (a[i - 1] === b[j - 1] ? 0 : 1));
        diagonal = above;
      }
    }
    return row[b.length];
  };
  let changed = false;
  const mended = query
    .split(/\s+/)
    .map((term) => {
      if (!term || term.includes(":") || term.startsWith("-") || term.startsWith('"')) return term;
      const word = fold(term);
      if (words.has(word)) return term;
      let best: string | null = null;
      let bestDistance = 3;
      for (const candidate of words) {
        const d = distance(word, candidate);
        if (d < bestDistance) [best, bestDistance] = [candidate, d];
      }
      if (best === null) return term;
      changed = true;
      return best;
    })
    .join(" ");
  return changed && searchSamples(mended, accountId).found.length > 0 ? mended : null;
}

/** What a section's list shows: without screening, new senders are with People. */
const shownIn = (asked: Section, section: Section) =>
  section === asked || (asked === "people" && section === "new_senders" && screeningMode === "off");

/** Like the daemon's runner: it goes once undo is over and its time has come, into Sent. */
function sendWhenDue(entry: OutboxEntry) {
  const due = Math.max(entry.undo_until, entry.send_at ?? 0);
  const meant = entry.send_at;
  const delay = due - Date.now() + 1500;
  if (delay > 2 ** 31 - 1) return; // Further than a timer reaches; the preview never gets there.
  window.setTimeout(() => {
    // Taken back, held, or moved to another time meanwhile.
    if (!outbox.includes(entry) || entry.state !== "queued" || entry.send_at !== meant) return;
    outbox.splice(outbox.indexOf(entry), 1);
    const draft = drafts.find((d) => d.id === entry.draft_id);
    if (draft) drafts.splice(drafts.indexOf(draft), 1);
    const sent = accounts.find((a) => a.account.id === entry.account_id)?.mailboxes.find((m) => m.role === "sent");
    if (sent && draft) {
      messages.push({
        id: nextId++,
        mailboxId: sent.id,
        subject: draft.subject,
        from: draft.from,
        to: draft.to,
        cc: draft.cc,
        received_at: Date.now(),
        seen: true,
        flagged: false,
        answered: false,
        text: draft.body || (draft.html ?? "").replace(/<[^>]+>/g, " "),
        html: null,
        remote_images: 0,
        attachments: [],
        thread: `sent-${draft.id}`,
      });
    }
    changed();
  }, delay);
}

// `?reminder=1`: a message Sam sent, whose reminder came due with no reply.
if (params.get("reminder") === "1") {
  const sent = accounts[0]?.mailboxes.find((m) => m.role === "sent");
  const mine = messages.find((m) => m.mailboxId === sent?.id);
  if (mine) Object.assign(mine, { reminded: true, surfaced_at: started - 60_000 });
}

// `?outbox=1`: one message scheduled for tomorrow morning, and one held after its time passed.
if (params.get("outbox") === "1" && accounts.length > 0) {
  const tomorrow = new Date();
  tomorrow.setDate(tomorrow.getDate() + 1);
  tomorrow.setHours(9, 0, 0, 0);
  const waiting = (subject: string, sendAt: number, state: "queued" | "held"): OutboxEntry => ({
    id: nextId++,
    account_id: accounts[0].account.id,
    draft_id: null,
    subject,
    recipients: [{ name: "Lena Okafor", address: "lena.okafor@example.net" }],
    state,
    undo_until: started,
    send_at: sendAt,
    next_attempt_at: sendAt,
    attempts: 0,
    last_error: null,
    executor: "device",
    needs_this_computer: true,
  });
  outbox.push(waiting("Seed order for spring", tomorrow.getTime(), "queued"));
  outbox.push(waiting("Monday's rota", started - 3 * 3600_000, "held"));
}

// Each account sends as its own address; the first also as a work alias.
const identities: Identity[] = accounts.flatMap((owner, index) => [
  {
    id: nextId++,
    account_id: owner.account.id,
    name: owner.account.display_name,
    email: owner.account.email,
    signature_id: null,
    signature: "",
    voice: index === 0 ? "Warm but direct; short sentences.\nVous with clients, tu with people who already use tu." : "",
    voice_on: index === 0,
    is_default: true,
  },
  ...(index === 0
    ? [
        {
          id: nextId++,
          account_id: owner.account.id,
          name: `${owner.account.display_name} (work)`,
          email: "sam@studio.example",
          signature_id: 2,
          signature: "Sam Rivera · Studio Kiln",
          voice: "",
          voice_on: false,
          is_default: false,
        },
      ]
    : []),
]);
// Signatures (ADR-0021): the work alias uses "Studio".
const signatures: Signature[] = [
  { id: 1, name: "Short", body: "Sam", updated_at: Date.now() },
  { id: 2, name: "Studio", body: "Sam Rivera · Studio Kiln", updated_at: Date.now() },
];

// Addresses forgotten from suggestions (people.forget), lowercase.
const forgottenPeople = new Set<string>();

// Notes about people (ADR-0020): two about the first account's most frequent sender.
let notesOn = true;
const notes: Note[] = accounts.length
  ? [
      { text: "Decides on renewals.", by: "person" as const, ago: 40 },
      { text: "Prefers calls before 10:00.", by: "agent" as const, ago: 7, from_subject: "Contract renewal" },
    ].map(({ ago, ...note }) => ({
      ...note,
      id: nextId++,
      // The account she writes to (the work one, when there are two).
      account_id: (accounts.find((a) => a.mailboxes.some((m) => m.id === 21)) ?? accounts[0]).account.id,
      address: "ines.moreau@example.com",
      created_at: Date.now() - ago * 86_400_000,
      updated_at: Date.now() - ago * 86_400_000,
    }))
  : [];
const identityFor = (accountId: string, id: number | null | undefined) =>
  identities.find((i) => i.account_id === accountId && (id == null ? i.is_default : i.id === id));

if (accounts.length > 0) {
  drafts.push({
    id: nextId++,
    account_id: PERSONAL.account.id,
    from: ME,
    identity_id: identityFor(PERSONAL.account.id, null)?.id ?? null,
    to: [{ name: "Hannah Weiss", address: "hannah.weiss@example.org" }],
    cc: [],
    bcc: [],
    subject: "Re: Book club: next pick",
    body: "I'd vote for the lighthouse keeper.",
    html: "<p>I'd vote for the lighthouse keeper.</p>",
    replying_to: null,
    updated_at: started - 70 * 60_000,
    sending: false,
    attachments: [],
  });
}

const changed = () => void emit("changes", {});
const mailboxOf = (id: number) => accounts.flatMap((a) => a.mailboxes).find((m) => m.id === id);
const accountOfMailbox = (id: number) => accounts.find((a) => a.mailboxes.some((m) => m.id === id));
const byId = (id: number) => {
  const message = messages.find((m) => m.id === id);
  if (!message) throw new Error("That message is no longer here.");
  return message;
};

function summary(message: FixtureMessage, listedAt = message.received_at): MessageSummary {
  return {
    id: message.id,
    uid: message.id,
    subject: message.subject,
    from: message.from,
    to: [...message.to, ...message.cc].slice(0, 3),
    received_at: message.received_at,
    sent_at: message.received_at,
    seen: message.seen,
    flagged: message.flagged,
    answered: message.answered,
    size: message.text.length,
    mailbox_id: message.mailboxId,
    pinned: message.pinned ?? false,
    aside: message.aside ?? false,
    snoozed_until: message.snoozed_until ?? null,
    remind_at: message.remind_at ?? null,
    reminded: message.reminded ?? false,
    listed_at: listedAt,
    preview: message.text.replace(/\s+/g, " ").trim().slice(0, 140) || null,
    has_attachments: message.attachments.length > 0,
    thread_size: messages.filter((m) => m.thread === message.thread).length,
    section: mailboxOf(message.mailboxId)?.role === "inbox" ? explanationOf(message).section : null,
  };
}

/** Change a mark on messages, with an undo entry, as the daemon does. */
function mark(ids: number[], change: (message: FixtureMessage) => void, description: string) {
  const marks = new Map<number, Marks>();
  for (const id of ids) {
    const message = byId(id);
    const { pinned, aside, snoozed_until, remind_at, reminded, surfaced_at } = message;
    marks.set(id, { pinned, aside, snoozed_until, remind_at, reminded, surfaced_at });
    change(message);
  }
  const id = nextId++;
  undos.set(id, { description, restore: new Map(), marks });
  changed();
  return { changed: ids.length, undo: { id, description } };
}

function moveTo(ids: number[], target: (message: FixtureMessage) => number | undefined, verb: string) {
  const restore = new Map<number, number>();
  for (const id of ids) {
    const message = byId(id);
    const to = target(message);
    if (to === undefined) throw new Error("There is no folder to move it to.");
    restore.set(id, message.mailboxId);
    message.mailboxId = to;
  }
  const id = nextId++;
  const description = `${verb} ${ids.length === 1 ? "1 message" : `${ids.length} messages`}`;
  undos.set(id, { description, restore });
  changed();
  return { moved: ids.length, undo: { id, description } };
}

const roleBox = (message: FixtureMessage, role: string) =>
  accountOfMailbox(message.mailboxId)?.mailboxes.find((m) => m.role === role)?.id;

function newDraftFor(accountId: string, fields: Partial<Draft>): Draft {
  const owner = accounts.find((a) => a.account.id === accountId) ?? accounts[0];
  const identity = identityFor(owner.account.id, null);
  const draft: Draft = {
    id: nextId++,
    account_id: owner.account.id,
    from: { name: identity?.name ?? owner.account.display_name, address: identity?.email ?? owner.account.email },
    identity_id: identity?.id ?? null,
    to: [],
    cc: [],
    bcc: [],
    subject: "",
    body: "",
    html: null,
    replying_to: null,
    updated_at: Date.now(),
    sending: false,
    attachments: [],
    ...fields,
  };
  // The account's automatic copy, unless that address is already a recipient.
  const copy = (owner.account as { auto_copy?: { kind: "cc" | "bcc"; address: string } }).auto_copy;
  const present = [...draft.to, ...draft.cc, ...draft.bcc].some((p) => p.address?.toLowerCase() === copy?.address.toLowerCase());
  if (copy && !present) draft[copy.kind] = [...draft[copy.kind], { name: null, address: copy.address }];
  drafts.unshift(draft);
  changed();
  return draft;
}

const quote = (message: FixtureMessage) =>
  `\n\nOn ${new Date(message.received_at).toLocaleString()}, ${message.from.name} wrote:\n${message.text
    .split("\n")
    .map((line) => `> ${line}`)
    .join("\n")}`;

const draftById = (id: number) => {
  const draft = drafts.find((d) => d.id === id);
  if (!draft) throw new Error("That draft is gone.");
  return draft;
};

let detailedLogs = false;
let connectionLog = false;
let prefetch = false;
let reading = { clean_links: true };
// Two agents connected in the sample: Claude Code's MCP server and a CLI.
let agentsGate = { enabled: true, connected: 2, apps: ["claude-code"] };
/** Whether `regards` is on the PATH (`command_line`). */
let commandOnPath = false;
let assistant: { kind: string | null; base_url: string | null; model: string | null; has_key: boolean } =
  // `?assistant=on`: a provider already chosen.
  params.get("assistant") === "on"
    ? { kind: "anthropic", base_url: "https://api.anthropic.com", model: "claude-opus-5-5", has_key: true }
    : { kind: null, base_url: null, model: null, has_key: false };
let mailApp = params.get("mailapp") === "1";

/** A link as the desktop reads it: `message:5`, `hold:2`, or a mailto: link. */
function parseLink(link: string) {
  if (link.startsWith("mailto:")) {
    const url = new URL(link);
    const list = (value: string | null) =>
      (value ?? "")
        .split(",")
        .map((address) => address.trim())
        .filter(Boolean);
    return {
      kind: "compose",
      to: [...list(decodeURIComponent(url.pathname)), ...list(url.searchParams.get("to"))],
      cc: list(url.searchParams.get("cc")),
      bcc: list(url.searchParams.get("bcc")),
      subject: url.searchParams.get("subject") ?? "",
      body: url.searchParams.get("body") ?? "",
    };
  }
  const [kind, id] = link.split(":");
  return { kind, id: Number(id) };
}
let badgeChoices: { on: boolean; accounts: string[] | null } = { on: true, accounts: null };

let notificationChoices = {
  new_mail: {} as Record<string, string>,
  snooze_ended: true,
  reminder_due: true,
  sending: true,
  agents: true,
};

function handle(cmd: string, args: Args): unknown {
  switch (cmd) {
    case "daemon_status":
      return {
        running: true,
        endpoint: "preview",
        version: "0.1.0 (preview)",
        buildId: "preview",
        pid: 4242,
        uptimeSecs: Math.round((Date.now() - started) / 1000),
        desktopAccess: true,
        started: false,
        replaced: false,
        error: null,
      };
    case "account_list":
      return accounts.map((a) => a.account);
    case "account_add":
      throw new Error("The preview cannot sign in to a real server.");
    case "account_set_login": {
      const input = args.input as {
        id: string;
        username: string | null;
        password: string | null;
        imapHost: string;
        imapPort: number;
        smtpHost: string;
        smtpPort: number;
        smtpSecurity: "tls" | "starttls";
      };
      const owner = accounts.find((a) => a.account.id === input.id);
      if (!owner) throw "No such account.";
      // "wrong" stands for a password the server turns down.
      if (input.password === "wrong") throw "The server did not accept the user name or password.";
      const account = owner.account as unknown as Record<string, unknown>;
      account.username = input.username?.trim() || owner.account.email;
      account.imap = { host: input.imapHost.trim(), port: input.imapPort, security: "tls" };
      account.smtp = { host: input.smtpHost.trim() || input.imapHost.trim(), port: input.smtpPort, security: input.smtpSecurity };
      changed();
      return owner.account;
    }
    case "account_discover": {
      const domain = String(args.email ?? "").split("@")[1]?.toLowerCase() ?? "";
      if (domain === "outlook.com" || domain === "hotmail.com") {
        // Microsoft's consumer mail takes no password: only signing in with Microsoft.
        return { source: "known", imap: null, smtp: null, oauth: "microsoft", oauth_only: true };
      }
      if (domain === "gmail.com") {
        return {
          source: "known",
          imap: { host: "imap.gmail.com", port: 993, security: "tls" },
          smtp: { host: "smtp.gmail.com", port: 587, security: "starttls" },
          oauth: "google",
          oauth_only: false,
          password_help: "Gmail takes an app password, not your Google password.",
        };
      }
      return domain.includes(".")
        ? {
            source: "guess",
            imap: { host: `imap.${domain}`, port: 993, security: "tls" },
            smtp: { host: `smtp.${domain}`, port: 587, security: "starttls" },
            oauth_only: false,
          }
        : { source: null, imap: null, smtp: null, oauth_only: false };
    }
    case "sender_set_images": {
      const sender = args.sender as { account_id: string; address: string; allow: boolean };
      const allowed = imageSenders.get(sender.account_id) ?? new Set<string>();
      if (sender.allow) allowed.add(sender.address.toLowerCase());
      else allowed.delete(sender.address.toLowerCase());
      imageSenders.set(sender.account_id, allowed);
      return { allow: sender.allow };
    }
    case "sender_images":
      return { senders: [...(imageSenders.get(args.accountId as string) ?? [])].sort() };
    case "background_settings":
      return { start_at_login: startAtLogin, close_to_tray: closeToTray };
    case "set_start_at_login":
      startAtLogin = args.on as boolean;
      return null;
    case "set_close_to_tray":
      closeToTray = args.on as boolean;
      return null;
    case "open_system_settings":
      if (params.get("platform") === "macos" && args.pane === "default_apps")
        throw "On a Mac, the default mail app is chosen in Mail: Mail › Settings › General.";
      return null;
    case "open_url":
    case "open_in_agent":
    case "add_to_cursor":
      return null;
    case "account_set_auto_copy": {
      const owner = accounts.find((a) => a.account.id === args.accountId);
      if (!owner) throw "No such account.";
      const wanted = args.autoCopy as { kind: string; address: string } | null;
      if (wanted) {
        wanted.address = wanted.address.trim();
        if (!/^[^\s@<>,;]+@[^\s@<>,;]+\.[^\s@<>,;]+$/.test(wanted.address)) throw "Enter one email address to copy.";
      }
      (owner.account as { auto_copy?: unknown }).auto_copy = args.autoCopy ?? undefined;
      changed();
      return { auto_copy: args.autoCopy ?? null };
    }
    case "account_update": {
      const owner = accounts.find((a) => a.account.id === args.id);
      if (!owner) throw "No such account.";
      owner.account.display_name = String(args.displayName ?? "").trim() || owner.account.email;
      changed();
      return owner.account;
    }
    case "draft_check": {
      const draft = draftById(args.id as number);
      const checks: Array<Record<string, string>> = [];
      if (draft.to.length + draft.cc.length + draft.bcc.length === 0) checks.push({ kind: "no_recipients" });
      if (!draft.subject.trim()) checks.push({ kind: "no_subject" });
      const written = draft.body.split(/\n\n.* wrote:\n|\n\n-{5,} Forwarded/)[0].toLowerCase();
      const word = ["attached", "attachment", "enclosed", "pièce jointe", "anbei"].find((w) => written.includes(w));
      if (word && (draft.attachments ?? []).length === 0) checks.push({ kind: "missing_attachment", word });
      const hole = /\{\{[^}\n]{0,60}\}\}|\[\[[^\]\n]{0,60}\]\]/.exec(written);
      if (hole) checks.push({ kind: "placeholder", text: hole[0] });
      return { checks };
    }
    case "people_suggest": {
      // From the sample mail: those the account wrote to first, then those who wrote to it.
      const typed = String(args.query ?? "").trim().toLowerCase();
      const own = new Set(accounts.map((a) => a.account.email.toLowerCase()));
      const ranked = new Map<string, { person: Person; wrote: number; got: number }>();
      for (const message of messages) {
        const owner = accountOfMailbox(message.mailboxId);
        if (args.accountId && owner?.account.id !== args.accountId) continue;
        const sent = owner?.mailboxes.find((m) => m.id === message.mailboxId)?.role === "sent";
        for (const person of sent ? [...message.to, ...message.cc] : [message.from]) {
          const address = person.address?.toLowerCase();
          if (!address || own.has(address) || /no-?reply|notifications?@|bounce/.test(address)) continue;
          const entry = ranked.get(address) ?? { person: { name: person.name, address: person.address }, wrote: 0, got: 0 };
          if (sent) entry.wrote++;
          else entry.got++;
          ranked.set(address, entry);
        }
      }
      // As the daemon matches: accents and case aside, each typed word starting
      // a word of the name or a part of the address, in any order.
      const fold = (text: string) => text.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
      const words = fold(typed).split(/\s+/).filter(Boolean);
      const people = [...ranked.values()]
        .filter(({ person }) => !forgottenPeople.has((person.address ?? "").toLowerCase()))
        .filter(({ person }) => {
          const parts = fold(`${person.name ?? ""} ${person.address ?? ""}`).split(/[^\p{L}\p{N}]+/u);
          return words.every((word) => parts.some((part) => part.startsWith(word)));
        })
        .sort((a, b) => b.wrote - a.wrote || b.got - a.got)
        .slice(0, Math.min(Number(args.limit ?? 8), 50))
        .map(({ person }) => person);
      return { people };
    }
    // A message's own window: a tab here, with the composer alone.
    case "composer_window":
      window.open(`preview.html?compose=${Number(args.draftId)}`, "_blank");
      return null;
    case "composer_window_open":
      return false;
    case "people_forget": {
      const address = String(args.address ?? "").trim().toLowerCase();
      const known = !forgottenPeople.has(address);
      forgottenPeople.add(address);
      return { forgotten: known ? 1 : 0 };
    }
    case "reading_settings":
      return reading;
    case "set_reading_settings":
      reading = args.settings as typeof reading;
      return null;
    case "assistant_settings":
      return assistant;
    case "set_assistant_settings": {
      const choice = args.settings as { kind: string | null; base_url?: string; model?: string; api_key?: string };
      if (!choice.kind) {
        assistant = { kind: null, base_url: null, model: null, has_key: false };
        return assistant;
      }
      const anthropic = choice.kind === "anthropic";
      const base_url = choice.base_url?.trim().replace(/\/+$/, "") || (anthropic ? "https://api.anthropic.com" : "");
      const model = choice.model?.trim() || (anthropic ? "claude-opus-5-5" : "");
      if (!base_url) throw new Error("an OpenAI-compatible server needs its address");
      if (!model) throw new Error("which model should answer?");
      const sameServer = assistant.kind === choice.kind && assistant.base_url === base_url;
      const has_key = Boolean(choice.api_key?.trim()) || (sameServer && assistant.has_key);
      if (anthropic && !has_key) throw new Error("an Anthropic API key is needed (from the Anthropic Console)");
      assistant = { kind: choice.kind, base_url, model, has_key };
      return assistant;
    }
    case "assistant_preview":
    case "assistant_run": {
      if (!assistant.kind) throw new Error("the assistant is off; choose a provider in Settings to use it");
      const message = byId(args.messageId as number);
      const thread = messages.filter((m) => m.thread === message.thread).slice(-8);
      const task = args.task as { kind: "summary" } | { kind: "reply"; instructions: string };
      if (cmd === "assistant_preview") {
        return {
          messages: thread.length,
          characters: thread.reduce((sum, m) => sum + Math.min(m.text.length, 8000), 0),
          left_out: 0,
          destination: (assistant.base_url ?? "").replace(/^https?:\/\//, "").split("/")[0],
          model: assistant.model,
        };
      }
      // A model takes a moment.
      const later = (result: unknown) => new Promise((resolve) => window.setTimeout(() => resolve(result), 900));
      const from = message.from.name ?? message.from.address ?? "They";
      if (task.kind === "summary") {
        return later({
          text: `${from} writes about “${message.subject}”. (A sample summary: the preview sends nothing to a model.)`,
          draft: null,
        });
      }
      const text = task.instructions.trim()
        ? `Thanks, ${from.split(" ")[0]}. ${task.instructions.trim()}.`
        : `Thanks, ${from.split(" ")[0]}. That works for me.`;
      const owner = accountOfMailbox(message.mailboxId);
      const draft = newDraftFor(owner?.account.id ?? "", {
        to: [message.from],
        subject: message.subject.startsWith("Re:") ? message.subject : `Re: ${message.subject}`,
        body: `${text}${quote(message)}`,
        replying_to: message.id,
      });
      return later({ text, draft });
    }
    case "prefetch_get":
      return prefetch;
    case "prefetch_set":
      prefetch = args.on as boolean;
      return null;
    case "mail_app_status":
      // `?platform=macos`: Mail's settings choose the mail app; Regards cannot.
      return { registered: mailApp, can_register: params.get("platform") !== "macos" };
    case "make_mail_app":
      if (params.get("platform") === "macos")
        throw "On macOS, choose Regards as the default mail app in Mail, under Settings › General.";
      mailApp = true;
      return null;
    case "badge_settings":
      return badgeChoices;
    case "set_badge_settings":
      badgeChoices = args.settings as typeof badgeChoices;
      return null;
    case "support_settings":
      return { detailed_logs: detailedLogs, connection_log: connectionLog };
    case "set_connection_log":
      connectionLog = args.on as boolean;
      return null;
    case "set_detailed_logs":
      detailedLogs = args.on as boolean;
      return null;
    case "export_diagnostics":
      return "C:\\Users\\you\\Documents\\regards-diagnostics.txt";
    case "notification_settings":
      return notificationChoices;
    case "set_notification_settings":
      notificationChoices = args.settings as typeof notificationChoices;
      return null;
    case "update_status":
      return { checks: updateChecks, current: "0.1.0", available: updateFound, checked_at: updateCheckedAt };
    case "update_set_checks":
      updateChecks = Boolean(args.on);
      return null;
    case "update_check":
      // `?updates=none`: already the latest.
      updateCheckedAt = Date.now();
      updateFound =
        params.get("updates") === "none"
          ? null
          : { version: "0.2.0", url: "https://github.com/lucienPochon/regards/releases/tag/v0.2.0", published_at: null };
      return updateFound;
    case "update_open":
      return null;
    case "account_oauth_providers":
      return { providers: ["microsoft", "google"] };
    case "account_oauth_start":
      oauthEmail = (args.email as string | null) ?? null;
      return { flow_id: "preview-flow", url: "https://login.example.test/authorize" };
    case "account_oauth_finish":
      // `?oauth=ok`: signing in again works, for an account the preview has.
      if (params.get("oauth") === "ok") {
        const owner = accounts.find((a) => a.account.email === oauthEmail);
        if (owner) {
          return new Promise((resolve) =>
            window.setTimeout(() => {
              owner.account = { ...owner.account, needs_sign_in: false };
              changed();
              resolve(owner.account);
            }, 1500),
          );
        }
      }
      // Waits like a person in the browser, then says what the preview cannot do.
      return new Promise((_, reject) =>
        window.setTimeout(() => reject(new Error("The preview cannot sign in to a real provider.")), 1500),
      );
    case "account_oauth_cancel":
      return null;
    case "account_remove": {
      const index = accounts.findIndex((a) => a.account.id === args.id);
      if (index !== -1) accounts.splice(index, 1);
      changed();
      return null;
    }
    case "account_set_agent_access": {
      const owner = accounts.find((a) => a.account.id === args.accountId);
      if (!owner) throw new Error("That account is gone.");
      owner.account = { ...owner.account, agent_access: args.access as AgentAccess };
      changed();
      return { agent_access: owner.account.agent_access };
    }
    case "agents_settings":
      return agentsGate;
    case "agent_apps_found":
      // As on a computer with Claude Code (set up) and Cursor (not yet).
      return [
        { id: "claude-code", set_up: true },
        { id: "cursor", set_up: false },
      ];
    case "command_line": {
      const changed = args.change === "add" ? !commandOnPath : args.change === "remove" ? commandOnPath : false;
      if (args.change) commandOnPath = args.change === "add";
      return { on_path: commandOnPath, folder: "C:\\Users\\you\\AppData\\Local\\Regards", can_add: true, changed };
    }
    case "agents_set_enabled":
      agentsGate = { enabled: args.enabled as boolean, connected: args.enabled ? agentsGate.connected : 0, apps: args.enabled ? agentsGate.apps : [] };
      return agentsGate;
    case "agents_disconnect": {
      const sent = agentsGate.connected;
      agentsGate = { ...agentsGate, connected: 0, apps: [] };
      return sent;
    }
    case "agent_activity": {
      const before = (args.before as number | null) ?? null;
      const limit = (args.limit as number | null) ?? 100;
      const all = agentActivity().filter((entry) => !args.accountId || entry.account_id === args.accountId);
      const after = all.filter((entry) => before === null || entry.id < before);
      const page = after.slice(0, limit);
      return { entries: page, next: after.length > limit ? page[page.length - 1].id : null };
    }
    case "link_take": {
      // `?link=message:3`, `draft:…`, `hold:2` or `mailto:…`: what Regards was started with.
      const link = params.get("link");
      return link ? parseLink(link) : null;
    }
    case "draft_attach_image": {
      // An image in the text: kept with the draft under a Content-ID, its bytes as base64.
      const draft = draftById(args.draftId as number);
      const contentId = `img${nextId++}@preview`;
      const data = String(args.data ?? "");
      draftImages.set(contentId, { mime: String(args.mime), data });
      draft.attachments = [
        ...draft.attachments,
        { id: nextId++, name: String(args.name), mime: String(args.mime), size: Math.floor((data.length * 3) / 4), content_id: contentId },
      ];
      changed();
      return { content_id: contentId, draft };
    }
    case "draft_images": {
      const draft = draftById(args.draftId as number);
      return {
        images: draft.attachments.flatMap((file) => {
          const image = file.content_id ? draftImages.get(file.content_id) : undefined;
          return image && file.content_id ? [{ content_id: file.content_id, ...image }] : [];
        }),
      };
    }
    case "draft_attach_dropped": {
      // As the desktop does with the paths it keeps: here, by the sample drop's name.
      const draft = draftById(args.draftId as number);
      draft.attachments = [...draft.attachments, { id: nextId++, name: "Garden-rota.pdf", mime: "application/pdf", size: 48_000 }];
      changed();
      return { draft, refused: [] };
    }
    case "hold_list":
      return { holds: sampleHolds().filter((hold) => !args.pending || hold.status === "pending") };
    case "hold_approve":
    case "hold_reject": {
      const hold = sampleHolds().find((one) => one.id === args.id);
      if (!hold || hold.status !== "pending") throw new Error("It is not waiting any more.");
      const approved = cmd === "hold_approve";
      holdDecisions.set(hold.id, approved ? "approved" : "rejected");
      changed();
      return {
        ...hold,
        status: approved ? "approved" : "rejected",
        decided_at: Date.now(),
        result: approved
          ? hold.reason === "sending"
            ? // `?scheduled=1`: the agent asked to send it in an hour.
              { outbox_id: 777, undo_until: Date.now() + 10_000, send_at: params.get("scheduled") ? Date.now() + 3_600_000 : null }
            : { moved: 120, undo: { id: 9003, description: "Archive 120 messages" } }
          : null,
      };
    }
    case "account_set_agent_send": {
      const owner = accounts.find((a) => a.account.id === args.accountId);
      if (!owner) throw new Error("That account is gone.");
      const seconds = (args.windowSeconds as number | null) ?? null;
      owner.account = { ...owner.account, agent_send_window: seconds ?? undefined };
      changed();
      return { agent_send_window: seconds };
    }
    case "account_resync": {
      // The sample mail stays: the preview has no server to fetch it again from.
      const owner = accounts.find((a) => a.account.id === args.accountId);
      const ids = new Set(owner?.mailboxes.map((m) => m.id));
      const dropped = messages.filter((m) => ids.has(m.mailboxId)).length;
      return new Promise((resolve) => window.setTimeout(() => resolve(dropped), 1200));
    }
    case "erase_everything":
      // The real app quits here, and starts as new next time; the preview keeps its sample.
      return new Promise((resolve) => window.setTimeout(() => resolve(null), 800));
    case "account_sync":
      return new Promise((resolve) => window.setTimeout(() => resolve(null), 900));
    case "mailbox_list": {
      const owner = accounts.find((a) => a.account.id === args.accountId);
      // Unread per folder; the inbox leaves out what is set aside or snoozed.
      const unread = (id: number, role: string | null) =>
        messages.filter((m) => m.mailboxId === id && !m.seen && (role !== "inbox" || !parked(m))).length;
      return {
        mailboxes: (owner?.mailboxes ?? []).map((m) => ({ ...m, unread: unread(m.id, m.role) })),
        sync: { running: false, last_success_at: Date.now(), last_error: null, push_active: true },
      };
    }
    case "message_list": {
      const before = args.before as { received_at: number; id: number } | null;
      const limit = (args.limit as number | null) ?? 50;
      const inbox = mailboxOf(args.mailboxId as number)?.role === "inbox";
      const account = accountOfMailbox(args.mailboxId as number);
      // The inbox also shows reminders that came due, from wherever they are filed.
      const place = (m: FixtureMessage) => (inbox ? inboxPlace(m) : m.received_at);
      const section = inbox ? ((args.section as Section | null) ?? null) : null;
      const seen = inbox ? ((args.seen as boolean | null | undefined) ?? null) : null;
      // Show Done: the account's Archive among the inbox, by date.
      const archive = inbox && args.withDone && section === null && seen === null ? account?.mailboxes.find((b) => b.role === "archive")?.id : undefined;
      const all = messages
        .filter((m) =>
          inbox
            ? (archive !== undefined && m.mailboxId === archive) ||
              (!parked(m) && (m.mailboxId === args.mailboxId || (m.reminded === true && account?.mailboxes.some((b) => b.id === m.mailboxId))))
            : m.mailboxId === args.mailboxId,
        )
        .filter((m) => {
          if (!inbox) return true;
          const placed = explanationOf(m).section;
          // Held new senders stay out of the inbox until accepted.
          if (section === null) return !(screeningMode === "hold" && placed === "new_senders");
          return shownIn(section, placed);
        })
        .filter((m) => seen === null || m.seen === seen)
        .sort((a, b) => place(b) - place(a) || b.id - a.id)
        .filter((m) => !before || place(m) < before.received_at || (place(m) === before.received_at && m.id < before.id));
      const page = all.slice(0, limit);
      const last = page[page.length - 1];
      return {
        messages: page.map((m) => summary(m, place(m))),
        next: all.length > limit && last ? { received_at: place(last), id: last.id } : null,
      };
    }
    case "attachment_preview": {
      const file = byId(args.messageId as number).attachments[args.index as number];
      if (!file?.preview) throw new Error("this file can't be shown here: open it or save it");
      const name = file.name ?? "attachment";
      if (file.preview === "text") {
        const text = name.endsWith(".csv")
          ? "name,flat,present\nOla,2B,yes\nDaniel,4A,yes\nPriya,1C,no\nMarco,3A,yes\n"
          : `The preview has no real files: this stands in for ${name}.\n`;
        return { kind: "text", text, truncated: false };
      }
      // A stand-in picture, as an image would come: base64 of its bytes.
      const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="800" viewBox="0 0 1200 800"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#9fc7b8"/><stop offset="1" stop-color="#1d7a61"/></linearGradient></defs><rect width="1200" height="800" fill="url(#g)"/><text x="600" y="420" font-family="sans-serif" font-size="48" fill="#fff" text-anchor="middle">${name.replace(/[<&>]/g, "")}</text></svg>`;
      const bytes = new TextEncoder().encode(svg);
      return { kind: "image", mime: "image/svg+xml", data: btoa(String.fromCharCode(...bytes)) };
    }
    case "attachment_save":
      return args.name as string;
    case "attachment_save_all":
      return byId(args.messageId as number)
        .attachments.filter((file) => !file.inline)
        .map((file) => file.name ?? "attachment");
    case "attachment_open": {
      const file = byId(args.messageId as number).attachments[args.index as number];
      if (file?.risk === "save_only") {
        throw new Error("this file could run a program, so Regards does not open it: save it, and open it only if you trust it");
      }
      return null;
    }
    case "message_source": {
      const message = byId(args.id as number);
      const show = (p: Person) => (p.name ? `${p.name} <${p.address}>` : (p.address ?? ""));
      const text = [
        `From: ${show(message.from)}`,
        `To: ${message.to.map(show).join(", ")}`,
        `Subject: ${message.subject}`,
        `Date: ${new Date(message.received_at).toUTCString()}`,
        `Message-ID: <preview-${message.id}@regards.invalid>`,
        "MIME-Version: 1.0",
        "Content-Type: text/plain; charset=utf-8",
        "",
        message.text,
      ].join("\r\n");
      return { text, truncated: false, size: text.length };
    }
    case "message_get": {
      const message = byId(args.id as number);
      return {
        message: summary(message),
        mailbox_id: message.mailboxId,
        to: message.to,
        cc: message.cc,
        text: message.text,
        has_html: message.html !== null,
        remote_images: message.remote_images,
        images_from_sender: imagesFrom(accountOfMailbox(message.mailboxId)?.account.id ?? "", message.from?.address),
        attachments: message.attachments,
      };
    }
    case "message_conversation_ids": {
      const all: number[] = [];
      for (const id of args.ids as number[]) {
        const message = byId(id);
        const conversation = messages
          .filter((m) => m.id === id || (m.thread === message.thread && m.mailboxId === message.mailboxId))
          .sort((a, b) => b.received_at - a.received_at || b.id - a.id)
          .map((m) => m.id);
        for (const found of [id, ...conversation]) if (!all.includes(found)) all.push(found);
      }
      return all;
    }
    case "thread_get": {
      const message = byId(args.id as number);
      return {
        messages: messages
          .filter((m) => m.thread === message.thread)
          .sort((a, b) => a.received_at - b.received_at)
          .map((m) => ({
            message: summary(m),
            mailbox_id: m.mailboxId,
            mailbox_name: mailboxOf(m.mailboxId)?.name ?? "",
            mailbox_role: mailboxOf(m.mailboxId)?.role ?? null,
          })),
      };
    }
    case "message_mark_all_read": {
      const section = (args.section as Section | null) ?? null;
      const unread = messages.filter(
        (m) =>
          m.mailboxId === (args.mailboxId as number) &&
          !m.seen &&
          (section === null || shownIn(section, explanationOf(m).section)),
      );
      for (const message of unread) message.seen = true;
      changed();
      if (unread.length === 0) return { changed: 0, undo: null };
      const id = nextId++;
      const description = `Marked ${unread.length === 1 ? "1 message" : `${unread.length} messages`} read`;
      undos.set(id, {
        description,
        restore: new Map(),
        also: () => unread.forEach((message) => (message.seen = false)),
      });
      return { changed: unread.length, undo: { id, description } };
    }
    case "message_set_flags": {
      const ids = args.ids as number[];
      for (const id of ids) {
        const message = byId(id);
        if (typeof args.seen === "boolean") message.seen = args.seen;
        if (typeof args.flagged === "boolean") message.flagged = args.flagged;
      }
      changed();
      // The daemon records an undo for it too; the app shows none for its own flag changes.
      return { changed: ids.length, undo: null };
    }
    case "message_archive": {
      // Done with a reminder shown from another folder: it leaves the inbox and stays filed.
      const ids = args.ids as number[];
      const reminders = ids.filter((id) => byId(id).reminded && mailboxOf(byId(id).mailboxId)?.role !== "inbox");
      for (const id of reminders) Object.assign(byId(id), { reminded: false, surfaced_at: null });
      const rest = ids.filter((id) => !reminders.includes(id));
      if (rest.length === 0) {
        changed();
        return { moved: reminders.length, undo: null };
      }
      return moveTo(rest, (m) => roleBox(m, "archive"), "Archived");
    }
    case "message_trash":
      return moveTo(args.ids as number[], (m) => roleBox(m, "trash"), "Deleted");
    case "message_move":
      return moveTo(args.ids as number[], () => args.mailboxId as number, "Moved");
    case "message_delete": {
      const ids = new Set(args.ids as number[]);
      messages = messages.filter((m) => !ids.has(m.id));
      changed();
      return { deleted: ids.size };
    }
    case "message_pin":
      return mark(args.ids as number[], (m) => (m.pinned = args.pinned as boolean), args.pinned ? "Pinned" : "Unpinned");
    case "message_set_aside":
      return mark(args.ids as number[], (m) => (m.aside = args.aside as boolean), args.aside ? "Set aside" : "Back in the inbox");
    case "message_snooze": {
      const until = args.someday ? SOMEDAY : ((args.until as number | null) ?? null);
      return mark(args.ids as number[], (m) => (m.snoozed_until = until), until ? "Snoozed" : "Back in the inbox");
    }
    case "message_remind": {
      const at = (args.at as number | null) ?? null;
      return mark(
        args.ids as number[],
        (m) => Object.assign(m, { remind_at: at }, m.reminded ? { reminded: false, surfaced_at: null } : {}),
        at ? "Reminder set" : "Reminder off",
      );
    }
    case "message_view": {
      const owner = accounts.find((a) => a.account.id === args.accountId);
      const shown = messages
        .filter((m) => owner?.mailboxes.some((b) => b.id === m.mailboxId))
        .filter((m) =>
          args.view === "pinned"
            ? m.pinned
            : args.view === "aside"
              ? m.aside
              : args.view === "reminders"
                ? m.remind_at != null
                : (m.snoozed_until ?? 0) > Date.now(),
        )
        .sort((a, b) => b.received_at - a.received_at || b.id - a.id);
      return { messages: shown.map((m) => summary(m)), next: null };
    }
    case "undo_apply": {
      if (AGENT_UNDOS.has(args.id as number)) {
        if (agentUndone.has(args.id as number)) throw new Error("It is too late to undo that.");
        agentUndone.add(args.id as number);
        changed();
        return { undone: args.id === 9001 ? 2 : 1 };
      }
      const undo = undos.get(args.id as number);
      if (!undo) throw new Error("It is too late to undo that.");
      undos.delete(args.id as number);
      for (const [id, before] of undo.marks ?? []) {
        const message = messages.find((m) => m.id === id);
        if (message) Object.assign(message, before);
      }
      for (const [id, mailboxId] of undo.restore) {
        const message = messages.find((m) => m.id === id);
        if (message) message.mailboxId = mailboxId;
      }
      undo.also?.();
      changed();
      return { undone: undo.restore.size + (undo.marks?.size ?? 0) };
    }

    case "draft_create":
      return newDraftFor(args.accountId as string, {});
    case "draft_reply": {
      const message = byId(args.messageId as number);
      const owner = accountOfMailbox(message.mailboxId);
      const others: Person[] = args.all ? [...message.to, ...message.cc].filter((p) => p.address !== owner?.account.email) : [];
      return newDraftFor(owner?.account.id ?? "", {
        to: [message.from, ...others.slice(0, 3)],
        subject: message.subject.startsWith("Re:") ? message.subject : `Re: ${message.subject}`,
        body: quote(message),
        replying_to: message.id,
      });
    }
    case "draft_forward": {
      const message = byId(args.messageId as number);
      return newDraftFor(accountOfMailbox(message.mailboxId)?.account.id ?? "", {
        subject: `Fwd: ${message.subject}`,
        body: `\n\n---------- Forwarded message ----------\nFrom: ${message.from.name} <${message.from.address}>\nSubject: ${message.subject}\n\n${message.text}`,
      });
    }
    case "draft_list":
      return { drafts: [...drafts].sort((a, b) => b.updated_at - a.updated_at) };
    case "draft_get":
      return draftById(args.id as number);
    case "draft_update": {
      const update = args.draft as Partial<Draft> & { id: number };
      const draft = draftById(update.id);
      Object.assign(draft, update, { updated_at: Date.now() });
      changed();
      return draft;
    }
    case "draft_delete": {
      const index = drafts.findIndex((d) => d.id === args.id);
      if (index !== -1) drafts.splice(index, 1);
      changed();
      return null;
    }
    case "draft_attach_files":
      return null;
    case "draft_detach": {
      const draft = draftById(args.draftId as number);
      draft.attachments = draft.attachments.filter((a) => a.id !== args.attachmentId);
      return draft;
    }
    case "draft_send": {
      const draft = draftById(args.id as number);
      draft.sending = true;
      const sendAt = (args.sendAt as number | null | undefined) ?? null;
      const entry: OutboxEntry = {
        id: nextId++,
        account_id: draft.account_id,
        draft_id: draft.id,
        subject: draft.subject,
        recipients: [...draft.to, ...draft.cc, ...draft.bcc],
        state: "queued",
        undo_until: Date.now() + ((args.undoSeconds as number) ?? 10) * 1000,
        send_at: sendAt,
        next_attempt_at: sendAt ?? Date.now(),
        attempts: 0,
        last_error: null,
        executor: "device",
        needs_this_computer: true,
      };
      outbox.push(entry);
      sendWhenDue(entry);
      changed();
      return {
        outbox_id: entry.id,
        undo_until: entry.undo_until,
        send_at: sendAt,
        executor: "device",
        needs_this_computer: true,
      };
    }
    case "outbox_reschedule": {
      const entry = outbox.find((e) => e.id === args.id);
      if (!entry || (entry.state !== "queued" && entry.state !== "held")) throw new Error("It has already gone.");
      entry.state = "queued";
      entry.send_at = (args.sendAt as number | null | undefined) ?? null;
      entry.next_attempt_at = entry.send_at ?? Date.now();
      sendWhenDue(entry);
      changed();
      return { rescheduled: true };
    }
    case "identity_list":
      return { identities: identities.filter((i) => i.account_id === args.accountId) };
    case "identity_save": {
      const saved = args.identity as { id?: number; account_id: string; name: string; email: string; signature_id: number | null };
      const chosen = signatures.find((s) => s.id === saved.signature_id) ?? null;
      const signature = { signature_id: chosen?.id ?? null, signature: chosen?.body ?? "" };
      const existing = identities.find((i) => i.id === saved.id);
      if (existing) return Object.assign(existing, { name: saved.name, email: saved.email }, signature);
      const added: Identity = { ...saved, ...signature, id: nextId++, voice: "", voice_on: false, is_default: false };
      identities.push(added);
      return added;
    }
    case "identity_remove": {
      const index = identities.findIndex((i) => i.id === args.id);
      if (identities[index]?.is_default) throw new Error("The default address cannot be removed; make another one the default first.");
      if (index !== -1) identities.splice(index, 1);
      return null;
    }
    case "identity_make_default": {
      const chosen = identities.find((i) => i.id === args.id);
      for (const i of identities) if (i.account_id === chosen?.account_id) i.is_default = i === chosen;
      return null;
    }
    case "signature_list":
      return { signatures: [...signatures].sort((a, b) => a.name.localeCompare(b.name)) };
    case "signature_save": {
      const asked = args.signature as { id?: number; name: string; body: string };
      const fields = { name: asked.name.trim(), body: asked.body.trimEnd(), updated_at: Date.now() };
      if (!fields.name || !fields.body.trim()) throw new Error("a signature needs a name and some text");
      const existing = signatures.find((s) => s.id === asked.id);
      if (existing) {
        Object.assign(existing, fields);
        for (const i of identities) if (i.signature_id === existing.id) i.signature = existing.body;
        return existing;
      }
      const added: Signature = { id: nextId++, ...fields };
      signatures.push(added);
      return added;
    }
    case "signature_remove": {
      const index = signatures.findIndex((s) => s.id === args.id);
      if (index === -1) throw new Error("no such signature");
      signatures.splice(index, 1);
      for (const i of identities) if (i.signature_id === args.id) Object.assign(i, { signature_id: null, signature: "" });
      return { removed: true };
    }
    case "identity_set_signature": {
      const identity = identities.find((i) => i.id === args.id);
      if (!identity) throw new Error("no such identity");
      const chosen = signatures.find((s) => s.id === args.signatureId);
      return Object.assign(identity, { signature_id: chosen?.id ?? null, signature: chosen?.body ?? "" });
    }
    case "identity_set_voice": {
      const identity = identities.find((i) => i.id === args.id);
      if (!identity) throw new Error("no such identity");
      return Object.assign(identity, { voice: String(args.voice).trim(), voice_on: Boolean(args.on) });
    }
    case "note_list":
      return {
        notes: notes.filter(
          (n) => n.account_id === args.accountId && n.address === String(args.address).trim().toLowerCase(),
        ),
      };
    case "note_add": {
      const asked = args.note as { account_id: string; address: string; text: string };
      const note: Note = {
        id: nextId++,
        account_id: asked.account_id,
        address: asked.address.trim().toLowerCase(),
        text: asked.text.trim(),
        by: "person",
        created_at: Date.now(),
        updated_at: Date.now(),
      };
      notes.push(note);
      return { note };
    }
    case "note_update": {
      const note = notes.find((n) => n.id === args.id);
      if (!note) throw new Error("no such note");
      return Object.assign(note, { text: String(args.text).trim(), by: "person", updated_at: Date.now() });
    }
    case "note_remove": {
      const index = notes.findIndex((n) => n.id === args.id);
      if (index === -1) throw new Error("no such note");
      notes.splice(index, 1);
      return { removed: true };
    }
    case "note_settings":
    case "note_set_enabled": {
      if (cmd === "note_set_enabled") notesOn = Boolean(args.on);
      return { on: notesOn, notes: notes.length, people: new Set(notes.map((n) => `${n.account_id} ${n.address}`)).size };
    }
    case "note_forget_all": {
      const forgotten = notes.length;
      notes.length = 0;
      return { forgotten };
    }
    case "draft_set_from": {
      const draft = draftById(args.id as number);
      const old = identityFor(draft.account_id, draft.identity_id);
      // Another account's address moves the draft there, as the daemon does.
      const next = identities.find((i) => i.id === args.identityId);
      if (!next) throw new Error("There is no such address.");
      draft.account_id = next.account_id;
      const block = (signature: string) => (signature ? `\n\n${signature}` : "");
      const body =
        old?.signature && draft.body.includes(block(old.signature))
          ? draft.body.replace(block(old.signature), block(next.signature))
          : `${draft.body}${block(next.signature)}`;
      Object.assign(draft, { identity_id: next.id, from: { name: next.name, address: next.email }, body, updated_at: Date.now() });
      changed();
      return draft;
    }
    case "background_get":
      return background;
    case "background_set":
      background = args.on as boolean;
      return null;
    case "notifications_get":
      return notifications;
    case "notifications_set":
      notifications = args.on as boolean;
      return null;
    case "outbox_list":
      return { entries: outbox };
    case "outbox_undo": {
      const index = outbox.findIndex((e) => e.id === args.id);
      if (index === -1) throw new Error("It has already gone.");
      const [entry] = outbox.splice(index, 1);
      const draft = drafts.find((d) => d.id === entry.draft_id);
      if (draft) draft.sending = false;
      changed();
      return { draft_id: entry.draft_id };
    }
    case "search_messages": {
      const before = args.before as { received_at: number; id: number } | null;
      const limit = (args.limit as number | null) ?? 50;
      const { found, top } = searchSamples(String(args.query ?? ""), (args.accountId as string | null) ?? null);
      const after = found.filter(
        (m) => !before || m.received_at < before.received_at || (m.received_at === before.received_at && m.id < before.id),
      );
      const page = after.slice(0, limit);
      const last = page[page.length - 1];
      const query = String(args.query ?? "");
      const suggestion = !before && found.length === 0 ? suggest(query, (args.accountId as string | null) ?? null) : null;
      return {
        ...(suggestion ? { suggestion } : {}),
        messages: page.map((m) => summary(m)),
        top: before ? [] : top.map((m) => summary(m)),
        next: after.length > limit && last ? { received_at: last.received_at, id: last.id } : null,
        indexing: stillIndexing,
      };
    }
    case "search_server": {
      const before = args.before as { received_at: number; id: number } | null;
      const limit = (args.limit as number | null) ?? 50;
      const asked = (args.accountId as string | null) ?? null;
      const away = unreachableServer && accounts.length > 1 ? [accounts[1].account.id] : [];
      const reached = asked ? (away.includes(asked) ? [] : [asked]) : accounts.map((a) => a.account.id).filter((id) => !away.includes(id));
      const found = reached.flatMap((id) => searchSamples(String(args.query ?? ""), id, true).found);
      found.sort((a, b) => b.received_at - a.received_at || b.id - a.id);
      const after = found.filter(
        (m) => !before || m.received_at < before.received_at || (m.received_at === before.received_at && m.id < before.id),
      );
      const page = after.slice(0, limit);
      const last = page[page.length - 1];
      const result = {
        messages: page.map((m) => summary(m)),
        next: after.length > limit && last ? { received_at: last.received_at, id: last.id } : null,
        not_here: found.length > 0 ? 2 : 0,
        unreachable: away.filter((id) => !asked || id === asked),
      };
      // A server takes a moment.
      return new Promise((resolve) => window.setTimeout(() => resolve(result), 900));
    }
    case "smart_overview": {
      const owner = accounts.find((a) => a.account.id === args.accountId);
      const inbox = owner?.mailboxes.find((m) => m.role === "inbox")?.id;
      const listed = messages.filter((m) => m.mailboxId === inbox && !parked(m));
      const sections = SECTIONS.filter((section) => section !== "new_senders" || screeningMode !== "off")
        .map((section) => {
          const inSection = listed.filter((m) => shownIn(section, explanationOf(m).section));
          return { section, total: inSection.length, unread: inSection.filter((m) => !m.seen).length };
        })
        .filter((count) => count.total > 0);
      return { sections };
    }
    case "message_explain":
      return explanationOf(byId(args.id as number));
    case "sender_set": {
      const { account_id, address, policy, priority } = args.sender as {
        account_id: string;
        address: string;
        policy: Policy | null;
        priority: boolean;
      };
      const key = choiceKey(account_id, address);
      choices.set(key, { policy, priority, blocked: choices.get(key)?.blocked ?? false });
      changed();
      return { changed: messages.filter((m) => m.from.address?.toLowerCase() === address.toLowerCase()).length };
    }
    case "sender_block": {
      const { account_id, address, blocked } = args.sender as { account_id: string; address: string; blocked: boolean };
      const key = choiceKey(account_id, address);
      const before = choices.get(key);
      choices.set(key, { policy: before?.policy ?? null, priority: before?.priority ?? false, blocked });
      if (!blocked) {
        changed();
        return { moved: 0, undo: null };
      }
      const owner = accounts.find((a) => a.account.id === account_id);
      const inbox = owner?.mailboxes.find((m) => m.role === "inbox")?.id;
      const theirs = messages
        .filter((m) => m.mailboxId === inbox && m.from.address?.toLowerCase() === address.toLowerCase())
        .map((m) => m.id);
      const result = moveTo(theirs, (m) => roleBox(m, "junk"), "Blocked the sender and moved");
      const undo = undos.get(result.undo.id);
      if (undo) undo.also = () => void (before ? choices.set(key, before) : choices.delete(key));
      return result;
    }
    case "template_list":
      return { templates: [...templates].sort((a, b) => a.name.localeCompare(b.name)) };
    case "template_save": {
      const given = args.template as { id?: number; name: string; subject: string; body: string; html?: string };
      const name = given.name.trim();
      if (!name) throw new Error("give the template a name");
      // As the daemon does: with formatting, the text is derived from it.
      const body = given.html ? given.html.replace(/<\/p>/g, "\n").replace(/<[^>]*>/g, "").trim() : given.body;
      const saved: Template = {
        id: given.id ?? nextId++,
        name,
        subject: given.subject,
        body,
        html: given.html ?? null,
        updated_at: Date.now(),
      };
      const index = templates.findIndex((t) => t.id === saved.id);
      if (index === -1) templates.push(saved);
      else templates[index] = saved;
      return saved;
    }
    case "template_remove": {
      const index = templates.findIndex((t) => t.id === args.id);
      if (index !== -1) templates.splice(index, 1);
      return null;
    }
    case "smart_screening":
      return { mode: screeningMode };
    case "smart_set_screening":
      screeningMode = args.mode as Screening;
      changed();
      return { mode: screeningMode };
    case "outbox_send_again":
    case "outbox_dismiss": {
      const index = outbox.findIndex((e) => e.id === args.id);
      if (index !== -1) outbox.splice(index, 1);
      changed();
      return null;
    }
  }
  throw new Error(`The preview does not know "${cmd}".`);
}

mockIPC(
  (cmd, args) => {
    try {
      // As the real bridge does, a failure comes back as its message, also from a call that takes a moment.
      return Promise.resolve(handle(cmd, (args ?? {}) as Args)).catch((error: unknown) =>
        Promise.reject(error instanceof Error ? error.message : String(error)),
      );
    } catch (error) {
      return Promise.reject(error instanceof Error ? error.message : String(error));
    }
  },
  { shouldMockEvents: true },
);

// Files dragged and dropped on the window, as the desktop sends them: regardsDropFiles() in the console
// (the drag shows for `holdMs`, then the drop). Paths never reach the page; the preview attaches by name.
(window as unknown as { regardsDropFiles: (holdMs?: number) => void }).regardsDropFiles = (holdMs = 1200) => {
  void emit("files_drag", { over: true });
  window.setTimeout(() => {
    void emit("files_drag", { over: false });
    void emit("files_dropped", { id: 1, files: [{ name: "Garden-rota.pdf", size: 48_000, image: false }] });
  }, holdMs);
};

// A link opened while Regards runs, as the desktop sends it: regardsOpenLink("message:5") in the console.
// A mailto: one too: regardsOpenLink("mailto:ada@example.org?subject=Hello").
(window as unknown as { regardsOpenLink: (link: string) => void }).regardsOpenLink = (link) => {
  void emit("link_open", parseLink(link));
};

// Message bodies: the HTML straight into the sandboxed frame.
(window as unknown as { __TAURI_INTERNALS__: { convertFileSrc: (path: string) => string } }).__TAURI_INTERNALS__.convertFileSrc =
  (path: string) => {
    const id = Number(path.match(/message\/(\d+)/)?.[1]);
    const html = messages.find((m) => m.id === id)?.html ?? "";
    return `data:text/html;charset=utf-8,${encodeURIComponent(html)}`;
  };
