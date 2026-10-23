import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const { blockableDomain } = await import("./smart");

describe("blocking a whole domain", () => {
  it("is offered for a company's or a service's own domain", () => {
    expect(blockableDomain("tickets@northline.example")).toBe("@northline.example");
    expect(blockableDomain(" News@Mail.Shop.Example ")).toBe("@mail.shop.example");
  });

  it("is not offered for providers anyone can have an address at", () => {
    for (const address of ["someone@gmail.com", "a@outlook.com", "b@icloud.com", "c@bluewin.ch", "d@gmx.ch"]) {
      expect(blockableDomain(address)).toBe(null);
    }
  });

  it("needs a real domain", () => {
    expect(blockableDomain("nobody")).toBe(null);
    expect(blockableDomain("x@localhost")).toBe(null);
  });
});
