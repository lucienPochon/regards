// A stand-in for the desktop bridge and daemon, so the UI runs in a plain
// browser with sample mail (`npm run dev`, then open /preview.html). Nothing
// here is part of the app build.

import { emit } from "@tauri-apps/api/event";
import { mockIPC } from "@tauri-apps/api/mocks";
import type { MessageSummary, Person } from "../accounts";
import type { Draft, OutboxEntry } from "../compose";
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
  { description: string; restore: Map<number, number>; marks?: Map<number, Pick<FixtureMessage, "pinned" | "aside" | "snoozed_until">> }
>();
const SOMEDAY = 32_503_680_000_000;
const parked = (m: FixtureMessage) => m.aside === true || (m.snoozed_until ?? 0) > Date.now();
let nextId = 10_000;

if (accounts.length > 0) {
  drafts.push({
    id: nextId++,
    account_id: PERSONAL.account.id,
    from: ME,
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

function summary(message: FixtureMessage): MessageSummary {
  return {
    id: message.id,
    uid: message.id,
    subject: message.subject,
    from: message.from,
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
  };
}

/** Change a mark on messages, with an undo entry, as the daemon does. */
function mark(ids: number[], change: (message: FixtureMessage) => void, description: string) {
  const marks = new Map<number, Pick<FixtureMessage, "pinned" | "aside" | "snoozed_until">>();
  for (const id of ids) {
    const message = byId(id);
    marks.set(id, { pinned: message.pinned, aside: message.aside, snoozed_until: message.snoozed_until });
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
  const draft: Draft = {
    id: nextId++,
    account_id: owner.account.id,
    from: { name: owner.account.display_name, address: owner.account.email },
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
      const all = messages
        .filter((m) => m.mailboxId === args.mailboxId && !(inbox && parked(m)))
        .sort((a, b) => b.received_at - a.received_at || b.id - a.id)
        .filter((m) => !before || m.received_at < before.received_at || (m.received_at === before.received_at && m.id < before.id));
      const page = all.slice(0, limit);
      const last = page[page.length - 1];
      return {
        messages: page.map(summary),
        next: all.length > limit && last ? { received_at: last.received_at, id: last.id } : null,
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
    case "message_archive":
      return moveTo(args.ids as number[], (m) => roleBox(m, "archive"), "Archived");
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
    case "message_view": {
      const owner = accounts.find((a) => a.account.id === args.accountId);
      const shown = messages
        .filter((m) => owner?.mailboxes.some((b) => b.id === m.mailboxId))
        .filter((m) =>
          args.view === "pinned" ? m.pinned : args.view === "aside" ? m.aside : (m.snoozed_until ?? 0) > Date.now(),
        )
        .sort((a, b) => b.received_at - a.received_at || b.id - a.id);
      return { messages: shown.map(summary), next: null };
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
      const entry: OutboxEntry = {
        id: nextId++,
        account_id: draft.account_id,
        draft_id: draft.id,
        subject: draft.subject,
        recipients: [...draft.to, ...draft.cc, ...draft.bcc],
        state: "queued",
        undo_until: Date.now() + ((args.undoSeconds as number) ?? 10) * 1000,
        next_attempt_at: Date.now(),
        attempts: 0,
        last_error: null,
      };
      outbox.push(entry);
      window.setTimeout(() => {
        const index = outbox.indexOf(entry);
        if (index === -1) return;
        outbox.splice(index, 1);
        const at = drafts.indexOf(draft);
        if (at !== -1) drafts.splice(at, 1);
        const sent = accounts.find((a) => a.account.id === draft.account_id)?.mailboxes.find((m) => m.role === "sent");
        if (sent) {
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
      }, entry.undo_until - Date.now() + 1500);
      changed();
      return { outbox_id: entry.id, undo_until: entry.undo_until };
    }
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
