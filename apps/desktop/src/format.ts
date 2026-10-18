import type { Person } from "./accounts";

/** A person as the list shows them: the name, else the address. */
export function personName(person: Person | null | undefined): string {
  return person?.name?.trim() || person?.address || "(unknown sender)";
}

/** "Ada, Bob and 2 more", for a line that has little room. */
export function peopleShort(people: Person[], limit = 2): string {
  if (people.length === 0) return "nobody";
  const names = people.slice(0, limit).map((person) => personName(person));
  const rest = people.length - names.length;
  return rest > 0 ? `${names.join(", ")} and ${rest} more` : names.join(", ");
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
  const time = date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  if (date.toDateString() === today.toDateString()) return time;
  const tomorrow = new Date(today.getFullYear(), today.getMonth(), today.getDate() + 1);
  if (date.toDateString() === tomorrow.toDateString()) return `Tomorrow ${time}`;
  if (ms < tomorrow.getTime() + 6 * DAY_MS) return `${date.toLocaleDateString([], { weekday: "short" })} ${time}`;
  if (date.getFullYear() === today.getFullYear()) {
    return date.toLocaleDateString([], { day: "numeric", month: "short" });
  }
  return date.toLocaleDateString([], { day: "numeric", month: "short", year: "numeric" });
}

export function listDate(ms: number, now = Date.now()): string {
  const date = new Date(ms);
  const today = new Date(now);
  if (date.toDateString() === today.toDateString()) {
    return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  }
  const startOfToday = new Date(today.getFullYear(), today.getMonth(), today.getDate()).getTime();
  if (ms >= startOfToday - DAY_MS) return "Yesterday";
  if (ms >= startOfToday - 6 * DAY_MS) return date.toLocaleDateString([], { weekday: "short" });
  if (date.getFullYear() === today.getFullYear()) {
    return date.toLocaleDateString([], { day: "numeric", month: "short" });
  }
  return date.toLocaleDateString([], { day: "numeric", month: "short", year: "numeric" });
}

/** The date in a message header: weekday, day, month and time. */
export function fullDate(ms: number): string {
  return new Date(ms).toLocaleString([], {
    weekday: "short",
    day: "numeric",
    month: "short",
    year: new Date(ms).getFullYear() === new Date().getFullYear() ? undefined : "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function fileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** The file type to show on an attachment tile: the extension, else the MIME subtype. */
export function fileKind(name: string | null, mime: string): string {
  const extension = name?.match(/\.([a-z0-9]{1,5})$/i)?.[1];
  const kind = extension ?? mime.split("/")[1]?.split(/[.+-]/)[0] ?? "file";
  return kind.slice(0, 4).toUpperCase();
}
