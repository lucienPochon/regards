import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const { KEYED_ACTIONS, keysFor } = await import("./actions");
const { shortcutList, takenKey } = await import("./shortcuts");

const labels = Object.fromEntries(KEYED_ACTIONS.map((one) => [one.id, one.label]));

describe("keys the person chose", () => {
  it("take the place of the key set's, and none is a choice too", () => {
    const keys = keysFor("standard", { archive: ["y"], flag: [] });
    expect(keys.archive).toEqual(["y"]);
    expect(keys.flag).toEqual([]);
    expect(keys.snooze).toEqual(["s"]);
  });

  it("show in the list of shortcuts, in place of the set's", () => {
    const mine = { archive: ["y"], replyAll: ["Shift+A"], mute: ["m"] };
    const list = shortcutList("standard", mine, keysFor("standard", mine), labels);
    expect(list.find((line) => line.action.startsWith("Done"))?.keys).toBe("y");
    expect(list.find((line) => line.action.startsWith("Reply,"))?.keys).toBe("Shift+R / Shift+A / f");
    expect(list.find((line) => line.action === "Mute the conversation")?.keys).toBe("m");
    // Unchanged lines stay as written.
    expect(list.find((line) => line.action === "Pin or unpin")?.keys).toBe("d");
  });

  it("are not those the app needs for something else", () => {
    expect(takenKey("standard", "j")).toBe("Next message");
    expect(takenKey("standard", "p")).not.toBeNull();
    expect(takenKey("classic", "p")).toBeNull();
    expect(takenKey("standard", "y")).toBeNull();
  });
});
