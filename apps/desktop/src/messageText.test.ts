import { describe, expect, it } from "vitest";
import type { MessageDetail } from "./accounts";
import { asMarkdown, textParts } from "./messageText";

const detail = {
  message: {
    id: 1,
    subject: "Garden *plot* schedule",
    from: { name: "Lena Okafor", address: "lena@example.net" },
    sent_at: Date.UTC(2026, 9, 19, 12, 0),
    received_at: Date.UTC(2026, 9, 19, 12, 0),
  },
  mailbox_id: 1,
  to: [{ name: null, address: "sam@example.org" }],
  cc: [],
  text: "Saturdays work.\n\n> Earlier\n",
  has_html: false,
  remote_images: 0,
  attachments: [],
} as unknown as MessageDetail;

describe("a message as Markdown", () => {
  it("starts with its head in bold labels, Markdown in values escaped", () => {
    const text = asMarkdown(detail);
    expect(text).toContain("**From:** Lena Okafor <lena@example.net>  \n");
    expect(text).toContain("**To:** sam@example.org");
    expect(text).toContain("**Subject:** Garden \\*plot\\* schedule");
    expect(text).not.toContain("**Cc:**");
  });

  it("ends with the text as it came, quotes kept", () => {
    expect(asMarkdown(detail).endsWith("Saturdays work.\n\n> Earlier\n")).toBe(true);
  });
});

describe("web addresses in plain text", () => {
  const links = (text: string) => textParts(text).flatMap((part) => (part.url ? [part.url] : []));

  it("open https:// and www. addresses, and keep every character of the text", () => {
    const text = "Tickets: https://rail.example/t?id=4&x=1 or www.example.org/help\nThanks";
    expect(links(text)).toEqual(["https://rail.example/t?id=4&x=1", "https://www.example.org/help"]);
    expect(textParts(text).map((part) => part.text).join("")).toBe(text);
  });

  it("leave the sentence's punctuation and brackets out", () => {
    expect(links("See https://x.example/a.")).toEqual(["https://x.example/a"]);
    expect(links("(at https://x.example/a)")).toEqual(["https://x.example/a"]);
    expect(links("https://en.example/wiki/Bern_(city), then")).toEqual(["https://en.example/wiki/Bern_(city)"]);
    expect(links("<https://x.example/b>")).toEqual(["https://x.example/b"]);
  });

  it("leave http://, other schemes and a bare https:// as text", () => {
    expect(links("http://old.example javascript:alert(1) file:///c:/x https:// done")).toEqual([]);
  });
});
