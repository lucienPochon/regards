import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const { KEYED_ACTIONS, keysFor } = await import("./actions");
const { COMMANDS, commandKeys, keyText, refusedKey, shortcutList } = await import("./shortcuts");

const labels = Object.fromEntries([...KEYED_ACTIONS, ...COMMANDS].map((one) => [one.id, one.label]));
const everyKey = (set: "standard" | "classic", mine: Record<string, string[]>) => ({ ...keysFor(set, mine), ...commandKeys(set, mine) });

describe("keys the person chose", () => {
  it("take the place of the key set's, and none is a choice too", () => {
    const keys = keysFor("standard", { archive: ["y"], flag: [] });
    expect(keys.archive).toEqual(["y"]);
    expect(keys.flag).toEqual([]);
    expect(keys.snooze).toEqual(["s"]);
  });

  it("change the app's own commands too", () => {
    const keys = commandKeys("standard", { search: ["Mod+Shift+S"], undo: [] });
    expect(keys.search).toEqual(["Mod+Shift+S"]);
    expect(keys.undo).toEqual([]);
    expect(keys.compose).toEqual(["c"]);
    // A key set without one leaves it free to give.
    expect(commandKeys("classic", {}).print).toEqual([]);
    expect(commandKeys("classic", { print: ["Shift+P"] }).print).toEqual(["Shift+P"]);
  });

  it("show in the list of shortcuts, in place of the set's", () => {
    const mine = { archive: ["y"], replyAll: ["Shift+A"], mute: ["m"], compose: ["w"] };
    const list = shortcutList("standard", everyKey("standard", mine), labels);
    expect(list.find((line) => line.action.startsWith("Done"))?.keys).toEqual(["y"]);
    expect(list.find((line) => line.action.startsWith("Reply,"))?.keys).toEqual(["Shift+R", "Shift+A", "f"]);
    expect(list.find((line) => line.action === "Mute the conversation")?.keys).toEqual(["m"]);
    expect(list.find((line) => line.action === "New message")?.keys).toEqual(["w"]);
    expect(list.find((line) => line.action === "Pin or unpin")?.keys).toEqual(["d"]);
  });

  it("keep the arrows beside the keys that step, and drop a line left without keys", () => {
    const list = shortcutList("standard", everyKey("standard", { next: [], print: [] }), labels);
    expect(list.find((line) => line.action === "Next message")?.keys).toEqual(["↓", "→"]);
    expect(list.find((line) => line.action === "Previous message")?.keys).toEqual(["k", "↑", "←"]);
    expect(list.find((line) => line.action === "Print the open message")).toBeUndefined();
  });

  it("are shown with Ctrl and a capital, and Space by name", () => {
    expect(keyText("Mod+k")).toMatch(/^(Ctrl|⌘)\+K$/);
    expect(keyText("Mod+Shift+F")).toMatch(/^(Ctrl|⌘)\+Shift\+F$/);
    expect(keyText(" ")).toBe("Space");
    expect(keyText("Shift+R")).toBe("Shift+R");
  });
});

describe("keys that cannot be given", () => {
  it("are the ones that never change: Escape, Enter, Tab, the arrows, copy and paste", () => {
    for (const name of ["Escape", "Enter", "Tab", "Shift+Tab", "ArrowDown", "ArrowLeft", "Mod+c", "Mod+v"]) {
      expect(refusedKey("archive", [name])).not.toBeNull();
    }
    expect(refusedKey("archive", ["j"])).toBeNull();
    expect(refusedKey("search", ["p"])).toBeNull();
  });

  it("for what works while typing, need Ctrl and leave the text's own keys", () => {
    expect(refusedKey("send", ["s"])).not.toBeNull();
    expect(refusedKey("send", ["Mod+b"])).not.toBeNull();
    expect(refusedKey("zoomIn", ["Mod+z"])).not.toBeNull();
    expect(refusedKey("send", ["Mod+Shift+Enter"])).toBeNull();
    expect(refusedKey("toFrom", ["Mod+&", "Mod+1"])).toBeNull();
  });
});

describe("the key sets", () => {
  it("give no key to two things", () => {
    for (const set of ["standard", "classic"] as const) {
      const keys = everyKey(set, {});
      const owners = new Map<string, string>();
      for (const [id, names] of Object.entries(keys)) {
        for (const name of names) {
          const owner = owners.get(name);
          // Read and unread are one toggle's halves.
          if (owner && !(owner === "unread" && id === "read")) throw new Error(`${set}: ${name} is ${owner} and ${id}`);
          owners.set(name, id);
        }
      }
    }
  });
});
