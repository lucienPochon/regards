import { describe, expect, it } from "vitest";
import type { MessageDetail } from "./accounts";
import { asMarkdown } from "./messageText";

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
