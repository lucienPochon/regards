import { describe, expect, it } from "vitest";
import { parseTypedTime } from "./typedTime";

// Wednesday 21 October 2026, 10:00, local time.
const now = new Date(2026, 9, 21, 10, 0);
const at = (month: number, day: number, hour: number, minute = 0, year = 2026) => new Date(year, month - 1, day, hour, minute).getTime();
const typed = (text: string, locale = "en-GB") => parseTypedTime(text, now, locale);

describe("a typed time", () => {
  it("counts minutes and hours from now", () => {
    expect(typed("2h")).toBe(now.getTime() + 2 * 3600_000);
    expect(typed("in 30 min")).toBe(now.getTime() + 30 * 60_000);
    expect(typed("in an hour")).toBe(now.getTime() + 3600_000);
  });

  it("counts days and weeks on the calendar, in the morning unless told", () => {
    expect(typed("in 3 days")).toBe(at(10, 24, 8));
    expect(typed("3d")).toBe(at(10, 24, 8));
    expect(typed("in a week")).toBe(at(10, 28, 8));
    expect(typed("in 3 days at 5pm")).toBe(at(10, 24, 17));
    expect(typed("in 2 months")).toBe(at(12, 21, 8));
  });

  it("knows today, tomorrow and parts of the day", () => {
    expect(typed("tomorrow 3pm")).toBe(at(10, 22, 15));
    expect(typed("tomorrow")).toBe(at(10, 22, 8));
    expect(typed("noon")).toBe(at(10, 21, 12));
    expect(typed("this evening")).toBe(at(10, 21, 18));
    expect(typed("tonight")).toBe(at(10, 21, 20));
    expect(typed("tomorrow afternoon")).toBe(at(10, 22, 14));
  });

  it("takes the next of a weekday, today only for a time still ahead", () => {
    expect(typed("friday 8:30")).toBe(at(10, 23, 8, 30));
    expect(typed("fri 8pm")).toBe(at(10, 23, 20));
    expect(typed("on Friday at 20h15")).toBe(at(10, 23, 20, 15));
    expect(typed("wednesday")).toBe(at(10, 28, 8));
    expect(typed("wednesday 6pm")).toBe(at(10, 21, 18));
    expect(typed("next wednesday 6pm")).toBe(at(10, 28, 18));
    expect(typed("next week")).toBe(at(10, 26, 8));
    expect(typed("this weekend")).toBe(at(10, 24, 9));
  });

  it("puts a time alone today, or tomorrow once it has passed", () => {
    expect(typed("8 pm")).toBe(at(10, 21, 20));
    expect(typed("8.30pm")).toBe(at(10, 21, 20, 30));
    expect(typed("17:45")).toBe(at(10, 21, 17, 45));
    expect(typed("7am")).toBe(at(10, 22, 7));
    // At 10:00, "9" more likely means this evening.
    expect(typed("9")).toBe(at(10, 21, 21));
  });

  it("reads dates, day first, and the next one when this year's has passed", () => {
    expect(typed("oct 30 9am")).toBe(at(10, 30, 9));
    expect(typed("30 oct 9:00")).toBe(at(10, 30, 9));
    expect(typed("30th of October")).toBe(at(10, 30, 8));
    expect(typed("30.10 14:00")).toBe(at(10, 30, 14));
    expect(typed("2026-11-02")).toBe(at(11, 2, 8));
    expect(typed("1.1")).toBe(at(1, 1, 8, 0, 2027));
    expect(typed("3/11")).toBe(at(11, 3, 8));
    expect(typed("11/3", "en-US")).toBe(at(11, 3, 8));
  });

  it("is nothing when not understood, impossible or already past", () => {
    for (const text of ["", "hello", "25:00", "31.2", "today 7am", "2h friday", "13pm", "week", "mon tue"]) {
      expect(typed(text), text).toBeNull();
    }
  });
});
