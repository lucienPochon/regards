import type { MenuItem } from "./Menu";
import { appLocale, t } from "./i18n";

/**
 * A time typed into a time picker, in local time: "in 3 days", "2h",
 * "tomorrow 3pm", "fri 8:30", "next week", "oct 30 9am", "30.10 14:00",
 * "8 pm", "noon", "tonight". Days are counted on the calendar and hours set
 * on them, so 8:00 stays 8:00 across a daylight saving change. Only a time
 * still to come counts: anything else, or anything not understood, is null.
 */
export function parseTypedTime(text: string, now: Date = new Date(), locale: string = navigator.language): number | null {
  const words = text.toLowerCase().replace(/,/g, " ").trim();
  if (!words) return null;
  const said: Said = {};
  let rest = words;
  while (rest) {
    const step = READERS.reduce<number | null>((found, read) => found ?? read(rest, said, locale), null);
    if (step === null) return null;
    rest = rest.slice(step).trimStart();
  }
  return when(said, now);
}

/** What the words said, as they were read. */
interface Said {
  /** Minutes or hours from now, to the minute. */
  exact?: number;
  days?: number;
  months?: number;
  weekday?: number;
  next?: boolean;
  nextWeek?: boolean;
  nextMonth?: boolean;
  weekend?: boolean;
  date?: { year?: number; month: number; day: number };
  time?: { hour: number; minute: number };
  /** The time had no am or pm and could be either. */
  twelve?: boolean;
}

/** When a day is given without a time, as the presets' mornings (Settings › Scheduling). */
const MORNING = { hour: 8, minute: 0 };
const WEEKEND = { hour: 9, minute: 0 };
const PARTS: Record<string, { hour: number; minute: number }> = {
  morning: MORNING,
  noon: { hour: 12, minute: 0 },
  midday: { hour: 12, minute: 0 },
  afternoon: { hour: 14, minute: 0 },
  evening: { hour: 18, minute: 0 },
  tonight: { hour: 20, minute: 0 },
  night: { hour: 20, minute: 0 },
};
const WEEKDAYS = ["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"];
const MONTHS = ["january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december"];
const NUMBERS: Record<string, number> = { a: 1, an: 1, one: 1, two: 2, three: 3, four: 4, five: 5, six: 6, seven: 7, eight: 8, nine: 9, ten: 10 };

const weekday = (word: string) => WEEKDAYS.findIndex((name) => word.length >= 3 && name.startsWith(word));
const month = (word: string) => MONTHS.findIndex((name) => word.length >= 3 && name.startsWith(word));
const count = (word: string) => NUMBERS[word] ?? Number(word);

/** Each reads one piece at the start of the words: how many characters it took, or null. */
type Reader = (rest: string, said: Said, locale: string) => number | null;

