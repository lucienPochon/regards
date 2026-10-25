// @vitest-environment jsdom
import { afterEach, describe, expect, it } from "vitest";
import { FR } from "./i18n-fr";
import { UNDONE, fromService } from "./i18n-daemon";
import { setPrefs } from "./prefs";

afterEach(() => setPrefs({ language: "en" }));

describe("what the background service writes", () => {
  it("stays as written in English", () => {
    expect(fromService("Archived (3 messages)")).toBe("Archived (3 messages)");
  });

  it("says its usual texts in French", () => {
    setPrefs({ language: "fr" });
    expect(fromService("Archived")).toBe("Archivé");
    expect(fromService("Archived (3 messages)")).toBe("Archivé (3 messages)");
    expect(fromService("Moved to Projects")).toBe("Déplacé dans Projects");
    expect(fromService("Marked 1 message read")).toBe("1 message marqué comme lu");
    expect(fromService("Marked 4 messages unread")).toBe("4 messages marqués comme non lus");
    expect(fromService("Send “Plans” to Ada, Bob and 3 others")).toBe("Envoyer « Plans » à Ada, Bob et 3 autres");
    expect(fromService("Delete 12 messages for good")).toBe("Supprimer définitivement 12 messages");
    // Anything else, as written.
    expect(fromService("Something new")).toBe("Something new");
  });

  it("has a French word for every undo word", () => {
    for (const word of UNDONE) expect(FR[`undo|${word}`], word).toBeTruthy();
  });
});
