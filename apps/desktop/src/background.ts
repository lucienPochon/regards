import { invoke } from "@tauri-apps/api/core";

/**
 * Background mode (on by default, ADR-0017 §1): Regards starts at login and
 * stays in the tray when its window closes, so snoozed mail comes back and
 * scheduled messages go while the computer is on. Off, closing the window
 * quits Regards and stops scheduled work.
 */
export const backgroundMode = () => invoke<boolean>("background_get");
export const setBackgroundMode = (on: boolean) => invoke("background_set", { on });

/**
 * Notifications (on by default): a snooze ended, a reminder came due, a
 * scheduled message waits in the Outbox, a message was not sent. Never the
 * message's text; not while Regards' window has the focus.
 */
export const notificationsShown = () => invoke<boolean>("notifications_get");
export const setNotificationsShown = (on: boolean) => invoke("notifications_set", { on });
