import { afterEach, describe, expect, it, vi } from "vitest";
import { rustLicences } from "./licences";

const list = {
  generated: true,
  tool: "cargo-about 0.9.2",
  texts: [{ id: "MIT", name: "MIT License", text: "Permission is hereby granted…" }],
  crates: [{ name: "rustls", version: "0.23.46", licence: "Apache-2.0 OR ISC OR MIT", texts: [0] }],
};

describe("the Rust programs' licences", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("come from the list the release made", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: true, json: async () => list }));
    expect(await rustLicences()).toEqual(list);
  });

  it("are none in a build without the list", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: true, json: async () => ({ ...list, generated: false }) }));
    expect(await rustLicences()).toBeNull();
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: false }));
    expect(await rustLicences()).toBeNull();
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("offline")));
    expect(await rustLicences()).toBeNull();
  });
});