const READERS: Reader[] = [
  // Words that change nothing.
  (rest) => /^(in|at|on|the|this|for|until|till)\b/.exec(rest)?.[0].length ?? null,
  (rest, said) => {
    const match = /^next\b/.exec(rest);
    if (!match) return null;
    said.next = true;
    return match[0].length;
  },
  // "3 days", "2h", "an hour", "30 min".
  (rest, said) => {
    const match = /^(\d{1,3}|an?|one|two|three|four|five|six|seven|eight|nine|ten)\s*(minutes?|mins?|m|hours?|hrs?|h|days?|d|weeks?|wks?|w|months?)\b/.exec(rest);
    if (!match) return null;
    const amount = count(match[1]);
    const unit = match[2][0] === "m" && match[2].startsWith("mo") ? "month" : match[2][0];
    if (unit === "m") said.exact = (said.exact ?? 0) + amount * 60_000;
    else if (unit === "h") said.exact = (said.exact ?? 0) + amount * 3600_000;
    else if (unit === "d") said.days = (said.days ?? 0) + amount;
    else if (unit === "w") said.days = (said.days ?? 0) + amount * 7;
    else said.months = (said.months ?? 0) + amount;
    return match[0].length;
  },
  (rest, said) => {
    const match = /^(today|tod|tomorrow|tmrw|tmr|tom)\b/.exec(rest);
    if (!match) return null;
    said.days = (said.days ?? 0) + (match[1].startsWith("tod") ? 0 : 1);
    return match[0].length;
  },
  (rest, said) => {
    const match = /^(weekend|week|month)\b/.exec(rest);
    if (!match) return null;
    if (match[1] === "weekend") said.weekend = true;
    else if (!said.next) return null;
    else if (match[1] === "week") said.nextWeek = true;
    else said.nextMonth = true;
    return match[0].length;
  },
  (rest, said) => {
    const match = /^(morning|noon|midday|afternoon|evening|tonight|night)\b/.exec(rest);
    if (!match || said.time) return null;
    said.time = PARTS[match[1]];
    return match[0].length;
  },
  // "2026-11-02".
  (rest, said) => {
    const match = /^(\d{4})-(\d{1,2})-(\d{1,2})\b/.exec(rest);
    if (!match || said.date) return null;
    said.date = { year: Number(match[1]), month: Number(match[2]) - 1, day: Number(match[3]) };
    return match[0].length;
  },
  // "30.10", "30.10.2026", and "30/10" (10/30 where the month comes first).
  (rest, said, locale) => {
    const match = /^(\d{1,2})([./])(\d{1,2})(?:\2(\d{2}|\d{4}))?(?![\d:.]|\s*[ap]\.?m?\b)/.exec(rest);
    if (!match || said.date) return null;
    const monthFirst = match[2] === "/" && /^en-us\b/i.test(locale);
    const [day, monthNumber] = monthFirst ? [match[3], match[1]] : [match[1], match[3]];
    const year = match[4] ? Number(match[4].length === 2 ? `20${match[4]}` : match[4]) : undefined;
    said.date = { year, month: Number(monthNumber) - 1, day: Number(day) };
    return match[0].length;
  },
  // "oct 30", "october 30th 2026".
  (rest, said) => {
    const match = /^([a-z]{3,9})\.?\s+(\d{1,2})(?:st|nd|rd|th)?(?:\s+(\d{4}))?\b(?!\s*(?::|h\d|[ap]\.?m?\b))/.exec(rest);
    if (!match || said.date || month(match[1]) < 0) return null;
    said.date = { year: match[3] ? Number(match[3]) : undefined, month: month(match[1]), day: Number(match[2]) };
    return match[0].length;
  },
  // "30 oct", "30th of october 2026".
  (rest, said) => {
    const match = /^(\d{1,2})(?:st|nd|rd|th)?\s+(?:of\s+)?([a-z]{3,9})\b(?:\s+(\d{4})\b)?/.exec(rest);
    if (!match || said.date || month(match[2]) < 0) return null;
    said.date = { year: match[3] ? Number(match[3]) : undefined, month: month(match[2]), day: Number(match[1]) };
    return match[0].length;
  },
  (rest, said) => {
    const match = /^([a-z]{3,9})\b/.exec(rest);
    if (!match || said.weekday !== undefined || weekday(match[1]) < 0) return null;
    said.weekday = weekday(match[1]);
    return match[0].length;
  },
  // "8pm", "8:30", "20h15", "8.30 pm", "8".
  (rest, said) => {
    const match = /^(\d{1,2})(?:[:h](\d{2})|\.(\d{2})(?=\s*[ap]))?\s*(?:([ap])\.?m?\.?)?(?=\s|$)/.exec(rest);
    if (!match || said.time) return null;
    let hour = Number(match[1]);
    const minute = Number(match[2] ?? match[3] ?? 0);
    const half = match[4];
    if (minute > 59 || hour > 23 || (half && (hour < 1 || hour > 12))) return null;
    if (half === "p" && hour < 12) hour += 12;
    if (half === "a" && hour === 12) hour = 0;
    said.time = { hour, minute };
    said.twelve = !half && hour >= 1 && hour <= 11;
    return match[0].length;
  },
];

