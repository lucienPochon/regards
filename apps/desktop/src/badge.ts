import { invoke } from "@tauri-apps/api/core";

/**
 * The unread count on the taskbar or dock: the inboxes' unread mail (not
 * what is set aside or snoozed). On Windows, drawn over the icon, up to
 * 9+. On by default, for every account.
 */
export interface BadgeSettings {
  on: boolean;
  /** The accounts counted; `null`: every one. */
  accounts: string[] | null;
}

export const badgeSettings = () => invoke<BadgeSettings>("badge_settings");
/** Saved at once; the badge follows. */
export const setBadgeSettings = (settings: BadgeSettings) => invoke<void>("set_badge_settings", { settings });
