import { invoke } from "@tauri-apps/api/core";

/**
 * Which new mail of an account notifies: all of it; People and Priority
 * ("smart", the default: no new senders, notifications or newsletters);
 * Priority only; or none.
 */
export type NewMail = "all" | "smart" | "priority" | "none";

/** Mirrors `NotifyChoices` in `src-tauri/src/notify.rs`, kept on this computer. */
export interface NotificationSettings {
  /** Per account id; an account not listed is "smart". */
  new_mail: Record<string, NewMail>;
  /** Snoozed mail came back. */
  snooze_ended: boolean;
  /** Nobody replied by a reminder's time. */
  reminder_due: boolean;
  /** A message not sent, perhaps not sent, or held after the computer was off. */
  sending: boolean;
  /** An agent waits for you, or is sending a message. */
  agents: boolean;
}

export const notificationSettings = () => invoke<NotificationSettings>("notification_settings");

/** Saved at once. The tray's "Show notifications" stays the switch for all of them. */
export const setNotificationSettings = (settings: NotificationSettings) =>
  invoke<void>("set_notification_settings", { settings });
