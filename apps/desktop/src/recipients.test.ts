import { describe, expect, it } from "vitest";
import { fieldText, isAddress, marked, pastedList, splitTyped, typing, typingStart, withAdded } from "./recipients";

describe("the address being typed", () => {
  it("is the whole field when there is one", () => {
    expect(typing("  len")).toBe("len");
    expect(typingStart("len")).toBe(0);
  });

  it("is what follows the last comma or semicolon", () => {
    expect(typing("Lena Okafor <lena@example.net>, bo")).toBe("bo");
    expect(typing("a@example.org; cy")).toBe("cy");
  });

  it("ignores commas inside quotes and angle brackets", () => {
    expect(typing('"Okafor, Lena" <lena@example.net>')).toBe('"Okafor, Lena" <lena@example.net>');
    expect(typing('"Okafor, Lena" <lena@example.net>, ma')).toBe("ma");
  });
});

describe("a field's text", () => {
  const lena = { name: "Lena Okafor", address: "lena.okafor@example.net" };
  const bob = { name: null, address: "bob@example.net" };

  it("splits into the people written out and what is still being typed", () => {
    expect(splitTyped("bob@example.net, Lena Okafor <lena.okafor@example.net>; cy")).toEqual({
      people: [bob, lena],
      rest: " cy",
    });
    expect(splitTyped('"Okafor, Lena" <lena@example.net>')).toEqual({ people: [], rest: '"Okafor, Lena" <lena@example.net>' });
  });

  it("is written back from its people and what is typed", () => {
    expect(fieldText([bob, lena], "cy")).toBe("bob@example.net, Lena Okafor <lena.okafor@example.net>, cy");
    expect(fieldText([bob], "")).toBe("bob@example.net");
    expect(fieldText([], "cy")).toBe("cy");
  });

  it("does not take the same address twice", () => {
    expect(withAdded([bob], [{ name: "Bob", address: "BOB@example.net" }, lena])).toEqual([bob, lena]);
  });
});

describe("what is an address", () => {
  it("is one address alone, with a dot in its domain", () => {
    expect(isAddress("lena@example.net")).toBe(true);
    expect(isAddress("lena@localhost")).toBe(false);
    expect(isAddress("Lena <lena@example.net>")).toBe(false);
    expect(isAddress("lena@example.net, bob@example.net")).toBe(false);
  });
});

describe("pasted text", () => {
  it("turns lines, tabs and spaces between bare addresses into a list", () => {
    expect(pastedList("a@example.org\r\nb@example.org\n")).toBe("a@example.org, b@example.org");
    expect(pastedList("a@example.org\tb@example.org")).toBe("a@example.org, b@example.org");
    expect(pastedList("a@example.org b@example.org")).toBe("a@example.org, b@example.org");
    expect(pastedList("a@example.org, b@example.org")).toBe("a@example.org, b@example.org");
  });

  it("leaves names with their addresses as they are", () => {
    expect(pastedList("Lena Okafor <lena@example.net>")).toBe("Lena Okafor <lena@example.net>");
  });
});

describe("what is marked in a suggestion", () => {
  const mark = (text: string, typed: string) =>
    marked(text, typed)
      .map((piece) => (piece.mark ? `[${piece.text}]` : piece.text))
      .join("");

  it("marks the start of each word that was typed, accents and case aside", () => {
    expect(mark("Hélène Dupré", "hel")).toBe("[Hél]ène Dupré");
    expect(mark("Hélène Dupré", "dupre HE")).toBe("[Hé]lène [Dupré]");
    expect(mark("lena.okafor@example.net", "okaf")).toBe("lena.[okaf]or@example.net");
  });

  it("marks nothing when nothing starts that way", () => {
    expect(mark("Lena Okafor", "fo")).toBe("Lena Okafor");
    expect(mark("Lena Okafor", "")).toBe("Lena Okafor");
  });
});
