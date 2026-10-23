import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const { fillPlaceholders, insertTemplateHtml, swapSignatureHtml, textToHtml } = await import("./compose");

describe("templates in a draft", () => {
  it("go where one writes: above the signature, in place of the empty first line", () => {
    const signed = textToHtml("\n\nAda");
    expect(insertTemplateHtml(signed, { body: "Thanks for booking.\nSee you soon.", html: null })).toBe(
      "<p>Thanks for booking.<br>See you soon.</p><p>Ada</p>",
    );
    expect(insertTemplateHtml("<p>Hi</p>", { body: "", html: "<p><b>Formatted</b></p>" })).toBe(
      "<p><b>Formatted</b></p><p>Hi</p>",
    );
  });
});

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

describe("template placeholders", () => {
  const names = { to: "Lena Müller", me: "Ada Lovelace" };

  it("take the first recipient's names and the sender's", () => {
    expect(fillPlaceholders("Dear {{First Name}}, from {{my first name}} ({{ my name }})", names)).toBe(
      "Dear Lena, from Ada (Ada Lovelace)",
    );
    expect(fillPlaceholders("{{name}} {{last name}}", names)).toBe("Lena Müller Müller");
  });

  it("stay for the send check to ask about when there is nothing to fill them", () => {
    expect(fillPlaceholders("Hi {{name}}, [[date]] {{order}}", { to: null, me: null })).toBe(
      "Hi {{name}}, [[date]] {{order}}",
    );
    expect(fillPlaceholders("{{last name}}", { to: "Lena", me: null })).toBe("{{last name}}");
  });

  it("are escaped in formatted text", () => {
    expect(fillPlaceholders("<p>Hi {{name}}</p>", { to: "Bob <& Co>", me: null }, true)).toBe(
      "<p>Hi Bob &lt;&amp; Co&gt;</p>",
    );
  });
});
