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
  /** Unread messages (for the inbox, not counting those set aside or snoozed), when the daemon says. */
  unread?: number;
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
  /** The folder it is in, or on its way to. */
  mailbox_id: number;
  uid: number;
  subject: string;
  from: Person | null;
  /** The first few recipients (To, then Cc). */
  to: Person[];
  received_at: number;
  sent_at: number | null;
  seen: boolean;
  flagged: boolean;
  answered: boolean;
  size: number | null;
  /** Pinned on this device. */
  pinned: boolean;
  /** Set aside: out of the inbox until brought back. */
  aside: boolean;
  /** Out of the inbox until then (Unix ms), if snoozed. */
  snoozed_until: number | null;
  /** "Remind me if no reply by" then (Unix ms). */
  remind_at: number | null;
  /** A reminder came due with no reply: shown in the inbox until done (archive). */
  reminded: boolean;
  /**
   * Its place in the list: `received_at`, except in the inbox for what came
   * back to it (a snooze that ended, a reminder), listed by when it did.
   */
  listed_at: number;
  /** The start of its text, for the list's third line, when the daemon has it. */
  preview?: string | null;
  /** It has attached files (not inline images), when the daemon says. */
  has_attachments?: boolean;
  /** How many messages its conversation has, when the daemon says. */
  thread_size?: number;
}

export interface MessageListResult {
  messages: MessageSummary[];
  next: MessageCursor | null;
}

/** A page of a mailbox's messages, newest first; pass `next` back for the following page. */
/** A page of a folder; in the inbox, of one Smart Inbox section (see smart.ts) when `section` is given. */
export function listMessages(
  mailboxId: number,
  before: MessageCursor | null,
  limit: number | null = null,
  section: string | null = null,
): Promise<MessageListResult> {
  return invoke<MessageListResult>("message_list", { mailboxId, before, limit, section });
}

export interface Attachment {
  name: string | null;
  mime: string;
  size: number;
  inline: boolean;
}

export interface MessageDetail {
  message: MessageSummary;
  /** The folder it is in, or on its way to. */
  mailbox_id: number;
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

export interface ThreadEntry {
  message: MessageSummary;
  mailbox_id: number;
  mailbox_name: string;
  mailbox_role: Role | null;
}

/** The conversation a message belongs to, across folders, oldest first. */
export function getThread(id: number): Promise<{ messages: ThreadEntry[] }> {
  return invoke("thread_get", { id });
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

/** Delete for good, from Trash or Junk only. This cannot be undone: confirm first. */
export function deleteForever(ids: number[]): Promise<{ deleted: number }> {
  return invoke("message_delete", { ids });
}

export function moveMessages(ids: number[], mailboxId: number): Promise<MoveResult> {
  return invoke<MoveResult>("message_move", { ids, mailboxId });
}

/** Reverse a recent action. Each undo id works once, for a few minutes. */
export function applyUndo(id: number): Promise<{ undone: number }> {
  return invoke("undo_apply", { id });
}

export interface MarkResult {
  changed: number;
  undo: UndoInfo | null;
}

/** Pin or unpin (kept on this device); the result can be undone like a move. */
export function pinMessages(ids: number[], pinned: boolean): Promise<MarkResult> {
  return invoke<MarkResult>("message_pin", { ids, pinned });
}

/** Set aside (out of the inbox) or bring back; undoable. */
export function setAside(ids: number[], aside: boolean): Promise<MarkResult> {
  return invoke<MarkResult>("message_set_aside", { ids, aside });
}

/**
 * "Remind me if no reply by" `at` (Unix ms, in the future), or no reminder
 * (`null`, also done with one that came due); undoable. If nobody else has
 * written in the conversation by then, the message comes back on top of the
 * inbox, from wherever it is filed (Sent, usually).
 */
export function remindMessages(ids: number[], at: number | null): Promise<MarkResult> {
  return invoke<MarkResult>("message_remind", { ids, at });
}

export type MessageView = "pinned" | "aside" | "snoozed" | "reminders";

/** An account's pinned, set-aside, snoozed or reminder messages, newest first; pass `next` back for more. */
export function listView(
  accountId: string,
  view: MessageView,
  before: MessageCursor | null,
  limit: number | null = null,
): Promise<MessageListResult> {
  return invoke<MessageListResult>("message_view", { accountId, view, before, limit });
}
