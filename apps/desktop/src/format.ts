import type { Person } from "./accounts";
import { appLocale, t } from "./i18n";

/** A person as the list shows them: the name, else the address. */
export function personName(person: Person | null | undefined): string {
  return person?.name?.trim() || person?.address || t("(unknown sender)");
}

/** "Ada, Bob and 2 more", for a line that has little room. */
export function peopleShort(people: Person[], limit = 2): string {
  if (people.length === 0) return t("nobody");
  const names = people.slice(0, limit).map((person) => personName(person));
  const rest = people.length - names.length;
  return rest > 0 ? t("{names} and {count} more", { names: names.join(", "), count: rest }) : names.join(", ");
}

/** Everyone in full, with addresses: `Ada Lovelace <ada@example.org>, …`. */
export function peopleFull(people: Person[]): string {
  return people
    .map((p) => (p.name && p.address ? `${p.name} <${p.address}>` : (p.name ?? p.address ?? "")))
    .join(", ");
}

const DAY_MS = 24 * 60 * 60 * 1000;

/** The date in a list: the time today, the weekday this week, the day this year, else the full date. */
/** A time still to come, short: "18:00", "Tomorrow 08:00", "Sat 09:00", "2 Nov". */
export function laterDate(ms: number, now = Date.now()): string {
  const date = new Date(ms);
  const today = new Date(now);
  const time = date.toLocaleTimeString(appLocale(), { hour: "2-digit", minute: "2-digit" });
  if (date.toDateString() === today.toDateString()) return time;
  const tomorrow = new Date(today.getFullYear(), today.getMonth(), today.getDate() + 1);
  if (date.toDateString() === tomorrow.toDateString()) return t("Tomorrow {time}", { time });
  if (ms < tomorrow.getTime() + 6 * DAY_MS) return `${date.toLocaleDateString(appLocale(), { weekday: "short" })} ${time}`;
  if (date.getFullYear() === today.getFullYear()) {
    return date.toLocaleDateString(appLocale(), { day: "numeric", month: "short" });
  }
  return date.toLocaleDateString(appLocale(), { day: "numeric", month: "short", year: "numeric" });
}

export function listDate(ms: number, now = Date.now()): string {
  const date = new Date(ms);
  const today = new Date(now);
  if (date.toDateString() === today.toDateString()) {
    return date.toLocaleTimeString(appLocale(), { hour: "2-digit", minute: "2-digit" });
  }
  const startOfToday = new Date(today.getFullYear(), today.getMonth(), today.getDate()).getTime();
  if (ms >= startOfToday - DAY_MS) return t("Yesterday");
  if (ms >= startOfToday - 6 * DAY_MS) return date.toLocaleDateString(appLocale(), { weekday: "short" });
  if (date.getFullYear() === today.getFullYear()) {
    return date.toLocaleDateString(appLocale(), { day: "numeric", month: "short" });
  }
  return date.toLocaleDateString(appLocale(), { day: "numeric", month: "short", year: "numeric" });
}

/** The date in a message header: weekday, day, month and time. */
export function fullDate(ms: number): string {
  return new Date(ms).toLocaleString(appLocale(), {
    weekday: "short",
    day: "numeric",
    month: "short",
    year: new Date(ms).getFullYear() === new Date().getFullYear() ? undefined : "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

/** The heading a message sits under in a list: Today, Yesterday, This week, Last week, then by month. */
export function dateSection(ms: number, now = Date.now()): string {
  const today = new Date(now);
  const start = new Date(today.getFullYear(), today.getMonth(), today.getDate()).getTime();
  if (ms >= start) return t("Today");
  if (ms >= start - DAY_MS) return t("Yesterday");
  // Weeks start on Monday.
  const weekStart = start - ((today.getDay() + 6) % 7) * DAY_MS;
  if (ms >= weekStart) return t("This week");
  if (ms >= weekStart - 7 * DAY_MS) return t("Last week");
  const date = new Date(ms);
  const month = date.toLocaleDateString(appLocale(), {
    month: "long",
    year: date.getFullYear() === today.getFullYear() ? undefined : "numeric",
  });
  return month.charAt(0).toUpperCase() + month.slice(1);
}

export function fileSize(bytes: number): string {
  if (bytes < 1024) return t("{size} B", { size: bytes });
  if (bytes < 1024 * 1024) return t("{size} KB", { size: Math.round(bytes / 1024) });
  const megabytes = (bytes / 1024 / 1024).toLocaleString(appLocale(), { minimumFractionDigits: 1, maximumFractionDigits: 1 });
  return t("{size} MB", { size: megabytes });
}

/** The file type to show on an attachment tile: the extension, else the MIME subtype. */
export function fileKind(name: string | null, mime: string): string {
  const extension = name?.match(/\.([a-z0-9]{1,5})$/i)?.[1];
  const kind = extension ?? mime.split("/")[1]?.split(/[.+-]/)[0] ?? "file";
  return kind.slice(0, 4).toUpperCase();
}
