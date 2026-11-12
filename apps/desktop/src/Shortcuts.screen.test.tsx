// @vitest-environment jsdom
// Settings › Shortcuts rendered: changing keys, and the layers answering to them.
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve(null)), convertFileSrc: (path: string) => path }));
const { KEY_PARTS, ShortcutsTab } = await import("./Settings");
const { KEYED_ACTIONS, keysFor } = await import("./actions");
const { getPrefs, setPrefs } = await import("./prefs");
const { COMMANDS, useShortcuts } = await import("./shortcuts");

const confirm = vi.fn(() => Promise.resolve(true));
const page = () => render(<ShortcutsTab confirm={confirm} />);
const field = (what: string) => screen.getByRole("button", { name: `Change the key of ${what}` });
const pickSet = (name: string) => {
  fireEvent.click(screen.getByRole("button", { name: /^Keys in use: / }));
  fireEvent.click(screen.getByRole("menuitem", { name }));
};

const change = (what: string, key: string, code = "") => {
  fireEvent.click(screen.getByRole("button", { name: `Change the key of ${what}` }));
  fireEvent.keyDown(screen.getByRole("button", { name: "Press a key…" }), { key, code });
};

beforeEach(() => {
  setPrefs({ keySet: "standard", myKeys: {} });
  confirm.mockClear();
});
afterEach(cleanup);

describe("changing a key", () => {
  it("moves a key another action had, and says so", () => {
    page();
    change("New message", "e", "KeyE");
    expect(getPrefs().myKeys).toEqual({ compose: ["e"], archive: [] });
    expect(screen.getByRole("status").textContent).toBe("E is now New message, no longer Done (archive).");
  });

  it("refuses the keys that never change", () => {
    page();
    change("Done (archive)", "Enter", "Enter");
    expect(getPrefs().myKeys).toEqual({});
    expect(screen.getByRole("status").textContent).toMatch(/^Enter is taken/);
  });

  it("refuses a key without Ctrl for what acts while typing", () => {
    page();
    change("Send", "s", "KeyS");
    expect(getPrefs().myKeys).toEqual({});
    expect(screen.getByRole("status").textContent).toMatch(/works while typing/);
  });
});

describe("the page", () => {
  it("has one row for every action and command", () => {
    const rows = KEY_PARTS.flatMap((part) => part.ids);
    expect([...rows].sort()).toEqual([...KEYED_ACTIONS, ...COMMANDS].map((one) => one.id).sort());
  });

  it("shows each key in a field, as printed on the keyboard", () => {
    page();
    expect(field("Print the open message").textContent).toBe("P");
    expect(field("Reply").textContent).toBe("Shift + R");
    expect(field("Move to a folder").textContent).toBe("Ctrl + Shift + M");
    expect(field("Delete").textContent).toBe("Delete or Backspace");
    expect(field("Zoom in").textContent).toBe("Ctrl + = or Ctrl + +");
    expect(field("Mute the conversation").textContent).toBe("No key");
  });

  it("keeps the rows whose name or key has what is searched", () => {
    page();
    const search = screen.getByRole("searchbox", { name: "Search for an action or a key" });
    fireEvent.change(search, { target: { value: "zoom" } });
    expect(screen.getAllByRole("button", { name: /^Change the key of / }).map((one) => one.getAttribute("aria-label"))).toEqual([
      "Change the key of Zoom in",
      "Change the key of Zoom out",
    ]);
    fireEvent.change(search, { target: { value: "ctrl+shift+m" } });
    expect(screen.getAllByRole("button", { name: /^Change the key of / })).toHaveLength(1);
    expect(field("Move to a folder")).toBeTruthy();
    fireEvent.change(search, { target: { value: "qqqq" } });
    expect(screen.getByRole("status").textContent).toBe("Nothing matches.");
  });

  it("waits for a key once the field is clicked, and Escape leaves it as it was", () => {
    page();
    fireEvent.click(field("Reply"));
    fireEvent.keyDown(screen.getByRole("button", { name: "Press a key…" }), { key: "Shift" });
    fireEvent.keyDown(screen.getByRole("button", { name: "Press a key…" }), { key: "Escape" });
    expect(field("Reply").textContent).toBe("Shift + R");
    expect(getPrefs().myKeys).toEqual({});
  });

  it("takes a key away, and brings the set's back", () => {
    page();
    fireEvent.click(screen.getByRole("button", { name: "No key for Reply" }));
    expect(field("Reply").textContent).toBe("No key");
    expect(getPrefs().myKeys).toEqual({ reply: [] });
    fireEvent.click(screen.getByRole("button", { name: "Reply: back to the key set's key" }));
    expect(field("Reply").textContent).toBe("Shift + R");
    expect(getPrefs().myKeys).toEqual({});
  });
});

