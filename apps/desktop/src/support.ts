import { invoke } from "@tauri-apps/api/core";

/**
 * "Remove everything from this computer": every password, sign-in and key
 * from the system's credential store, the mail, drafts and Outbox (messages
 * waiting there are not sent), the settings and the logs, and starting at
 * login. Then Regards quits; its next start is a first run. Mail on the
 * servers is not touched. Ask first, naming what goes.
 */
export const eraseEverything = () => invoke<void>("erase_everything");

export interface SupportSettings {
  /** The service logs in detail; takes effect when Regards next starts. */
  detailed_logs: boolean;
  /** Connections to the mail servers are logged (commands and replies only); at once. */
  connection_log: boolean;
}

export const supportSettings = () => invoke<SupportSettings>("support_settings");
export const setDetailedLogs = (on: boolean) => invoke<void>("set_detailed_logs", { on });

/**
 * Log connections to the mail servers, or stop: at once, into
 * `logs/connections.log` and diagnostics. The log holds commands and the
 * servers' replies, never mail, passwords, tokens or addresses (it names
 * the servers). Off by default.
 */
export const setConnectionLog = (on: boolean) => invoke<void>("set_connection_log", { on });

/**
 * Save diagnostics to a file the person picks: versions, the service's
 * state, how many accounts of each kind, and Regards' own log. No mail,
 * addresses, passwords or tokens; never sent anywhere. Resolves with the
 * file's path, or `null` if the person cancelled.
 */
export const exportDiagnostics = () => invoke<string | null>("export_diagnostics");
