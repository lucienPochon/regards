import { describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
const { attachImage, coloursAsMarks, coloursAsStyles, toEditorHtml, toStoredHtml } = await import("./images");

describe("images in the text", () => {
  it("are kept by Content-ID, without their bytes, and shown from them again", () => {
    const edited = `<p>Look <img src="data:image/png;base64,AAAA" alt="Lake" data-cid="lake@regards"> there</p>`;
    const stored = toStoredHtml(edited);
    expect(stored).toBe(`<p>Look <img src="cid:lake@regards" alt="Lake"> there</p>`);
    expect(toEditorHtml(stored, [{ content_id: "lake@regards", mime: "image/png", data: "AAAA" }])).toBe(
      `<p>Look <img src="data:image/png;base64,AAAA" alt="Lake" data-cid="lake@regards"> there</p>`,
    );
  });

  it("leave other images alone, for the daemon to drop", () => {
    const web = `<p><img src="https://tracker.example/p.gif"></p>`;
    expect(toStoredHtml(web)).toBe(web);
    expect(toEditorHtml(`<p><img src="cid:gone@regards" alt=""></p>`, [])).toBe(
      `<p><img src="" alt="" data-cid="gone@regards"></p>`,
    );
  });

  it("are sent to the daemon as base64", async () => {
    invoke.mockResolvedValueOnce({ content_id: "x@regards", draft: { id: 4 } });
    await attachImage(4, { name: "paste.png", mime: "image/png", data: "AAAA" });
    expect(invoke).toHaveBeenLastCalledWith("draft_attach_image", {
      draftId: 4,
      name: "paste.png",
      mime: "image/png",
      data: "AAAA",
    });
  });
});

describe("colours and picture sizes in the text", () => {
  it("are kept as styles and sizes, and marked again for the editor", () => {
    const edited =
      `<p><span data-colour="#c62828" data-highlight="#fff59d">urgent</span> ` +
      `<img src="data:image/png;base64,AAAA" alt="" data-cid="lake@regards" width="320.4" height="180"></p>`;
    const stored = toStoredHtml(edited);
    expect(stored).toBe(
      `<p><span style="color: #c62828; background-color: #fff59d">urgent</span> ` +
        `<img src="cid:lake@regards" alt="" width="320" height="180"></p>`,
    );
    expect(toEditorHtml(stored, [{ content_id: "lake@regards", mime: "image/png", data: "AAAA" }])).toBe(
      `<p><span data-colour="#c62828" data-highlight="#fff59d">urgent</span> ` +
        `<img src="data:image/png;base64,AAAA" alt="" data-cid="lake@regards" width="320" height="180"></p>`,
    );
  });

  it("leave other spans and styles alone, for the daemon to judge", () => {
    expect(coloursAsStyles(`<span data-colour="red">x</span><span>y</span>`)).toBe(`<span data-colour="red">x</span><span>y</span>`);
    expect(coloursAsMarks(`<span style="font-size: 40px">x</span>`)).toBe(`<span style="font-size: 40px">x</span>`);
  });
});
