// @vitest-environment jsdom
// Settings › Shortcuts rendered: changing keys, and the layers answering to them.
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve(null)), convertFileSrc: (path: string) => path }));
const { ShortcutsTab } = await import("./Settings");
const { getPrefs, setPrefs } = await import("./prefs");
const { useShortcuts } = await import("./shortcuts");

const change = (what: string, key: string, code = "") => {
  fireEvent.click(screen.getByRole("button", { name: `Change the key of ${what}` }));
  fireEvent.keyDown(screen.getByRole("button", { name: "Press a key…" }), { key, code });
};

beforeEach(() => setPrefs({ keySet: "standard", myKeys: {} }));
afterEach(cleanup);

describe("changing a key", () => {
  it("moves a key another action had, and says so", () => {
    render(<ShortcutsTab editable />);
    change("New message", "e", "KeyE");
    expect(getPrefs().myKeys).toEqual({ compose: ["e"], archive: [] });
    expect(screen.getByRole("status").textContent).toBe("e is now New message, no longer Done (archive).");
  });

  it("refuses the keys that never change", () => {
    render(<ShortcutsTab editable />);
    change("Done (archive)", "Enter", "Enter");
    expect(getPrefs().myKeys).toEqual({});
    expect(screen.getByRole("status").textContent).toMatch(/^Enter is taken/);
  });

  it("refuses a key without Ctrl for what acts while typing", () => {
    render(<ShortcutsTab editable />);
    change("Send", "s", "KeyS");
    expect(getPrefs().myKeys).toEqual({});
    expect(screen.getByRole("status").textContent).toMatch(/works while typing/);
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
