import { describe, expect, it } from "vitest";
import { providerFor } from "./accounts";

describe("providerFor", () => {
  it("offers the provider's sign-in for its own addresses", () => {
    expect(providerFor("Ada@Hotmail.com")).toBe("microsoft");
    expect(providerFor("ada@outlook.fr")).toBe("microsoft");
    expect(providerFor("ada@gmail.com")).toBe("google");
  });

  it("knows nothing of other domains or of text that is no address", () => {
    expect(providerFor("ada@infomaniak.ch")).toBeNull();
    expect(providerFor("gmail.com")).toBeNull();
    expect(providerFor("")).toBeNull();
  });
});