describe("the keys in use", () => {
  it("change every field to the set chosen", () => {
    page();
    pickSet("Gmail");
    expect(getPrefs().keySet).toBe("gmail");
    expect(screen.getByRole("button", { name: "Keys in use: Gmail" })).toBeTruthy();
    expect(field("Reply").textContent).toBe("R");
    expect(field("Snooze").textContent).toBe("B");
    expect(screen.getByText(/two-key moves/)).toBeTruthy();
    expect(confirm).not.toHaveBeenCalled();
  });

  it("ask before dropping keys the person chose, and keep them on No", async () => {
    setPrefs({ myKeys: { reply: ["w"] } });
    confirm.mockResolvedValueOnce(false);
    page();
    pickSet("Gmail");
    await waitFor(() => expect(confirm).toHaveBeenCalledTimes(1));
    await Promise.resolve();
    expect(getPrefs()).toMatchObject({ keySet: "standard", myKeys: { reply: ["w"] } });
  });

  it("start clean on Yes", async () => {
    setPrefs({ myKeys: { reply: ["w"] } });
    page();
    pickSet("Apple Mail");
    await waitFor(() => expect(getPrefs().keySet).toBe("apple"));
    expect(getPrefs().myKeys).toEqual({});
    expect(field("Reply").textContent).toBe("Ctrl + R");
  });

  it("leave the set alone when it is the one already in use", () => {
    setPrefs({ myKeys: { reply: ["w"] } });
    page();
    pickSet("Regards");
    expect(confirm).not.toHaveBeenCalled();
    expect(getPrefs().myKeys).toEqual({ reply: ["w"] });
  });

  it("work for the message actions as for the rest", () => {
    expect(keysFor("superhuman", {}).snooze).toEqual(["h"]);
    expect(keysFor("superhuman", { snooze: ["w"] }).snooze).toEqual(["w"]);
  });
});

function Layer({ onCompose }: { onCompose: () => void }) {
  useShortcuts({ compose: onCompose });
  return null;
}

describe("a command's keys", () => {
  it("are the ones chosen, not the set's", () => {
    const compose = vi.fn();
    render(<Layer onCompose={compose} />);
    fireEvent.keyDown(window, { key: "c", code: "KeyC" });
    expect(compose).toHaveBeenCalledTimes(1);
    setPrefs({ myKeys: { compose: ["w"] } });
    fireEvent.keyDown(window, { key: "c", code: "KeyC" });
    fireEvent.keyDown(window, { key: "w", code: "KeyW" });
    expect(compose).toHaveBeenCalledTimes(2);
  });
});

describe("the page's own keys", () => {
  it("never reload or print, even when nothing answers them", () => {
    render(<Layer onCompose={vi.fn()} />);
    expect(fireEvent.keyDown(window, { key: "r", code: "KeyR", ctrlKey: true })).toBe(false);
    expect(fireEvent.keyDown(window, { key: "p", code: "KeyP", ctrlKey: true })).toBe(false);
    expect(fireEvent.keyDown(window, { key: "t", code: "KeyT", ctrlKey: true })).toBe(true);
  });
});
