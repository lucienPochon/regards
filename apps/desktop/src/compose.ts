import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import type { MessageSummary, Person } from "./accounts";

/** Mirrors `DraftView` in `regards-ipc`. */
export interface Draft {
  id: number;
  account_id: string;
  /** Who it goes out from: its identity's name and address. */
  from: Person;
  /** That identity (see identities.ts). */
  identity_id: number | null;
  to: Person[];
  cc: Person[];
  bcc: Person[];
  subject: string;
  body: string;
  /** The formatted version, when it was written with formatting. */
  html: string | null;
  replying_to: number | null;
  updated_at: number;
  /** In the outbox: take it back before changing it. */
  sending: boolean;
  attachments: DraftAttachment[];
}

export interface DraftAttachment {
  id: number;
  name: string;
  mime: string;
  size: number;
  /** An image in the text (images.ts), not a file beside it. */
  content_id?: string | null;
}

/**
 * `held`: scheduled, but found well past its time (the computer was off); it waits for the person.
 * `handed_off` (from 1.1): a remote executor holds it and sends it at its time, computer on or off.
 */
export type OutboxState = "queued" | "held" | "handed_off" | "sending" | "failed" | "uncertain";

/** Who sends a queued message (plan §5.7). Regards 1.0 has only "device": this computer. */
export type Executor = "device" | "provider" | "self_hosted" | "send_service";

export interface OutboxEntry {
  id: number;
  account_id: string;
  draft_id: number | null;
  subject: string;
  recipients: Person[];
  state: OutboxState;
  undo_until: number;
  /** Send later: when it is meant to go, Unix ms. */
  send_at: number | null;
  next_attempt_at: number;
  attempts: number;
  last_error: string | null;
  executor?: Executor;
  /** See `Queued.needs_this_computer`. */
  needs_this_computer?: boolean;
  /**
   * Queued, and its account must be signed in again (`AccountSummary.needs_sign_in`):
   * it goes as soon as that is done.
   */
  waiting_for_sign_in?: boolean;
  /** An agent asked for it: it goes after the account's undo window for agents, or the person approved it. */
  by_agent?: boolean;
}

export interface Queued {
  outbox_id: number;
  /** Until when the send can be undone, Unix time in milliseconds. */
  undo_until: number;
  /** Send later: when it goes. Until then it can be taken back. */
  send_at: number | null;
  executor?: Executor;
  /**
   * It goes from this computer: when scheduled, say that the computer must be
   * on then ("your computer must be on at 09:00", ADR-0019).
   */
  needs_this_computer?: boolean;
  /**
   * Sent with Send and Done (Ctrl+Shift+Enter): the conversation it answers
   * is filed as Done, whatever the setting. Set by the composer.
   */
  done?: boolean;
}

/** Something to ask about before a draft goes (`checkDraft`). */
export type SendCheck =
  | { kind: "no_recipients" }
  | { kind: "no_subject" }
  | { kind: "missing_attachment"; word: string }
  | { kind: "placeholder"; text: string };

/**
 * What to ask about before a saved draft goes: none when it can go as it is.
 * `no_recipients` stops it; for the others the person confirms (send again).
 * Only what was written counts, not a quoted reply or forwarded message.
 */
export const checkDraft = (id: number) =>
  invoke<{ checks: SendCheck[] }>("draft_check", { id }).then((result) => result.checks);

/** What a check says, for the composer. */
export function describeCheck(check: SendCheck): string {
  switch (check.kind) {
    case "no_recipients":
      return "Add at least one recipient.";
    case "no_subject":
      return "There is no subject.";
    case "missing_attachment":
      return `It mentions “${check.word}”, but nothing is attached.`;
    case "placeholder":
      return `“${check.text}” is still to be filled in.`;
  }
}

/**
 * Whom to write to, as a recipient is typed: the start of an address or of
 * a word of a name; empty gives those written with most. Best first: those
 * the account writes to, lately, before those who only wrote to it. No-reply
 * and blocked addresses are left out. `accountId`: only that account's people.
 */
export const suggestPeople = (query: string, accountId: string | null = null) =>
  invoke<{ people: Person[] }>("people_suggest", { query, accountId }).then((result) => result.people);

