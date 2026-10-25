import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const { groupQuery, groupsFor } = await import("./search-groups");

const saved = (id: number, account_id: string | null) => ({ id, name: `S${id}`, query: "from:@acme.example", account_id, color: null });

describe("a saved search as a group", () => {
  it("finds what it finds in the inbox", () => {
    expect(groupQuery(" from:@acme.example ")).toBe("from:@acme.example in:inbox");
  });

  it("shows when chosen, in the sidebar's order, and not in another account's inbox", () => {
    const searches = [saved(1, null), saved(2, "work"), saved(3, "home"), saved(4, null)];
    expect(groupsFor(searches, [4, 1, 2, 3], null).map((one) => one.id)).toEqual([1, 2, 3, 4]);
    expect(groupsFor(searches, [1, 2, 3], "work").map((one) => one.id)).toEqual([1, 2]);
  });
});
