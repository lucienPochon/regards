import { invoke } from "@tauri-apps/api/core";
import type { Person } from "./accounts";
import { appLocale, t } from "./i18n";

/** A time as an invitation writes it (the daemon passes it as is). */
export interface CalendarTime {
  /** `20261030T100000`, or `20261030` for a whole day. */
  value: string;
  /** An IANA name (`Europe/Zurich`) or a Windows one (`W. Europe Standard Time`), when not UTC. */
  zone: string | null;
  utc: boolean;
  all_day: boolean;
}

export type InvitationAnswer = "accepted" | "tentative" | "declined";

/** What an invitation in a message asks (iMIP). */
export interface Invitation {
  /** "request" (to answer), "cancel", or another method. */
  method: string;
  summary: string;
  location: string | null;
  start: CalendarTime | null;
  end: CalendarTime | null;
  organizer: Person | null;
  attendees: number;
  repeats: boolean;
  /** The person's address it was sent to. */
  me: string | null;
  /** How the invitation says the person stands ("needs-action", "accepted"…). */
  status: string | null;
  /** What the person answered in Regards to this version of it. */
  answered: InvitationAnswer | null;
}

/** The invitation a message carries, or null. */
export const getInvitation = (id: number) => invoke<Invitation | null>("invitation_get", { id });

/** Answer it: a reply to its organizer goes at once. */
export const answerInvitation = (messageId: number, answer: InvitationAnswer) =>
  invoke<Invitation>("invitation_respond", { messageId, answer });

/** Windows zone names (Outlook's) for the IANA ones `Intl` knows. */
const WINDOWS_ZONES: Record<string, string> = {
  "W. Europe Standard Time": "Europe/Berlin",
  "Romance Standard Time": "Europe/Paris",
  "Central Europe Standard Time": "Europe/Budapest",
  "Central European Standard Time": "Europe/Warsaw",
  "GMT Standard Time": "Europe/London",
  "Greenwich Standard Time": "Atlantic/Reykjavik",
  "E. Europe Standard Time": "Europe/Chisinau",
  "FLE Standard Time": "Europe/Kyiv",
  "GTB Standard Time": "Europe/Bucharest",
  "Russian Standard Time": "Europe/Moscow",
  "Eastern Standard Time": "America/New_York",
  "Central Standard Time": "America/Chicago",
  "Mountain Standard Time": "America/Denver",
  "Pacific Standard Time": "America/Los_Angeles",
  "India Standard Time": "Asia/Kolkata",
  "China Standard Time": "Asia/Shanghai",
  "Tokyo Standard Time": "Asia/Tokyo",
  "AUS Eastern Standard Time": "Australia/Sydney",
  UTC: "UTC",
};

/** `zone` as one `Intl` knows, or null. */
function knownZone(zone: string): string | null {
  for (const name of [zone, WINDOWS_ZONES[zone]]) {
    if (!name) continue;
    try {
      new Intl.DateTimeFormat("en-US", { timeZone: name });
      return name;
    } catch {
      // Not one it knows.
    }
  }
  return null;
}

/** How far `zone` is from UTC at instant `at` (ms), in ms. */
function offsetAt(at: number, zone: string): number {
  const parts = new Intl.DateTimeFormat("en-US", {
    timeZone: zone,
    hourCycle: "h23",
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  }).formatToParts(new Date(at));
  const part = (type: string) => Number(parts.find((p) => p.type === type)?.value ?? 0);
  const wall = Date.UTC(part("year"), part("month") - 1, part("day"), part("hour") % 24, part("minute"), part("second"));
  return wall - Math.floor(at / 1000) * 1000;
}

/**
 * The instant a time names (Unix ms), and whether it could be placed for
 * sure: a zone `Intl` does not know is read as local time. A whole day is
 * its local midnight.
 */
export function instantOf(time: CalendarTime): { at: number; sure: boolean } | null {
  const match = /^(\d{4})(\d{2})(\d{2})(?:T(\d{2})(\d{2})(\d{2}))?$/.exec(time.value);
  if (!match) return null;
  const [year, month, day, hour, minute, second] = match.slice(1).map((n) => Number(n ?? 0));
  if (time.all_day) return { at: new Date(year, month - 1, day).getTime(), sure: true };
  const wall = Date.UTC(year, month - 1, day, hour, minute, second);
  if (time.utc) return { at: wall, sure: true };
  const zone = time.zone ? knownZone(time.zone) : null;
  if (!zone) return { at: new Date(year, month - 1, day, hour, minute, second).getTime(), sure: time.zone === null };
  // The instant whose wall time there is `wall`: twice, for a change of offset between.
  const first = wall - offsetAt(wall, zone);
  return { at: wall - offsetAt(first, zone), sure: true };
}

/** "Friday 30 October, 10:00–11:00", in local time; whole days as "Thursday 24 December, all day". */
export function whenText(start: CalendarTime | null, end: CalendarTime | null, now = new Date()): string {
  const from = start ? instantOf(start) : null;
  if (!start || !from) return t("Time not given");
  const day = (at: number) => {
    const date = new Date(at);
    return date.toLocaleDateString(appLocale(), {
      weekday: "long",
      day: "numeric",
      month: "long",
      year: date.getFullYear() === now.getFullYear() ? undefined : "numeric",
    });
  };
  const time = (at: number) => new Date(at).toLocaleTimeString(appLocale(), { hour: "2-digit", minute: "2-digit" });
  if (start.all_day) return t("{day}, all day", { day: day(from.at) });
  const until = end ? instantOf(end) : null;
  const zone = from.sure ? "" : ` (${start.zone})`;
  if (!until) return `${day(from.at)}, ${time(from.at)}${zone}`;
  const sameDay = new Date(from.at).toDateString() === new Date(until.at).toDateString();
  return sameDay
    ? `${day(from.at)}, ${time(from.at)}–${time(until.at)}${zone}`
    : `${day(from.at)}, ${time(from.at)} – ${day(until.at)}, ${time(until.at)}${zone}`;
}
