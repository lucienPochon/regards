import { describe, expect, it } from "vitest";
import { arranged } from "./sidebar-prefs";

const rows = ["pinned", "drafts", "sent", "trash", "outbox"];
const same = (row: string) => row;

describe("the sidebar's rows", () => {
  it("keep their order until moved", () => {
    expect(arranged(rows, same, [])).toEqual(rows);
  });

  it("follow the order the person gave them; rows never moved come after", () => {
    expect(arranged(rows, same, ["sent", "pinned", "drafts", "trash", "outbox"])).toEqual(["sent", "pinned", "drafts", "trash", "outbox"]);
    // A folder starred later goes last; other sections' keys change nothing.
    expect(arranged(["archive", "receipts", "travel"], same, ["receipts", "archive", "sent"])).toEqual(["receipts", "archive", "travel"]);
  });
});
