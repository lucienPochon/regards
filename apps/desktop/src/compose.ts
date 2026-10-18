import { invoke } from "@tauri-apps/api/core";
import type { Person } from "./accounts";

/** Mirrors `DraftView` in `regards-ipc`. */
export interface Draft {
  id: number;
  account_id: string;
  /** Who it goes out from: the account's name and address. */
  from: Person;
  to: Person[];
  cc: Person[];
  bcc: Person[];
  subject: string;
  body: string;
  replying_to: number | null;
  updated_at: number;
  /** In the outbox: take it back before changing it. */
  sending: boolean;
}

export type OutboxState = "queued" | "sending" | "failed" | "uncertain";

export interface OutboxEntry {
  id: number;
  account_id: string;
  draft_id: number | null;
  subject: string;
  recipients: Person[];
  state: OutboxState;
  undo_until: number;
  next_attempt_at: number;
  attempts: number;
  last_error: string | null;
}

export interface Queued {
  outbox_id: number;
  /** Until when the send can be undone, Unix time in milliseconds. */
  undo_until: number;
}

export const newDraft = (accountId: string) => invoke<Draft>("draft_create", { accountId });
export const replyDraft = (messageId: number, all: boolean) => invoke<Draft>("draft_reply", { messageId, all });
export const forwardDraft = (messageId: number) => invoke<Draft>("draft_forward", { messageId });
export const listDrafts = () => invoke<{ drafts: Draft[] }>("draft_list");
export const getDraft = (id: number) => invoke<Draft>("draft_get", { id });
export const deleteDraft = (id: number) => invoke("draft_delete", { id });

export function saveDraft(draft: Pick<Draft, "id" | "to" | "cc" | "bcc" | "subject" | "body">): Promise<Draft> {
  const { id, to, cc, bcc, subject, body } = draft;
  return invoke<Draft>("draft_update", { draft: { id, to, cc, bcc, subject, body } });
}

/** Queue it; it goes once the undo window is over. */
export const sendDraft = (id: number, undoSeconds = 10) => invoke<Queued>("draft_send", { id, undoSeconds });

export const listOutbox = () => invoke<{ entries: OutboxEntry[] }>("outbox_list");
/** Undo send, or edit a failed message: returns the draft to reopen. */
export const undoSend = (id: number) => invoke<{ draft_id: number | null }>("outbox_undo", { id });
export const sendAgain = (id: number) => invoke("outbox_send_again", { id });
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
