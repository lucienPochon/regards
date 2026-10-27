import { invoke } from "@tauri-apps/api/core";
import type { MarkResult } from "./accounts";
import { appLocale, t } from "./i18n";
import type { MenuItem } from "./Menu";

/**
 * Snooze until `until` (Unix ms), "someday" (`until: "someday"`), or end a
 * snooze (`null`). Snoozed messages leave the inbox and come back unread at
 * that time, or sooner if someone replies; `quiet`: without a notification.
 * Undoable like a move.
 */
export function snoozeMessages(ids: number[], until: number | "someday" | null, quiet = false): Promise<MarkResult> {
  return invoke<MarkResult>("message_snooze", {
    ids,
    until: typeof until === "number" ? until : null,
    someday: until === "someday",
    quiet,
  });
}

/** A switch under a time picker's choices, flipped in place; read once a time is picked. */
export interface TimeOption {
  label: string;
  on: boolean;
}

/** A snooze's alert when it comes back: on, unless switched off in the picker. */
export const snoozeAlert = (): TimeOption => ({ label: t("Alert me when it comes back"), on: true });

/** The switch as a menu row: picking it flips it and shows the menu again (`show`). */
export const timeOptionEntry = (option: TimeOption, show: () => void): MenuItem => ({
  label: option.label,
  icon: "bell",
  checked: option.on,
  onSelect: () => {
    option.on = !option.on;
    show();
  },
});

export interface Preset {
  label: string;
  /** Unix ms, or "someday". */
  until: number | "someday";
}

/** A quick choice of a time picker, as Settings › Scheduling lists it. */
export interface PresetDef {
  id: string;
  label: string;
  /** Its time of day, "HH:MM", when it has one (not "Later today" or "Someday"). */
  time?: string;
  /** When it is offered, for Settings. */
  when: string;
}

/** The person's choices for a picker's presets: hidden, or at another time. */
export type PresetPrefs = Partial<Record<string, { on?: boolean; time?: string }>>;

export const SNOOZE_PRESETS: PresetDef[] = [
  { id: "later", label: t("Later today"), when: t("In about three hours, while it is still today") },
  { id: "evening", label: t("This evening"), time: "18:00", when: t("Until an hour before") },
  { id: "tomorrow", label: t("Tomorrow"), time: "08:00", when: t("Every day") },
  { id: "weekend", label: t("This weekend"), time: "09:00", when: t("Saturday, offered Monday to Thursday") },
  { id: "nextweek", label: t("Next week"), time: "08:00", when: t("Monday, unless tomorrow is Monday") },
  { id: "someday", label: t("Someday"), when: t("No time: it waits in Snoozed until you bring it back") },
];

export const SEND_PRESETS: PresetDef[] = [
  { id: "later", label: t("Later today"), when: t("In about three hours, while it is still today") },
  { id: "evening", label: t("This evening"), time: "18:00", when: t("Until an hour before") },
  { id: "tomorrow", label: t("Tomorrow morning"), time: "08:00", when: t("Every day") },
  { id: "monday", label: t("Monday morning"), time: "08:00", when: t("Unless tomorrow is Monday") },
];

export const REMINDER_PRESETS: PresetDef[] = [
  { id: "tomorrow", label: t("Tomorrow"), time: "08:00", when: t("A day after") },
  { id: "days2", label: t("In 2 days"), time: "08:00", when: t("Two days after") },
  { id: "days3", label: t("In 3 days"), time: "08:00", when: t("Three days after") },
  { id: "week", label: t("In a week"), time: "08:00", when: t("A week after") },
];

/** One picker's presets with the person's choices: whether each shows, and its time. */
function chosen(defs: PresetDef[], prefs: PresetPrefs) {
  const on = (id: string) => prefs[id]?.on !== false;
  const time = (id: string) => {
    const value = prefs[id]?.time ?? defs.find((def) => def.id === id)?.time ?? "08:00";
    const [hour, minute] = value.split(":").map(Number);
    return { hour: hour || 0, minute: minute || 0 };
  };
  const label = (id: string) => defs.find((def) => def.id === id)?.label ?? id;
  return { on, time, label };
}

