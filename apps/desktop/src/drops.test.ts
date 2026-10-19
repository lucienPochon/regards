import { describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
const { attachDropped } = await import("./compose");

describe("dropped files", () => {
  it("are attached by the drop's id, never by a path", async () => {
    invoke.mockResolvedValueOnce({ draft: null, refused: [{ name: "folder", error: "that is not a file" }] });
    const attached = await attachDropped(7, 3);
    expect(invoke).toHaveBeenLastCalledWith("draft_attach_dropped", { draftId: 7, dropId: 3 });
    expect(attached.refused[0].name).toBe("folder");
  });
});
