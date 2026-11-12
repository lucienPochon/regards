// @vitest-environment jsdom
// The sidebar rendered: each account's folders, in the three ways Settings › Appearance offers.
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AccountSummary, MailboxSummary } from "./accounts";
import type { AccountMail } from "./mail";
import type { SidebarChanges, SidebarPrefs } from "./sidebar-prefs";
import type { View } from "./views";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const { Sidebar } = await import("./Sidebar");
const { DEFAULT_PREFS, setPrefs } = await import("./prefs");

afterEach(() => {
  cleanup();
  setPrefs({ sidebarAccounts: DEFAULT_PREFS.sidebarAccounts });
});

const box = (id: number, name: string, role: MailboxSummary["role"] = null, unread?: number): MailboxSummary => ({
  id,
  name,
  delimiter: "/",
  role,
  selectable: true,
  unread,
});

const account = (id: string, email: string, mailboxes: MailboxSummary[]): AccountMail => ({
  account: { id, display_name: id, email } as AccountSummary,
  mailboxes,
  sync: { running: false, last_success_at: 1, last_error: null, push_active: true },
  error: null,
});

// A new mailbox with only an inbox and Spam, and one with the usual folders and its own.
const small = account("small", "hello@example.org", [box(1, "INBOX", "inbox"), box(2, "Spam", "junk", 2)]);
const full = account("full", "work@example.org", [
  box(10, "INBOX", "inbox", 4),
  box(11, "Drafts", "drafts"),
  box(12, "Sent", "sent"),
  box(13, "Trash", "trash"),
  box(14, "Clients"),
  box(15, "Clients/Schools"),
]);

function show(prefs: Partial<SidebarPrefs> = {}) {
  const onView = vi.fn<(view: View) => void>();
  const changes = {
    star: vi.fn(),
    unstar: vi.fn(),
    visit: vi.fn(),
    forget: vi.fn(),
    clearRecents: vi.fn(),
    hide: vi.fn(),
    show: vi.fn(),
    arrange: vi.fn(),
    fold: vi.fn(),
    choose: vi.fn(),
  } satisfies SidebarChanges;
  const onNewFolder = vi.fn();
  render(
    <Sidebar
      mail={{ state: "ready", accounts: [small, full] }}
      outbox={[]}
      view={{ kind: "role", role: "inbox" }}
      onView={onView}
      expanded
      locked
      onToggleLock={vi.fn()}
      onExpand={vi.fn()}
      onSettings={vi.fn()}
      onSync={vi.fn()}
      syncing={false}
      syncProblem={null}
      prefs={{ starred: [], recents: [], hidden: [], order: [], folded: [], account: null, ...prefs }}
      changes={changes}
      onMenu={vi.fn()}
      onDrop={vi.fn()}
      onNewFolder={onNewFolder}
      onAddAccount={vi.fn()}
    />,
  );
  return { onView, changes, onNewFolder };
}

describe("a list of accounts (the default)", () => {
  it("shows every account with all its folders, its drafts aside", () => {
    const { onView } = show();
    for (const name of ["hello@example.org", "Junk", "work@example.org", "Clients", "Schools", "Add an account…"]) {
      expect(screen.getByText(name)).toBeTruthy();
    }
    // What is being written is in the one Drafts row.
    expect(screen.getAllByText("Drafts")).toHaveLength(1);
    // No chevrons on Inbox, Sent and Trash: the list of accounts stands in for them.
    expect(screen.queryByRole("button", { name: "Show accounts" })).toBeNull();

    fireEvent.click(screen.getByText("Clients"));
    expect(onView).toHaveBeenCalledWith({ kind: "mailbox", mailboxId: 14, accountId: "full" });
  });

  it("folds an account away, then says its unread mail on its row", () => {
    const { changes } = show({ folded: ["full"] });
    expect(screen.queryByText("Clients")).toBeNull();
    const row = screen.getByTitle("work@example.org");
    expect(within(row).getByText("4")).toBeTruthy();
    fireEvent.click(row);
    expect(changes.fold).toHaveBeenCalledWith("full", false);
  });

  it("adds a folder to an account from its row", () => {
    const { onNewFolder } = show();
    fireEvent.click(screen.getByRole("button", { name: "New folder in hello@example.org" }));
    expect(onNewFolder).toHaveBeenCalledWith("small");
  });
});

describe("one account at a time", () => {
  it("shows the folders of the account chosen at the top, and no other's", () => {
    setPrefs({ sidebarAccounts: "switcher" });
    const { changes, onView } = show({ account: "small" });
    expect(screen.getByRole("button", { name: "hello@example.org" }).getAttribute("aria-pressed")).toBe("true");
    expect(screen.getByText("Junk")).toBeTruthy();
    expect(screen.queryByText("Clients")).toBeNull();
    expect(screen.queryByText("Sent")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "work@example.org" }));
    expect(changes.choose).toHaveBeenCalledWith("full");
    expect(onView).toHaveBeenCalledWith({ kind: "mailbox", mailboxId: 10, accountId: "full" });
  });

  it("shows all accounts together until one is chosen", () => {
    setPrefs({ sidebarAccounts: "switcher" });
    show();
    expect(screen.getByRole("button", { name: "All accounts" }).getAttribute("aria-pressed")).toBe("true");
    expect(screen.getByText("Sent")).toBeTruthy();
    expect(screen.queryByText("Clients")).toBeNull();
  });
});

describe("under the inbox", () => {
  it("opens each account onto its other folders", () => {
    setPrefs({ sidebarAccounts: "inbox" });
    show();
    expect(screen.queryByText("Clients")).toBeNull();
    fireEvent.click(screen.getAllByRole("button", { name: "Show accounts" })[0]);
    expect(screen.getByText("Clients")).toBeTruthy();
    expect(screen.getByText("Junk")).toBeTruthy();
  });
});
