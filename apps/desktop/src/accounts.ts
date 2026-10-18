import { convertFileSrc, invoke } from "@tauri-apps/api/core";

export type ConnectionSecurity = "tls" | "starttls";

export interface ServerSettings {
  host: string;
  port: number;
  security: ConnectionSecurity;
}

/** Mirrors `AccountSummary` in `regards-ipc` (wire names are snake_case). */
export interface AccountSummary {
  id: string;
  display_name: string;
  email: string;
  username: string;
  imap: ServerSettings;
  smtp: ServerSettings;
  imap_capabilities: string[];
  created_at: number;
}

/** Mirrors `AccountInput` in `src-tauri/src/main.rs`. IMAP always uses TLS. */
export interface AccountInput {
  displayName: string;
  email: string;
  username: string | null;
  password: string;
  imapHost: string;
  imapPort: number;
  smtpHost: string;
  smtpPort: number;
  smtpSecurity: ConnectionSecurity;
}

export function listAccounts(): Promise<AccountSummary[]> {
  return invoke<AccountSummary[]>("account_list");
}

/** Signs in to the server to check the details, then saves the account. */
export function addAccount(input: AccountInput): Promise<AccountSummary> {
  return invoke<AccountSummary>("account_add", { input });
}

export function removeAccount(id: string): Promise<void> {
  return invoke<void>("account_remove", { id });
}

export type Role = "inbox" | "sent" | "drafts" | "trash" | "junk" | "archive" | "all" | "flagged";

export interface MailboxSummary {
  id: number;
  name: string;
  delimiter: string | null;
  role: Role | null;
  selectable: boolean;
}

export interface SyncState {
  running: boolean;
  last_success_at: number | null;
  last_error: string | null;
  /** A push connection is waiting for new mail. */
  push_active: boolean;
}

export interface MailboxListResult {
  mailboxes: MailboxSummary[];
  sync: SyncState;
}

export function listMailboxes(accountId: string): Promise<MailboxListResult> {
  return invoke<MailboxListResult>("mailbox_list", { accountId });
}

/** Syncs now; resolves when the sync is done, rejects with the reason if it failed. */
export function syncAccount(accountId: string): Promise<unknown> {
  return invoke("account_sync", { accountId });
}

export interface Person {
  name: string | null;
  address: string | null;
}

export interface MessageCursor {
  received_at: number;
  id: number;
}

export interface MessageSummary {
  id: number;
  uid: number;
  subject: string;
  from: Person | null;
  received_at: number;
  sent_at: number | null;
  seen: boolean;
  flagged: boolean;
  answered: boolean;
  size: number | null;
}

export interface MessageListResult {
  messages: MessageSummary[];
  next: MessageCursor | null;
}

/** A page of a mailbox's messages, newest first; pass `next` back for the following page. */
export function listMessages(
  mailboxId: number,
  before: MessageCursor | null,
  limit: number | null = null,
): Promise<MessageListResult> {
  return invoke<MessageListResult>("message_list", { mailboxId, before, limit });
}

export interface Attachment {
  name: string | null;
  mime: string;
  size: number;
  inline: boolean;
}

export interface MessageDetail {
  message: MessageSummary;
  to: Person[];
  cc: Person[];
  text: string;
  has_html: boolean;
  /** Remote images in the HTML version, not loaded unless asked for. */
  remote_images: number;
  attachments: Attachment[];
}

/** One message with its body; downloaded from the server the first time. */
export function getMessage(id: number): Promise<MessageDetail> {
  return invoke<MessageDetail>("message_get", { id });
}

/**
 * Where the sandboxed reading frame loads a message's sanitised HTML from
 * (the `regards-mail` scheme, served by the desktop bridge with a strict CSP).
 */
export function messageHtmlUrl(id: number, allowRemote: boolean): string {
  return convertFileSrc(`message/${id}?remote=${allowRemote ? 1 : 0}`, "regards-mail");
}

/** Mark messages read or unread, flagged or not. Applied at once; the server is told in the background. */
export function setFlags(ids: number[], flags: { seen?: boolean; flagged?: boolean }): Promise<{ changed: number }> {
  return invoke("message_set_flags", { ids, seen: flags.seen ?? null, flagged: flags.flagged ?? null });
}

export interface UndoInfo {
  id: number;
  description: string;
}

export interface MoveResult {
  moved: number;
  undo: UndoInfo | null;
}

export function archiveMessages(ids: number[]): Promise<MoveResult> {
  return invoke<MoveResult>("message_archive", { ids });
}

export function trashMessages(ids: number[]): Promise<MoveResult> {
  return invoke<MoveResult>("message_trash", { ids });
}

export function moveMessages(ids: number[], mailboxId: number): Promise<MoveResult> {
  return invoke<MoveResult>("message_move", { ids, mailboxId });
}

/** Reverse a recent action. Each undo id works once, for a few minutes. */
export function applyUndo(id: number): Promise<{ undone: number }> {
  return invoke("undo_apply", { id });
}
