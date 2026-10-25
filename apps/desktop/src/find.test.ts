// @vitest-environment jsdom
import { expect, it } from "vitest";
import { findRanges, markFound } from "./find";

it("finds every match, ignoring case, and not in scripts or styles", () => {
  const root = document.createElement("div");
  root.innerHTML = "<pre>Lunch at noon? <b>LUNCH</b> then</pre><script>lunch()</script><style>.lunch{}</style>";
  document.body.append(root);
  const found = findRanges(root, "lunch");
  expect(found.map((range) => range.toString())).toEqual(["Lunch", "LUNCH"]);
  expect(findRanges(root, "")).toEqual([]);
  expect(findRanges(root, "dinner")).toEqual([]);

  // Without the Custom Highlight API (as here), the current match is selected.
  markFound(found, 1);
  expect(window.getSelection()?.toString()).toBe("LUNCH");
  root.remove();
});
