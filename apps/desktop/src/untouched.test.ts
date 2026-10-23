import { describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
const { forwardDraft, getDraft, isUntouched, newDraft, replyDraft, touchDraft } = await import("./compose");

describe("drafts started here", () => {
  it("are untouched until something is done to them; others never are", async () => {
    invoke.mockResolvedValueOnce({ id: 1 }).mockResolvedValueOnce({ id: 2 }).mockResolvedValueOnce({ id: 3 });
    await newDraft("a");
    await replyDraft(10, false);
    await forwardDraft(11);
    expect([1, 2, 3].map(isUntouched)).toEqual([true, true, true]);

    touchDraft(2);
    expect(isUntouched(2)).toBe(false);

    // Opened from Drafts (or written by an agent): something to keep.
    invoke.mockResolvedValueOnce({ id: 4 });
    await getDraft(4);
    expect(isUntouched(4)).toBe(false);
  });
});
