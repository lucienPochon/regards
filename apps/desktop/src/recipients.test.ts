import { describe, expect, it } from "vitest";
import { typing, typingStart, withPicked } from "./recipients";

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

describe("picking a suggestion", () => {
  const lena = { name: "Lena Okafor", address: "lena.okafor@example.net" };

  it("replaces what was typed and leaves room for the next", () => {
    expect(withPicked("le", lena)).toBe("Lena Okafor <lena.okafor@example.net>, ");
  });

  it("keeps the addresses already there", () => {
    expect(withPicked("bob@example.net, le", lena)).toBe("bob@example.net, Lena Okafor <lena.okafor@example.net>, ");
    expect(withPicked("bob@example.net;le", lena)).toBe("bob@example.net, Lena Okafor <lena.okafor@example.net>, ");
  });

  it("writes a bare address without a name", () => {
    expect(withPicked("cy", { name: null, address: "cy@example.org" })).toBe("cy@example.org, ");
  });
});
