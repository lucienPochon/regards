import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const { swapSignatureHtml, textToHtml } = await import("./compose");

describe("signatures in a formatted draft", () => {
  const reply = textToHtml("Yes!\n\nOn 2026-10-18 10:05 UTC, Bob wrote:\n> Plans?");

  it("swaps the old signature for the new one", () => {
    const signed = textToHtml("Yes!\n\nAda\nRegards") + textToHtml("On 2026-10-18 10:05 UTC, Bob wrote:\n> Plans?");
    expect(swapSignatureHtml(signed, "Ada\nRegards", "Ada Lovelace")).toBe(
      "<p>Yes!</p><p>Ada Lovelace</p><p>On 2026-10-18 10:05 UTC, Bob wrote:</p><blockquote><p>Plans?</p></blockquote>",
    );
    expect(swapSignatureHtml(signed, "Ada\nRegards", "")).toBe(
      "<p>Yes!</p><p>On 2026-10-18 10:05 UTC, Bob wrote:</p><blockquote><p>Plans?</p></blockquote>",
    );
  });

  it("puts a new one above the quote, a forward, or at the end", () => {
    expect(swapSignatureHtml(reply, "", "Ada")).toBe(
      "<p>Yes!</p><p>Ada</p><p>On 2026-10-18 10:05 UTC, Bob wrote:</p><blockquote><p>Plans?</p></blockquote>",
    );
    const forward = textToHtml("See below\n\n---------- Forwarded message ----------\nSubject: Plans\n\nHi");
    expect(swapSignatureHtml(forward, "", "Ada")).toBe(
      "<p>See below</p><p>Ada</p><p>---------- Forwarded message ----------<br>Subject: Plans</p><p>Hi</p>",
    );
    expect(swapSignatureHtml("<p>Hello</p>", "Gone", "Bea")).toBe("<p>Hello</p><p>Bea</p>");
  });

  it("starts a new message with room above the signature", () => {
    expect(textToHtml("\n\nAda")).toBe("<p></p><p>Ada</p>");
  });
});
