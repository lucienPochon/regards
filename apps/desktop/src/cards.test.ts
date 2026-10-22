import { describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke, convertFileSrc: (path: string) => path }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
const { inboxSources, sectionSources } = await import("./mail");

const mail = [
  {
    account: { id: "a" },
    mailboxes: [
      { id: 7, role: "inbox", selectable: true },
      { id: 8, role: "sent", selectable: true },
    ],
  },
] as unknown as Parameters<typeof inboxSources>[0];

describe("the card view's sources", () => {
  it("ask for one section's unread mail, and for the inbox's mail read", async () => {
    invoke.mockResolvedValue({ messages: [], next: null });
    const [unread] = sectionSources(mail, "people", false);
    await unread.fetch(null, 5);
    expect(invoke).toHaveBeenLastCalledWith("message_list", {
      mailboxId: 7,
      before: null,
      limit: 5,
      section: "people",
      seen: false,
    });

    const read = inboxSources(mail, true);
    expect(read).toHaveLength(1);
    await read[0].fetch(null, 50);
    expect(invoke).toHaveBeenLastCalledWith("message_list", {
      mailboxId: 7,
      before: null,
      limit: 50,
      section: null,
      seen: true,
    });
  });

  it("are told apart from the same section unfiltered", () => {
    const [all] = sectionSources(mail, "people");
    const [unread] = sectionSources(mail, "people", false);
    expect(all.key).not.toBe(unread.key);
  });
});
