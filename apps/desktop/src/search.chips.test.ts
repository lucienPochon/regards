import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const { hasTerm, toggleTerm } = await import("./search");

describe("a search chip", () => {
  it("adds its words at the end of the search, and takes them out again", () => {
    expect(toggleTerm("lena", "is:unread")).toBe("lena is:unread");
    expect(toggleTerm("lena ", "is:unread")).toBe("lena is:unread");
    expect(toggleTerm("", "has:attachment")).toBe("has:attachment");
    expect(toggleTerm("lena is:unread plan", "is:unread")).toBe("lena plan");
    expect(toggleTerm("Is:Unread", "is:unread")).toBe("");
  });

  it("is on only for its whole words", () => {
    expect(hasTerm("lena is:unread", "is:unread")).toBe(true);
    expect(hasTerm("lena -is:unread", "is:unread")).toBe(false);
    expect(hasTerm("in:sentbox", "in:sent")).toBe(false);
    expect(hasTerm("plan last  7 days", "last 7 days")).toBe(true);
    expect(toggleTerm("plan last 7 days", "last 7 days")).toBe("plan");
  });
});
