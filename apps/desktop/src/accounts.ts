import { invoke } from "@tauri-apps/api/core";

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
