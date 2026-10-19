// A stand-in for the desktop bridge and daemon, so the UI runs in a plain
// browser with sample mail (`npm run dev`, then open /preview.html). Nothing
// here is part of the app build.

import { emit } from "@tauri-apps/api/event";
import { mockIPC } from "@tauri-apps/api/mocks";
import type { MessageSummary, Person } from "../accounts";
import type { Draft, OutboxEntry } from "../compose";
import type { Identity } from "../identities";
import { type FixtureAccount, type FixtureMessage, ME, PERSONAL, WORK, fixtureMessages } from "./fixtures";

type Args = Record<string, unknown>;

const params = new URLSearchParams(window.location.search);
const accountCount = Number(params.get("accounts") ?? "1");
const accounts: FixtureAccount[] = accountCount === 0 ? [] : accountCount >= 2 ? [PERSONAL, WORK] : [PERSONAL];
const started = Date.now();
let messages: FixtureMessage[] = fixtureMessages(started, accounts);
const drafts: Draft[] = [];
const outbox: OutboxEntry[] = [];
const undos = new Map<
  number,
  { description: string; restore: Map<number, number>; marks?: Map<number, Marks> }
>();
const SOMEDAY = 32_503_680_000_000;
type Marks = Pick<FixtureMessage, "pinned" | "aside" | "snoozed_until" | "remind_at" | "reminded" | "surfaced_at">;
const parked = (m: FixtureMessage) => m.aside === true || (m.snoozed_until ?? 0) > Date.now();
/** Its place in the inbox: what came back is listed by when it did, as the daemon does. */
const inboxPlace = (m: FixtureMessage) =>
  m.surfaced_at && (m.reminded || m.surfaced_at > m.received_at) ? m.surfaced_at : m.received_at;
let nextId = 10_000;
let background = true;
let notifications = true;

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
    signature: "",
    is_default: true,
  },
  ...(index === 0
    ? [
        {
          id: nextId++,
          account_id: owner.account.id,
          name: `${owner.account.display_name} (work)`,
          email: "sam@studio.example",
          signature: "Sam Rivera · Studio Kiln",
          is_default: false,
        },
      ]
    : []),
]);
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
    case "account_discover": {
      const domain = String(args.email ?? "").split("@")[1]?.toLowerCase() ?? "";
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
    case "update_status":
      return { checks: null, current: "0.1.0", available: null, checked_at: null };
    case "update_set_checks":
      return null;
    case "update_check":
      return { version: "0.2.0", url: "https://github.com/lucienPochon/regards/releases/tag/v0.2.0", published_at: null };
    case "update_open":
      return null;
    case "account_oauth_providers":
      return { providers: ["microsoft", "google"] };
    case "account_oauth_start":
      return { flow_id: "preview-flow", url: "https://login.example.test/authorize" };
    case "account_oauth_finish":
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
    case "account_sync":
      return new Promise((resolve) => window.setTimeout(() => resolve(null), 900));
    case "mailbox_list": {
      const owner = accounts.find((a) => a.account.id === args.accountId);
      return {
        mailboxes: owner?.mailboxes ?? [],
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
      const all = messages
        .filter((m) =>
          inbox
            ? !parked(m) && (m.mailboxId === args.mailboxId || (m.reminded === true && account?.mailboxes.some((b) => b.id === m.mailboxId)))
            : m.mailboxId === args.mailboxId,
        )
        .sort((a, b) => place(b) - place(a) || b.id - a.id)
        .filter((m) => !before || place(m) < before.received_at || (place(m) === before.received_at && m.id < before.id));
      const page = all.slice(0, limit);
      const last = page[page.length - 1];
      return {
        messages: page.map((m) => summary(m, place(m))),
        next: all.length > limit && last ? { received_at: place(last), id: last.id } : null,
      };
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
        attachments: message.attachments,
      };
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
    case "message_set_flags": {
      const ids = args.ids as number[];
      for (const id of ids) {
        const message = byId(id);
        if (typeof args.seen === "boolean") message.seen = args.seen;
        if (typeof args.flagged === "boolean") message.flagged = args.flagged;
      }
      changed();
      return { changed: ids.length };
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
      const saved = args.identity as Omit<Identity, "is_default"> & { id?: number };
      const existing = identities.find((i) => i.id === saved.id);
      if (existing) return Object.assign(existing, { name: saved.name, email: saved.email, signature: saved.signature });
      const added: Identity = { ...saved, id: nextId++, is_default: false };
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
    case "draft_set_from": {
      const draft = draftById(args.id as number);
      const old = identityFor(draft.account_id, draft.identity_id);
      const next = identities.find((i) => i.id === args.identityId);
      if (!next) throw new Error("That address is not one of this account's.");
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
      return Promise.resolve(handle(cmd, (args ?? {}) as Args));
    } catch (error) {
      return Promise.reject(error instanceof Error ? error.message : String(error));
    }
  },
  { shouldMockEvents: true },
);

// Message bodies: the HTML straight into the sandboxed frame.
(window as unknown as { __TAURI_INTERNALS__: { convertFileSrc: (path: string) => string } }).__TAURI_INTERNALS__.convertFileSrc =
  (path: string) => {
    const id = Number(path.match(/message\/(\d+)/)?.[1]);
    const html = messages.find((m) => m.id === id)?.html ?? "";
    return `data:text/html;charset=utf-8,${encodeURIComponent(html)}`;
  };
