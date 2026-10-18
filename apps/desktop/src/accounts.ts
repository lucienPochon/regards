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