/**
 * Stop suggesting someone (a mistyped address, say) until there is new mail
 * with them: by `accountId` only, or every account. How many accounts knew them.
 */
export const forgetPerson = (address: string, accountId: string | null = null) =>
  invoke<{ forgotten: number }>("people_forget", { address, accountId }).then((result) => result.forgotten);

/**
 * Drafts started in this window (a new message, a reply, a forward) and not
 * written in yet: closed as they are, they are deleted rather than kept and
 * copied to the server's Drafts. Not drafts opened from Drafts, from a
 * template, a link or an agent: those have something to keep.
 */
const untouched = new Set<number>();
const started = (draft: Draft) => {
  untouched.add(draft.id);
  return draft;
};
/** Whether a draft was started here and nothing was done to it yet (see `untouched`). */
export const isUntouched = (id: number) => untouched.has(id);
/** Something was done to it: kept from now on, even closed as it is. */
export const touchDraft = (id: number) => untouched.delete(id);

export const newDraft = (accountId: string) => invoke<Draft>("draft_create", { accountId }).then(started);
/** This computer's time minus UTC, in minutes: a reply's "On … wrote:" and a forward's "Date:" are in it. */
const localOffset = () => -new Date().getTimezoneOffset();
/** A reply, addressed and quoted (formatted mail formatted, in a cited blockquote). */
export const replyDraft = (messageId: number, all: boolean) =>
  invoke<Draft>("draft_reply", { messageId, all, utcOffsetMinutes: localOffset() }).then(started);
/** A forward, with the original's files (and formatting, when it has some). */
export const forwardDraft = (messageId: number) =>
  invoke<Draft>("draft_forward", { messageId, utcOffsetMinutes: localOffset() }).then(started);
export const listDrafts = () => invoke<{ drafts: Draft[] }>("draft_list");
export const getDraft = (id: number) => invoke<Draft>("draft_get", { id });
/** Pick files with the system dialog and attach them; `null` if cancelled. */
export const attachFiles = (draftId: number) => invoke<Draft | null>("draft_attach_files", { draftId });
export const detachFile = (draftId: number, attachmentId: number) =>
  invoke<Draft>("draft_detach", { draftId, attachmentId });
export const deleteDraft = (id: number) => invoke("draft_delete", { id });
/** Drafts written elsewhere (another device, webmail), in the server's Drafts folders. */
export const listDraftsElsewhere = () =>
  invoke<{ messages: MessageSummary[] }>("draft_elsewhere").then((result) => result.messages);
/** Pick one up: it becomes a draft here, kept in step with its copy on the server. */
export const adoptDraft = (messageId: number) => invoke<Draft>("draft_adopt", { messageId });
/** Send it from another of the account's identities; a plain-text draft's signature is swapped. */
export const setDraftFrom = (id: number, identityId: number) => invoke<Draft>("draft_set_from", { id, identityId });

export function saveDraft(
  draft: Pick<Draft, "id" | "to" | "cc" | "bcc" | "subject" | "body"> & { html?: string | null },
): Promise<Draft> {
  const { id, to, cc, bcc, subject, body, html } = draft;
  return invoke<Draft>("draft_update", { draft: { id, to, cc, bcc, subject, body, html: html ?? null } });
}

/**
 * Queue it; it goes once the undo window is over, or at `sendAt` (Unix ms,
 * in the future). A scheduled message goes from this computer, which must be
 * on then. With `remindAt`, it comes back on top of the inbox then if nobody
 * has replied (see `remindMessages`).
 */
export const sendDraft = (id: number, undoSeconds = 10, sendAt: number | null = null, remindAt: number | null = null) =>
  invoke<Queued>("draft_send", { id, undoSeconds, sendAt, remindAt });

export const listOutbox = () => invoke<{ entries: OutboxEntry[] }>("outbox_list");
/** Undo send, or edit a failed message: returns the draft to reopen. */
export const undoSend = (id: number) => invoke<{ draft_id: number | null }>("outbox_undo", { id });
export const sendAgain = (id: number) => invoke("outbox_send_again", { id });
/** A scheduled or held message: at another time, or now (`null`). */
export const reschedule = (id: number, sendAt: number | null) => invoke("outbox_reschedule", { id, sendAt });
export const dismissOutbox = (id: number) => invoke("outbox_dismiss", { id });

