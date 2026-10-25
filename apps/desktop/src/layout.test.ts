import { describe, expect, it } from "vitest";
import { type Layout, inboxViewOf } from "./layout";

const layout = (accountViews: Layout["accountViews"]): Layout => ({
  mode: "split",
  locked: true,
  listWidth: 460,
  inboxView: "date",
  accountViews,
  handle: true,
});

describe("an inbox's view", () => {
  it("is its account's own, or the one of all inboxes", () => {
    const chosen = layout({ work: "smart" });
    expect(inboxViewOf(chosen, "work")).toBe("smart");
    expect(inboxViewOf(chosen, "home")).toBe("date");
    expect(inboxViewOf(chosen, null)).toBe("date");
  });
});
