// @vitest-environment jsdom
import { afterEach, describe, expect, it } from "vitest";
import { FR } from "./i18n-fr";
import { t, tc, tn } from "./i18n";
import { setPrefs } from "./prefs";

afterEach(() => setPrefs({ language: "en" }));

describe("the app's words", () => {
  it("are the English text until another language is chosen, with their names filled in", () => {
    expect(t("Saved: “{name}” is in the sidebar", { name: "Receipts" })).toBe("Saved: “Receipts” is in the sidebar");
    expect(tn(1, "1 message", "{count} messages")).toBe("1 message");
    expect(tn(1200, "1 message", "{count} messages")).toMatch(/^1.?200 messages$/);
  });

  it("are French where the table has them, English where it does not yet", () => {
    setPrefs({ language: "fr" });
    expect(t("Mark read")).toBe(FR["Mark read"]);
    expect(t("A sentence nobody translated")).toBe("A sentence nobody translated");
  });

  it("count as French does: 0 and 1 are singular", () => {
    setPrefs({ language: "fr" });
    const forms = FR["{count} messages selected"];
    expect(typeof forms).toBe("object");
    expect(tn(0, "1 message selected", "{count} messages selected")).toBe("0 message sélectionné");
    expect(tn(1, "1 message selected", "{count} messages selected")).toBe("1 message sélectionné");
    expect(tn(3, "1 message selected", "{count} messages selected")).toBe("3 messages sélectionnés");
  });
});

describe("a word English uses for two things", () => {
  it("is told apart by its context", () => {
    expect(tc("folder", "Archive")).toBe("Archive");
    setPrefs({ language: "fr" });
    expect(tc("folder", "Archive")).toBe("Archives");
    expect(t("Archive")).toBe("Archiver");
  });
});

describe("the French table", () => {
  const names = (text: string) => new Set([...text.matchAll(/\{(\w+)\}/g)].map((match) => match[1]));
  it("keeps the names of the English text: each one, and no other", () => {
    for (const [english, french] of Object.entries(FR)) {
      const wanted = names(english);
      for (const form of typeof french === "string" ? [french] : [french.one, french.other]) {
        const used = names(form);
        for (const name of used) expect(wanted.has(name), `${english} → ${form}: {${name}}`).toBe(true);
        for (const name of wanted) if (name !== "count" || typeof french === "string") expect(used.has(name), `${english} → ${form}: no {${name}}`).toBe(true);
      }
    }
  });
});
