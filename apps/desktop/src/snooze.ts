import { invoke } from "@tauri-apps/api/core";
import type { MarkResult } from "./accounts";

/**
 * Snooze until `until` (Unix ms), "someday" (`until: "someday"`), or end a
 * snooze (`null`). Snoozed messages leave the inbox and come back unread at
 * that time, or sooner if someone replies. Undoable like a move.
 */
export function snoozeMessages(ids: number[], until: number | "someday" | null): Promise<MarkResult> {
  return invoke<MarkResult>("message_snooze", {
    ids,
    until: typeof until === "number" ? until : null,
    someday: until === "someday",
  });
}

export interface Preset {
  label: string;
  /** Unix ms, or "someday". */
  until: number | "someday";
}

/** When the working day starts and the evening begins, local time. */
const MORNING = 8;
const EVENING = 18;

/**
 * The usual choices, in local time. Times are built from calendar dates and
 * hours, not by adding hours, so "tomorrow at 8" stays 8:00 across a daylight
 * saving change.
 */
export function presets(now: Date = new Date()): Preset[] {
  const at = (days: number, hour: number) => {
    const date = new Date(now.getFullYear(), now.getMonth(), now.getDate() + days, hour, 0, 0, 0);
    return date.getTime();
  };
  const list: Preset[] = [];
  // Three hours on, to the next quarter hour, if that is still today.
  const later = new Date(now.getTime() + 3 * 3600_000);
  later.setMinutes(Math.ceil(later.getMinutes() / 15) * 15, 0, 0);
  if (later.getDate() === now.getDate() && later.getHours() < 22) {
    list.push({ label: "Later today", until: later.getTime() });
  }
  if (now.getHours() < EVENING - 1) {
    list.push({ label: "This evening", until: at(0, EVENING) });
  }
  list.push({ label: "Tomorrow", until: at(1, MORNING) });
  const day = now.getDay(); // 0 Sunday … 6 Saturday
  if (day >= 1 && day <= 4) {
    list.push({ label: "This weekend", until: at(6 - day, MORNING + 1) });
  }
  // On a Sunday, next Monday morning is already "Tomorrow".
  const toMonday = ((8 - day) % 7) || 7;
  if (toMonday > 1) list.push({ label: "Next week", until: at(toMonday, MORNING) });
  list.push({ label: "Someday", until: "someday" });
  return list;
}

/**
 * When to be reminded if nobody has replied: mornings, a day to a week
 * away. Built from calendar dates, so 08:00 stays 08:00 across DST.
 */
export function reminderPresets(now: Date = new Date()): Preset[] {
  const at = (days: number) =>
    new Date(now.getFullYear(), now.getMonth(), now.getDate() + days, MORNING, 0, 0, 0).getTime();
  return [
    { label: "Tomorrow", until: at(1) },
    { label: "In 2 days", until: at(2) },
    { label: "In 3 days", until: at(3) },
    { label: "In a week", until: at(7) },
  ];
}
