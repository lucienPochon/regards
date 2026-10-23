import { describe, expect, it, vi } from "vitest";
import type { MessageCursor, MessageListResult, MessageSummary } from "./accounts";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const { ALL_AT_ONCE, allOf } = await import("./mail");

/** A folder of `count` messages, newest first, read in pages as the daemon gives them. */
function folder(accountId: string, mailboxId: number, count: number, firstId: number) {
  const messages = Array.from({ length: count }, (_, at) => ({
    id: firstId + at,
    mailbox_id: mailboxId,
    listed_at: 1_000_000 - firstId - at,
  })) as MessageSummary[];
  const fetch = vi.fn(async (before: MessageCursor | null, limit: number | null): Promise<MessageListResult> => {
    const start = before === null ? 0 : messages.findIndex((message) => message.id === (before as unknown as number)) + 1;
    const page = messages.slice(start, start + (limit ?? 50));
    const last = page[page.length - 1];
    return { messages: page, next: start + page.length < messages.length && last ? (last.id as unknown as MessageCursor) : null };
  });
  return { key: `mailbox:${mailboxId}`, accountId, mailboxId, role: "inbox" as const, fetch };
}

describe("a whole group, to finish it", () => {
  it("reads every page of every inbox", async () => {
    const one = folder("a", 1, 450, 1);
    const two = folder("b", 2, 3, 5000);
    const items = await allOf([one, two]);
    expect(items).toHaveLength(453);
    expect(one.fetch).toHaveBeenCalledTimes(3);
    expect(items.find((item) => item.id === 5000)?.accountId).toBe("b");
  });

  it("stops at what one change may touch", async () => {
    const big = folder("a", 1, ALL_AT_ONCE + 300, 1);
    const items = await allOf([big]);
    expect(items).toHaveLength(ALL_AT_ONCE);
  });
});
