import { describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
const { imageSource, openAttachment, previewAttachment, saveAllAttachments, saveAttachment } = await import(
  "./attachments"
);

describe("attachments", () => {
  it("are named by their message and place, never by a path", async () => {
    invoke.mockResolvedValueOnce({ kind: "text", text: "hi", truncated: false });
    expect(await previewAttachment(7, 2)).toEqual({ kind: "text", text: "hi", truncated: false });
    expect(invoke).toHaveBeenLastCalledWith("attachment_preview", { messageId: 7, index: 2 });

    invoke.mockResolvedValueOnce("notes (2).txt");
    expect(await saveAttachment(7, 2, "notes.txt")).toBe("notes (2).txt");
    expect(invoke).toHaveBeenLastCalledWith("attachment_save", { messageId: 7, index: 2, name: "notes.txt" });

    invoke.mockResolvedValueOnce(null);
    expect(await saveAllAttachments(7)).toBeNull();
    expect(invoke).toHaveBeenLastCalledWith("attachment_save_all", { messageId: 7 });

    invoke.mockResolvedValueOnce(undefined);
    await openAttachment(7, 0);
    expect(invoke).toHaveBeenLastCalledWith("attachment_open", { messageId: 7, index: 0 });
  });

  it("show images from their own type", () => {
    expect(imageSource({ kind: "image", mime: "image/png", data: "iVBO" })).toBe("data:image/png;base64,iVBO");
  });
});