/** The moment the words name, or null if there is none still to come. */
function when(said: Said, now: Date): number | null {
  const dayGiven =
    said.days !== undefined ||
    said.months !== undefined ||
    said.weekday !== undefined ||
    said.date !== undefined ||
    said.nextWeek ||
    said.nextMonth ||
    said.weekend;
  if (said.exact !== undefined) {
    // "in 2 hours" is a moment, not a day and a time.
    return dayGiven || said.time ? null : now.getTime() + said.exact;
  }
  let year = now.getFullYear();
  let monthIndex = now.getMonth();
  let day = now.getDate();
  const today = now.getDay();
  const time = said.time ?? (said.weekend && !said.weekday ? WEEKEND : MORNING);
  const at = (date: Date) => new Date(date.getFullYear(), date.getMonth(), date.getDate(), time.hour, time.minute).getTime();
  const laterToday = said.time !== undefined && at(now) > now.getTime();

  if (said.date) {
    ({ month: monthIndex, day } = said.date);
    year = said.date.year ?? year;
    if (monthIndex < 0 || monthIndex > 11 || day < 1 || day > new Date(year, monthIndex + 1, 0).getDate()) return null;
    if (said.date.year === undefined && new Date(year, monthIndex, day, time.hour, time.minute).getTime() <= now.getTime()) {
      year += 1;
      if (day > new Date(year, monthIndex + 1, 0).getDate()) return null;
    }
  } else if (said.weekday !== undefined) {
    let ahead = (said.weekday - today + 7) % 7;
    if (ahead === 0 && (said.next || !laterToday)) ahead = 7;
    day += ahead;
  } else if (said.nextWeek) {
    day += (8 - today) % 7 || 7;
  } else if (said.weekend) {
    let ahead = (6 - today + 7) % 7;
    if (ahead === 0 && !laterToday && at(now) <= now.getTime()) ahead = 7;
    day += ahead;
  } else if (said.nextMonth) {
    monthIndex += 1;
    day = 1;
  }
  if (said.months) {
    monthIndex += said.months;
    // The 31st of a shorter month is its last day.
    day = Math.min(day, new Date(year, monthIndex + 1, 0).getDate());
  }
  day += said.days ?? 0;

  if (!dayGiven) {
    // A time alone: today, or tomorrow once it has passed. "9" at 10:00 is
    // more likely 21:00 today than 9:00 tomorrow.
    const todayAt = at(now);
    if (todayAt > now.getTime()) return todayAt;
    if (said.twelve && time.hour + 12 <= 23) {
      const evening = new Date(year, monthIndex, day, time.hour + 12, time.minute).getTime();
      if (evening > now.getTime()) return evening;
    }
    return new Date(year, monthIndex, day + 1, time.hour, time.minute).getTime();
  }
  const moment = new Date(year, monthIndex, day, time.hour, time.minute).getTime();
  return moment > now.getTime() ? moment : null;
}

/** "Friday 30 October, 20:00", with the year when it is not this one. */
export function typedTimeLabel(time: number, now: Date = new Date()): string {
  const date = new Date(time);
  return date.toLocaleString(appLocale(), {
    weekday: "long",
    day: "numeric",
    month: "long",
    year: date.getFullYear() === now.getFullYear() ? undefined : "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

/** Placeholder of a time picker's field. */
export const TYPED_TIME_HINT = t("Or type a time, like “in 3 days” or “fri 8pm”");

/** A time picker's entry for what is typed, when it names a time. */
export function typedTimeEntries(query: string, pick: (time: number) => void): MenuItem[] {
  const time = parseTypedTime(query);
  return time === null ? [] : [{ label: typedTimeLabel(time), icon: "clock", onSelect: () => pick(time) }];
}