/**
 * Parse what is typed in an address field: `Ada <ada@example.org>, bob@example.org`.
 * Commas inside quotes or angle brackets do not split.
 */
export function parseRecipients(text: string): Person[] {
  const parts: string[] = [];
  let current = "";
  let quoted = false;
  let angled = false;
  for (const char of text) {
    if (char === '"') quoted = !quoted;
    else if (char === "<" && !quoted) angled = true;
    else if (char === ">" && !quoted) angled = false;
    if ((char === "," || char === ";") && !quoted && !angled) {
      parts.push(current);
      current = "";
    } else {
      current += char;
    }
  }
  parts.push(current);
  return parts
    .map((part) => part.trim())
    .filter((part) => part.length > 0)
    .map((part) => {
      const match = part.match(/^(.*?)\s*<([^<>]*)>\s*$/);
      if (match) {
        const name = match[1].trim().replace(/^"(.*)"$/, "$1").trim();
        return { name: name || null, address: match[2].trim() };
      }
      return { name: null, address: part };
    });
}

export function formatRecipients(people: Person[]): string {
  return people
    .map((person) => {
      const address = person.address ?? "";
      if (!person.name) return address;
      const name = /[,;<>"]/.test(person.name) ? `"${person.name.replace(/"/g, "")}"` : person.name;
      return `${name} <${address}>`;
    })
    .join(", ");
}

/** Addresses that are clearly not addresses, to point out before sending. */
export function invalidAddresses(people: Person[]): string[] {
  return people
    .map((person) => person.address ?? "")
    .filter((address) => !/^[^\s@<>,"]+@[^\s@<>,"]+\.[^\s@<>,"]+$/.test(address));
}

/** A plain-text draft (a new reply's quote, say) as editor HTML: paragraphs, and "> " lines as quotes. */
/**
 * The draft's HTML sent from another identity: the old signature becomes the
 * new one. Where the old one is not found (none, or edited), the new one goes
 * above the quoted ("On …, Bob wrote:") or forwarded message, or at the end.
 * Signatures are plain text, put in as `textToHtml` does.
 */
export function swapSignatureHtml(html: string, old: string, next: string): string {
  const block = (signature: string) => (signature.trim() ? textToHtml(signature.trimEnd()) : "");
  const [from, to] = [block(old), block(next)];
  if (from && html.includes(from)) return html.replace(from, () => to);
  if (!to) return html;
  const quote = html.search(/<p>[^<]*wrote:<\/p><blockquote>|<p>---------- Forwarded message ----------/);
  return quote === -1 ? html + to : html.slice(0, quote) + to + html.slice(quote);
}

/**
 * A draft's HTML with a template's text put at the start, where one writes:
 * in place of the empty paragraph a new message starts with, above the
 * signature and any quote.
 */
export function insertTemplateHtml(html: string, template: { body: string; html: string | null }): string {
  const insert = template.html ?? textToHtml(template.body);
  const rest = html.startsWith("<p></p>") ? html.slice("<p></p>".length) : html;
  return insert + rest;
}

/**
 * A template's name placeholders filled in, as Spark's templates do:
 * `{{name}}`, `{{first name}}` and `{{last name}}` from the first recipient,
 * `{{my name}}` and `{{my first name}}` from whom it is sent as. Case and
 * spaces inside the braces do not matter. One with no name to fill it (a
 * recipient known only by address), any other `{{…}}`, and `[[blanks]]`
 * stay as they are: the send check asks about them. With `html`, what goes
 * in is escaped.
 */
export function fillPlaceholders(
  text: string,
  names: { to: string | null; me: string | null },
  html = false,
): string {
  const words = (name: string | null) => (name ?? "").trim().split(/\s+/).filter(Boolean);
  const [to, me] = [words(names.to), words(names.me)];
  const values: Record<string, string | undefined> = {
    name: to.join(" ") || undefined,
    firstname: to[0],
    lastname: to.length > 1 ? to[to.length - 1] : undefined,
    myname: me.join(" ") || undefined,
    myfirstname: me[0],
  };
  const escape = (value: string) =>
    html ? value.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;") : value;
  return text.replace(/\{\{([^{}\n]{1,40})\}\}/g, (whole, inside: string) => {
    const value = values[inside.toLowerCase().replace(/\s+/g, "")];
    return value === undefined ? whole : escape(value);
  });
}

export function textToHtml(text: string): string {
  const escape = (line: string) =>
    line.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
  const blocks: string[] = [];
  let paragraph: string[] = [];
  let quote: string[] = [];
  const endParagraph = () => {
    if (paragraph.length) blocks.push(`<p>${paragraph.map(escape).join("<br>")}</p>`);
    paragraph = [];
  };
  const endQuote = () => {
    if (quote.length) blocks.push(`<blockquote>${textToHtml(quote.join("\n"))}</blockquote>`);
    quote = [];
  };
  for (const line of text.replace(/\r\n?/g, "\n").split("\n")) {
    if (line.startsWith(">")) {
      endParagraph();
      quote.push(line.replace(/^> ?/, ""));
    } else if (line.trim() === "") {
      endParagraph();
      endQuote();
      if (blocks.length === 0) blocks.push("<p></p>");
    } else {
      endQuote();
      paragraph.push(line);
    }
  }
  endParagraph();
  endQuote();
  return blocks.join("") || "<p></p>";
}

/** A file dropped on the window (its path stays in the app's Rust side). */
export interface DroppedFile {
  name: string;
  size: number | null;
  /** An image, which could go in the text instead. */
  image: boolean;
}

export interface Dropped {
  id: number;
  files: DroppedFile[];
  /** Where in the window (CSS pixels): on the text, images go in it. */
  x: number;
  y: number;
}

/**
 * Attach the files of a drop to a draft, once; with `inline`, as images in
 * its text. What could not be attached comes back, with why.
 */
export const attachDropped = (draftId: number, dropId: number, inline = false) =>
  invoke<{ draft: Draft | null; refused: { name: string; error: string }[] }>("draft_attach_dropped", {
    draftId,
    dropId,
    inline,
  });

/**
 * An event for this window only, as files dropped on one window are for it;
 * in the browser preview, which has no windows, any.
 */
function listenHere<T>(name: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  let here: ReturnType<typeof getCurrentWebviewWindow> | null = null;
  try {
    here = getCurrentWebviewWindow();
  } catch {
    // The preview.
  }
  return here ? here.listen<T>(name, (event) => handler(event.payload)) : listen<T>(name, (event) => handler(event.payload));
}

/** Files are dragged over the window (`true`), or no longer are. */
export const onFilesDrag = (handler: (over: boolean) => void): Promise<UnlistenFn> =>
  listenHere<{ over: boolean }>("files_drag", (payload) => handler(payload.over));

/** Files were dropped on the window. */
export const onFilesDropped = (handler: (dropped: Dropped) => void): Promise<UnlistenFn> =>
  listenHere<Dropped>("files_dropped", handler);

/** Open a draft in a window of its own, or bring that window to the front. */
export const openComposerWindow = (draftId: number, title: string) => invoke("composer_window", { draftId, title });

/** Whether a draft is open in a window of its own. */
export const composerWindowOpen = (draftId: number) => invoke<boolean>("composer_window_open", { draftId });

/** The draft a window of its own was opened for (its label is `compose-<id>`); `null` in the main window. */
export function composerWindowDraft(): number | null {
  let label = "";
  try {
    label = getCurrentWebviewWindow().label;
  } catch {
    // The preview: `?compose=<id>` stands in.
    label = `compose-${new URLSearchParams(window.location.search).get("compose") ?? ""}`;
  }
  const id = Number(label.match(/^compose-(\d+)$/)?.[1]);
  return Number.isInteger(id) && id > 0 ? id : null;
}

/** What a message's own window tells the main one when it is sent: the undo is shown there. */
export interface SentElsewhere {
  queued: Queued;
  accountId?: string;
  replyingTo?: number | null;
}
