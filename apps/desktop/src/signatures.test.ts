import { describe, expect, it } from "vitest";
import { type Signature, signatureIn } from "./signatures";

const signature = (id: number, body: string): Signature => ({ id, name: `S${id}`, body, updated_at: 0 });

describe("signatures", () => {
  it("recognises a draft's signature by its text, the longest first", () => {
    const short = signature(1, "Ada");
    const formal = signature(2, "Ada Lovelace\nEngines Ltd");
    const list = [short, formal, signature(3, "  ")];
    expect(signatureIn("Hello\n\nAda Lovelace\nEngines Ltd", list)).toBe(formal);
    expect(signatureIn("Hello\n\nAda", list)).toBe(short);
    expect(signatureIn("Hello", list)).toBeNull();
  });
});
