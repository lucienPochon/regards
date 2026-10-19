import { invoke } from "@tauri-apps/api/core";

/**
 * Background mode (on by default, ADR-0017 §1): Regards starts at login and
 * stays in the tray when its window closes, so snoozed mail comes back and
 * scheduled messages go while the computer is on. Off, closing the window
 * quits Regards and stops scheduled work.
 */
export const backgroundMode = () => invoke<boolean>("background_get");
export const setBackgroundMode = (on: boolean) => invoke("background_set", { on });

/** Background mode's two halves, each its own setting (both on by default). */
export interface BackgroundSettings {
  start_at_login: boolean;
  /** Closing the window leaves Regards in the tray; off, it quits Regards. */
  close_to_tray: boolean;
}

export const backgroundSettings = () => invoke<BackgroundSettings>("background_settings");
export const setStartAtLogin = (on: boolean) => invoke<void>("set_start_at_login", { on });
export const setCloseToTray = (on: boolean) => invoke<void>("set_close_to_tray", { on });

/** A pane of the system's own settings. On macOS the default mail app is chosen in Mail, and this rejects. */
export type SystemPane = "notifications" | "default_apps";
export const openSystemSettings = (pane: SystemPane) => invoke<void>("open_system_settings", { pane });

/** An `https://` page in the system's browser (help, release pages). */
export const openUrl = (url: string) => invoke<void>("open_url", { url });

/**
 * Notifications (on by default): a snooze ended, a reminder came due, a
 * scheduled message waits in the Outbox, a message was not sent. Never the
 * message's text; not while Regards' window has the focus.
 */
export const notificationsShown = () => invoke<boolean>("notifications_get");
export const setNotificationsShown = (on: boolean) => invoke("notifications_set", { on });
