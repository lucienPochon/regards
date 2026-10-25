import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const { instantOf, whenText } = await import("./invitations");

const at = (value: string, zone: string | null, utc = false, all_day = false) => ({ value, zone, utc, all_day });

describe("an invitation's time", () => {
  it("is placed by its zone, IANA or Windows, and by UTC", () => {
    // 10:00 in Zurich on 30 October 2026 is 09:00 UTC (winter time, CET).
    expect(instantOf(at("20261030T100000", "Europe/Zurich"))).toEqual({ at: Date.UTC(2026, 9, 30, 9), sure: true });
    // …and 08:00 UTC in summer (CEST).
    expect(instantOf(at("20260701T100000", "Europe/Zurich"))?.at).toBe(Date.UTC(2026, 6, 1, 8));
    expect(instantOf(at("20261030T100000", "W. Europe Standard Time"))?.at).toBe(Date.UTC(2026, 9, 30, 9));
    expect(instantOf(at("20261030T090000", null, true))?.at).toBe(Date.UTC(2026, 9, 30, 9));
  });

  it("is read as local time where the zone is unknown, and says so", () => {
    const unknown = instantOf(at("20261030T100000", "Somewhere Standard Time"));
    expect(unknown).toEqual({ at: new Date(2026, 9, 30, 10).getTime(), sure: false });
    expect(instantOf(at("nonsense", null))).toBeNull();
  });

  it("reads as a day and a span", () => {
    const now = new Date(2026, 9, 25);
    const allDay = whenText(at("20261224", null, false, true), null, now);
    expect(allDay).toMatch(/24/);
    expect(allDay).toMatch(/all day$/);
    const span = whenText(at("20261030T090000", null, true), at("20261030T100000", null, true), now);
    expect(span).toMatch(/–/);
    expect(whenText(null, null, now)).toBe("Time not given");
    expect(whenText(at("20261030T100000", "Somewhere Standard Time"), null, now)).toMatch(/\(Somewhere Standard Time\)$/);
  });
});
