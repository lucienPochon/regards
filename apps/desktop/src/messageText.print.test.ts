// @vitest-environment jsdom
// The page a conversation prints as, before the print dialog.
import { afterEach, expect, it } from "vitest";
import type { MessageDetail } from "./accounts";
import { printMessages } from "./messageText";

const detail = (id: number, text: string) =>
  ({
    message: {
      id,
      subject: "Garden <plot>",
      from: { name: "Lena Okafor", address: "lena@example.net" },
      sent_at: Date.UTC(2026, 9, 19, 12, id),
      received_at: Date.UTC(2026, 9, 19, 12, id),
    },
    mailbox_id: 1,
    to: [{ name: null, address: "sam@example.org" }],
    cc: [],
    text,
    has_html: false,
    remote_images: 0,
    attachments: [],
  }) as unknown as MessageDetail;

afterEach(() => document.querySelectorAll("iframe").forEach((frame) => frame.remove()));

it("prints a conversation's messages on one page, oldest first, each with its head", () => {
  printMessages([detail(1, "Saturdays work."), detail(2, "Then Saturday <10am>.")], "Garden <plot>");
  const page = document.querySelector("iframe")?.srcdoc ?? "";
  expect(page).toContain("<title>Garden &lt;plot&gt;</title>");
  expect(page.match(/<section>/g)).toHaveLength(2);
  expect(page.match(/<th>From<\/th>/g)).toHaveLength(2);
  expect(page.indexOf("Saturdays work.")).toBeLessThan(page.indexOf("Then Saturday &lt;10am&gt;."));
});
