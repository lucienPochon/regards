import { invoke } from "@tauri-apps/api/core";

/**
 * Background mode (on by default, ADR-0017 §1): Regards starts at login and
 * stays in the tray when its window closes, so snoozed mail comes back and
 * scheduled messages go while the computer is on. Off, closing the window
 * quits Regards and stops scheduled work.
 */
export const backgroundMode = () => invoke<boolean>("background_get");
export const setBackgroundMode = (on: boolean) => invoke("background_set", { on });
