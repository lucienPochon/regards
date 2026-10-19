import { describe, expect, it } from "vitest";
import { zoomStep } from "./prefs";
import { characterKey } from "./shortcuts";

describe("single-key shortcuts", () => {
  it("are one character, shifted or not", () => {
    for (const name of ["e", "#", "?", "/", "Shift+R", "Shift+A"]) expect(characterKey(name)).toBe(true);
  });

  it("leave out named keys and Ctrl/Cmd combinations", () => {
    for (const name of ["Escape", "ArrowDown", "Delete", "Enter", "Mod+k", "Mod+,", "Mod+Shift+T", "Shift+Delete"]) {
      expect(characterKey(name)).toBe(false);
    }
  });
});

describe("zoom steps", () => {
  it("go up and down the offered steps, and stop at the ends", () => {
    expect(zoomStep(1, 1)).toBe(1.1);
    expect(zoomStep(1.5, 1)).toBe(1.75);
    expect(zoomStep(2, 1)).toBe(2);
    expect(zoomStep(1, -1)).toBe(0.9);
    expect(zoomStep(0.9, -1)).toBe(0.9);
  });

  it("from a zoom between steps, go to the nearest step in that direction", () => {
    expect(zoomStep(1.3, 1)).toBe(1.5);
    expect(zoomStep(1.3, -1)).toBe(1.25);
  });
});
