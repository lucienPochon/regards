// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { forgetOpened, linkOf, rememberOpened } from "./recent-opened";

const opened = (link: string, at: number) => ({ link, subject: `About ${at}`, from: null, at });
const kept = () => (JSON.parse(window.localStorage.getItem("regards.opened") ?? "[]") as { link: string }[]).map((entry) => entry.link);

describe("recently opened messages", () => {
  it("keeps the last eight, newest first, each once", () => {
    for (let n = 1; n <= 10; n++) rememberOpened(opened(`regards://message/${n}`, n));
    rememberOpened(opened("regards://message/5", 11));
    expect(kept()).toHaveLength(8);
    expect(kept()[0]).toBe("regards://message/5");
    expect(kept().filter((link) => link === "regards://message/5")).toHaveLength(1);
    forgetOpened("regards://message/5");
    expect(kept()).not.toContain("regards://message/5");
  });

  it("opens by Message-ID, or by row", () => {
    expect(linkOf("regards://message-id/CA%2Babc%2F1%3Dx@mail.example.test")).toEqual({
      kind: "message_id",
      message_id: "CA+abc/1=x@mail.example.test",
    });
    expect(linkOf("regards://message/42")).toEqual({ kind: "message", id: 42 });
    expect(linkOf("https://example.test")).toBeNull();
  });
});
