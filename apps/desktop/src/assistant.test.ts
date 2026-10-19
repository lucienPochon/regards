import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const { describePreview } = await import("./assistant");

describe("what the assistant would send", () => {
  it("says how much goes, where, and what is left out", () => {
    const preview = {
      messages: 2,
      characters: 1234,
      left_out: 0,
      destination: "api.anthropic.com",
      model: "claude-opus-5-5",
    };
    expect(describePreview(preview)).toBe("2 messages (1,234 characters) go to api.anthropic.com, for claude-opus-5-5.");
    const one = { ...preview, messages: 1, characters: 1, left_out: 1, destination: "localhost:11434" };
    expect(describePreview(one)).toBe(
      "1 message (1 character) goes to localhost:11434, for claude-opus-5-5. 1 message with a code is left out.",
    );
  });
});