/** A time a number of days from now at a time of day, from calendar dates (so 08:00 stays 08:00 across DST). */
const dayAt = (now: Date, days: number, at: { hour: number; minute: number }) =>
  new Date(now.getFullYear(), now.getMonth(), now.getDate() + days, at.hour, at.minute, 0, 0).getTime();

/** Three hours on, to the next quarter hour, if that is still today before `lastHour`. */
function laterToday(now: Date, lastHour: number): number | null {
  const later = new Date(now.getTime() + 3 * 3600_000);
  later.setMinutes(Math.ceil(later.getMinutes() / 15) * 15, 0, 0);
  return later.getDate() === now.getDate() && later.getHours() < lastHour ? later.getTime() : null;
}

/**
 * The usual choices for snoozing, in local time, as the person set them in
 * Settings › Scheduling. Times are built from calendar dates and hours, not
 * by adding hours, so "tomorrow at 8" stays 8:00 across a daylight saving change.
 */
export function presets(now: Date = new Date(), prefs: PresetPrefs = {}): Preset[] {
  const { on, time, label } = chosen(SNOOZE_PRESETS, prefs);
  const list: Preset[] = [];
  const later = laterToday(now, 22);
  if (on("later") && later !== null) list.push({ label: label("later"), until: later });
  const evening = dayAt(now, 0, time("evening"));
  if (on("evening") && now.getTime() < evening - 3600_000) list.push({ label: label("evening"), until: evening });
  if (on("tomorrow")) list.push({ label: label("tomorrow"), until: dayAt(now, 1, time("tomorrow")) });
  const day = now.getDay(); // 0 Sunday … 6 Saturday
  if (on("weekend") && day >= 1 && day <= 4) list.push({ label: label("weekend"), until: dayAt(now, 6 - day, time("weekend")) });
  // On a Sunday, next Monday morning is already "Tomorrow".
  const toMonday = ((8 - day) % 7) || 7;
  if (on("nextweek") && toMonday > 1) list.push({ label: label("nextweek"), until: dayAt(now, toMonday, time("nextweek")) });
  if (on("someday")) list.push({ label: label("someday"), until: "someday" });
  return list;
}

/** When to send later, as set in Settings › Scheduling; built from calendar days and hours. */
export function sendPresets(now: Date = new Date(), prefs: PresetPrefs = {}): { label: string; until: number }[] {
  const { on, time, label } = chosen(SEND_PRESETS, prefs);
  const list: { label: string; until: number }[] = [];
  const later = laterToday(now, 21);
  if (on("later") && later !== null) list.push({ label: label("later"), until: later });
  const evening = dayAt(now, 0, time("evening"));
  if (on("evening") && now.getTime() < evening - 3600_000) list.push({ label: label("evening"), until: evening });
  if (on("tomorrow")) list.push({ label: label("tomorrow"), until: dayAt(now, 1, time("tomorrow")) });
  const toMonday = ((8 - now.getDay()) % 7) || 7;
  if (on("monday") && toMonday > 1) list.push({ label: label("monday"), until: dayAt(now, toMonday, time("monday")) });
  return list;
}

/**
 * When to be reminded if nobody has replied, as set in Settings ›
 * Scheduling: a day to a week away. Built from calendar dates, so 08:00
 * stays 08:00 across DST.
 */
export function reminderPresets(now: Date = new Date(), prefs: PresetPrefs = {}): Preset[] {
  const { on, time, label } = chosen(REMINDER_PRESETS, prefs);
  const days: Record<string, number> = { tomorrow: 1, days2: 2, days3: 3, week: 7 };
  return REMINDER_PRESETS.filter((def) => on(def.id)).map((def) => ({
    label: label(def.id),
    until: dayAt(now, days[def.id], time(def.id)),
  }));
}

/** A time beside its preset: "18:00", or "Thu 08:00" when it is not today. */
export function timeHint(until: number | "someday"): string | undefined {
  if (until === "someday") return undefined;
  const date = new Date(until);
  const time = date.toLocaleTimeString(appLocale(), { hour: "2-digit", minute: "2-digit" });
  return date.toDateString() === new Date().toDateString()
    ? time
    : `${date.toLocaleDateString(appLocale(), { weekday: "short" })} ${time}`;
}
