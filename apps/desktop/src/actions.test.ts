import { describe, expect, it, vi } from "vitest";
import type { Target } from "./actions";

const invoke = vi.fn(async (command: string, args?: { ids?: number[] }) => {
  if (command === "message_conversation_ids") return [...(args?.ids ?? []), 7, 8];
  if (command === "message_archive") return { moved: args?.ids?.length ?? 0, undo: null };
  if (command === "message_set_flags") return { changed: args?.ids?.length ?? 0 };
  if (command === "message_mute") return { changed: args?.ids?.length ?? 0, undo: null };
  if (command === "message_copy") return { copied: args?.ids?.length ?? 0 };
  return null;
});
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
const { messageActions, newSendersOf } = await import("./actions");

const target = (conversation: boolean): Target => ({
  ids: [1],
  single: 1,
  accountId: "a",
  mailboxId: 10,
  seen: false,
  flagged: false,
  disposable: false,
  pinned: false,
  aside: false,
  snoozed: false,
  reminding: false,
  reminded: false,
  conversation,
});
const host = () => ({
  compose: vi.fn(),
  moved: vi.fn(),
  marked: vi.fn(),
  failed: vi.fn(),
  confirm: vi.fn(async () => true),
  pickFolder: vi.fn(async () => null),
  pickSnooze: vi.fn(async () => null),
  pickReminder: vi.fn(async () => null),
});
const run = async (on: Target, id: string, using = host()) => {
  invoke.mockClear();
  messageActions(on, using as never)
    .find((action) => action.id === id)
    ?.run(null);
  await new Promise((done) => setTimeout(done, 0));
  return using;
};
const sent = (command: string) => invoke.mock.calls.find(([name]) => name === command)?.[1];

describe("an action on a conversation's row", () => {
  it("archives the rest of the conversation in the folder too, and reports the row", async () => {
    const used = await run(target(true), "archive");
    expect(sent("message_archive")).toEqual({ ids: [1, 7, 8] });
    expect(used.moved).toHaveBeenCalledWith([1], { moved: 3, undo: null });
  });

  it("marks the whole conversation read, but flags only the row's message", async () => {
    await run(target(true), "read");
    expect(sent("message_set_flags")).toMatchObject({ ids: [1, 7, 8] });
    await run(target(true), "flag");
    expect(sent("message_conversation_ids")).toBeUndefined();
    expect(sent("message_set_flags")).toMatchObject({ ids: [1] });
  });

  it("stays on one message for a card's own menu", async () => {
    await run(target(false), "archive");
    expect(sent("message_conversation_ids")).toBeUndefined();
    expect(sent("message_archive")).toEqual({ ids: [1] });
  });
});

describe("Done on a new sender's mail", () => {
  it("accepts the sender first, then files the conversation", async () => {
    const accept = [{ accountId: "a", address: "lena@example.net" }];
    await run({ ...target(true), accept }, "archive");
    const commands = invoke.mock.calls.map(([name]) => name);
    expect(sent("sender_set")).toEqual({
      sender: { account_id: "a", address: "lena@example.net", policy: "people", priority: false },
    });
    expect(commands.indexOf("sender_set")).toBeLessThan(commands.indexOf("message_archive"));
    expect(sent("message_archive")).toEqual({ ids: [1, 7, 8] });
  });

  it("names each new sender once, and only new senders", () => {
    const lena = { name: "Lena", address: "Lena@example.net" };
    expect(
      newSendersOf([
        { accountId: "a", section: "new_senders", from: lena },
        { accountId: "a", section: "new_senders", from: { ...lena, address: "lena@example.net" } },
        { accountId: "a", section: "people", from: { name: "Sam", address: "sam@example.org" } },
        { accountId: "a", section: "new_senders", from: null },
      ]),
    ).toEqual([{ accountId: "a", address: "lena@example.net" }]);
  });
});

describe("Copy to", () => {
  it("asks for a folder to copy to, and copies the whole conversation there", async () => {
    const using = { ...host(), pickFolder: vi.fn(async () => 20), copied: vi.fn() };
    await run(target(true), "copy", using as never);
    await new Promise((done) => setTimeout(done, 0));
    expect(using.pickFolder).toHaveBeenCalledWith("a", 10, null, "copy");
    expect(sent("message_copy")).toEqual({ ids: [1, 7, 8], mailboxId: 20 });
    expect(using.copied).toHaveBeenCalledWith(3, 20);
  });

  it("is not offered where labels do it (Gmail), nor without a way to say it was done", () => {
    const ids = (using: object) => messageActions(target(false), using as never).map((action) => action.id);
    expect(ids({ ...host(), copied: vi.fn(), hasLabels: () => true })).not.toContain("copy");
    expect(ids(host())).not.toContain("copy");
    expect(ids({ ...host(), copied: vi.fn() })).toContain("copy");
  });
});

describe("Mute", () => {
  it("mutes the whole conversation, and unmutes a muted one", async () => {
    const used = await run(target(true), "mute");
    expect(sent("message_mute")).toEqual({ ids: [1, 7, 8], muted: true });
    expect(used.marked).toHaveBeenCalledWith({ changed: 3, undo: null });
    await run({ ...target(true), muted: true }, "mute");
    expect(sent("message_mute")).toEqual({ ids: [1, 7, 8], muted: false });
  });
});
