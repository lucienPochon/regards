import { describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
const { searchMessages, searchTerm } = await import("./search");

describe("search", () => {
  it("asks the daemon with what was typed, every account by default", async () => {
    invoke.mockResolvedValueOnce({ messages: [], top: [], next: null, indexing: 0 });
    await searchMessages("garden from:lena");
    expect(invoke).toHaveBeenLastCalledWith("search_messages", {
      query: "garden from:lena",
      accountId: null,
      before: null,
      limit: null,
    });
    invoke.mockResolvedValueOnce({ messages: [], top: [], next: null, indexing: 0 });
    await searchMessages("garden", { accountId: "a1", before: { received_at: 5, id: 9 }, limit: 20 });
    expect(invoke).toHaveBeenLastCalledWith("search_messages", {
      query: "garden",
      accountId: "a1",
      before: { received_at: 5, id: 9 },
      limit: 20,
    });
  });

  it("writes terms the way the search reads them", () => {
    expect(searchTerm("from", "lena@example.net")).toBe("from:lena@example.net");
    expect(searchTerm("from", "Lena Müller")).toBe('from:"Lena Müller"');
    expect(searchTerm("subject", ' The "big" plan ')).toBe('subject:"The big plan"');
    expect(searchTerm("in", "Archive")).toBe("in:Archive");
  });
});
