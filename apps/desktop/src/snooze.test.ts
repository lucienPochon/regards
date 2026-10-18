import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const { presets } = await import("./snooze");

const local = (until: number | "someday") => {
  if (until === "someday") return "someday";
  const d = new Date(until);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
};

describe("snooze presets", () => {
  it("offers later today, the evening, tomorrow, the weekend, next week and someday on a weekday morning", () => {
    // Wednesday 21 October 2026, 09:10.
    const shown = presets(new Date(2026, 9, 21, 9, 10)).map((p) => `${p.label} ${local(p.until)}`);
    expect(shown).toEqual([
      "Later today 2026-10-21 12:15",
      "This evening 2026-10-21 18:00",
      "Tomorrow 2026-10-22 08:00",
      "This weekend 2026-10-24 09:00",
      "Next week 2026-10-26 08:00",
      "Someday someday",
    ]);
  });

  it("drops what has passed late in the day and on weekends", () => {
    // Saturday 24 October 2026, 21:00.
    const labels = presets(new Date(2026, 9, 24, 21, 0)).map((p) => p.label);
    expect(labels).toEqual(["Tomorrow", "Next week", "Someday"]);
  });

  it("does not offer Monday morning twice on a Sunday", () => {
    // Sunday 18 October 2026, 23:20.
    const shown = presets(new Date(2026, 9, 18, 23, 20)).map((p) => `${p.label} ${local(p.until)}`);
    expect(shown).toEqual(["Tomorrow 2026-10-19 08:00", "Someday someday"]);
  });

  it("keeps wall-clock times across a daylight saving change", () => {
    // In Europe, clocks go back on Sunday 25 October 2026 at 03:00. Tests run
    // with TZ=Europe/Zurich (see vitest.config.ts), so this crosses the change.
    const saturday = presets(new Date(2026, 9, 24, 10, 0));
    const tomorrow = saturday.find((p) => p.label === "Tomorrow");
    expect(local(tomorrow!.until)).toBe("2026-10-25 08:00");
    const nextWeek = saturday.find((p) => p.label === "Next week");
    expect(local(nextWeek!.until)).toBe("2026-10-26 08:00");
    // 25 hours separate Saturday 08:00 from Sunday 08:00 that night.
    const sameTimeSaturday = new Date(2026, 9, 24, 8, 0).getTime();
    expect((tomorrow!.until as number) - sameTimeSaturday).toBe(25 * 3600_000);
  });
});
