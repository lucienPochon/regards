import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
const { groupMessages, groupQuery, groupedIds, groupsFor } = await import("./search-groups");

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

describe("a group's messages", () => {
  // Pages of the search, newest first: ids 100… with received_at falling by 10 each.
  const message = (id: number) => ({ id, received_at: 10_000 - id * 10 });
  const pages = [
    { messages: [message(1), message(2)], next: { received_at: 9_980, id: 2 } },
    { messages: [message(3), message(4)], next: { received_at: 9_960, id: 4 } },
    { messages: [message(5)], next: null },
  ];
  beforeEach(() => {
    invoke.mockReset();
    invoke.mockImplementation((_command: string, args: { before: { id: number } | null }) =>
      Promise.resolve(args.before === null ? pages[0] : args.before.id === 2 ? pages[1] : pages[2]),
    );
  });

  it("are the first page when the list has not loaded further", async () => {
    expect((await groupMessages(saved(1, null), null, null)).map((one) => one.id)).toEqual([1, 2]);
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("go back as far as the list is loaded, and no further", async () => {
    // The list reaches 9_970: the second page covers it.
    expect((await groupMessages(saved(1, null), null, 9_970)).map((one) => one.id)).toEqual([1, 2, 3, 4]);
    expect(invoke).toHaveBeenCalledTimes(2);
    // Older than everything: every page.
    expect((await groupMessages(saved(1, null), null, 0)).map((one) => one.id)).toEqual([1, 2, 3, 4, 5]);
  });

  it("are left out of the list, whichever group found them", () => {
    const ids = groupedIds([
      { saved: saved(1, null), messages: [message(1), message(2)] as never },
      { saved: saved(2, null), messages: [message(7)] as never },
    ]);
    expect([...ids].sort()).toEqual([1, 2, 7]);
  });
});
