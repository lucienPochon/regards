import { invoke } from "@tauri-apps/api/core";
import type { Person } from "./accounts";

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
}

/** `held`: scheduled, but found well past its time (the computer was off); it waits for the person. */
export type OutboxState = "queued" | "held" | "sending" | "failed" | "uncertain";

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
}

export interface Queued {
  outbox_id: number;
  /** Until when the send can be undone, Unix time in milliseconds. */
  undo_until: number;
  /** Send later: when it goes. Until then it can be taken back. */
  send_at: number | null;
}

export const newDraft = (accountId: string) => invoke<Draft>("draft_create", { accountId });
export const replyDraft = (messageId: number, all: boolean) => invoke<Draft>("draft_reply", { messageId, all });
export const forwardDraft = (messageId: number) => invoke<Draft>("draft_forward", { messageId });
export const listDrafts = () => invoke<{ drafts: Draft[] }>("draft_list");
export const getDraft = (id: number) => invoke<Draft>("draft_get", { id });
/** Pick files with the system dialog and attach them; `null` if cancelled. */
export const attachFiles = (draftId: number) => invoke<Draft | null>("draft_attach_files", { draftId });
export const detachFile = (draftId: number, attachmentId: number) =>
  invoke<Draft>("draft_detach", { draftId, attachmentId });
export const deleteDraft = (id: number) => invoke("draft_delete", { id });
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
