// @vitest-environment jsdom
// A list at rest draws nothing: the spinner at its end is there only while a
// page is being fetched. Left spinning out of sight, it kept the window
// drawing and Regards at several percent of a processor for as long as a
// folder had more mail than was listed.
import { act, cleanup, render, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { MessageCursor, MessageListResult, MessageSummary } from "./accounts";
import type { Source } from "./mail";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
const { ListEnd } = await import("./MessageList");
const { useFeed } = await import("./mail");

afterEach(cleanup);

const message = (id: number) => ({ id, mailbox_id: 1, listed_at: id }) as MessageSummary;

describe("the end of a list with more to fetch", () => {
  it("has no spinner while nothing is being fetched", () => {
    const { container } = render(<ListEnd loading={false} />);
    expect(container.querySelector(".row-more")).not.toBeNull();
    expect(container.querySelector(".spinner")).toBeNull();
  });

  it("has one while a page is being fetched", () => {
    const { container } = render(<ListEnd loading />);
    expect(container.querySelector(".row-more .spinner")).not.toBeNull();
  });
});

describe("a feed", () => {
  it("says it is fetching more only during the fetch", async () => {
    let finish: (page: MessageListResult) => void = () => {};
    const fetch = vi.fn((before: MessageCursor | null): Promise<MessageListResult> => {
      if (before === null) {
        return Promise.resolve({ messages: [message(2)], next: { received_at: 2, id: 2 } } as MessageListResult);
      }
      return new Promise((resolve) => (finish = resolve));
    });
    const sources: Source[] = [{ key: "inbox", accountId: "a", mailboxId: 1, role: "inbox", fetch }];
    const { result } = renderHook(() => useFeed(sources));

    await waitFor(() => expect(result.current.more).toBe(true));
    expect(result.current.loadingMore).toBe(false);

    act(() => result.current.loadMore());
    expect(result.current.loadingMore).toBe(true);

    await act(async () => finish({ messages: [message(1)], next: null } as MessageListResult));
    await waitFor(() => expect(result.current.loadingMore).toBe(false));
    expect(result.current.more).toBe(false);
    expect(result.current.items.map((item) => item.id)).toEqual([2, 1]);
  });
});
