import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** A later version than the running one (mirrors `Update` in `src-tauri/src/updates.rs`). */
export interface Update {
  version: string;
  /** Its release page, where the installer is downloaded (`openUpdate`). */
  url: string;
  published_at: string | null;
}

export interface UpdateStatus {
  /**
   * Whether Regards checks for updates once a day. `null`: the person was
   * not asked yet. Ask once (a check tells GitHub this computer's address,
   * so it stays off until they say yes; see docs/privacy.md).
   */
  checks: boolean | null;
  /** The running version. */
  current: string;
  /** Found by the last check, this run. */
  available: Update | null;
  /** Unix ms of the last check, this run. */
  checked_at: number | null;
}

export const updateStatus = () => invoke<UpdateStatus>("update_status");

/** The person's answer: check once a day, or never. */
export const setUpdateChecks = (on: boolean) => invoke<void>("update_set_checks", { on });

/** Check now, as the person asked, whatever the setting; rejects with the reason if it could not. */
export const checkForUpdate = () => invoke<Update | null>("update_check");

/** Open the found release's page in the browser, for the download. */
export const openUpdate = () => invoke<void>("update_open");

/** A daily check found a version the person was not told of yet (also notified once). */
export const onUpdateAvailable = (handler: (update: Update) => void): Promise<UnlistenFn> =>
  listen<Update>("update_available", (event) => handler(event.payload));
