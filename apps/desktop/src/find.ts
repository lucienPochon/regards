/**
 * Find in the open message: its matches, ignoring case, marked with the CSS
 * Custom Highlight API (the current one more strongly), or the current one
 * selected where that API is missing. An HTML message is searched inside its
 * frame by the frame's own script (mail_frame.rs), which works the same way.
 */

/** Matches looked for at most. */
const MAX = 1000;

/** The matches of `query` in the text under `root`, in document order. A match across two text nodes is not found. */
export function findRanges(root: Node, query: string): Range[] {
  const wanted = query.toLowerCase();
  const found: Range[] = [];
  if (!wanted) return found;
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node && found.length < MAX; node = walker.nextNode()) {
    // Not the words of a script or a style sheet, which do not show.
    if (/^(SCRIPT|STYLE|NOSCRIPT)$/.test(node.parentNode?.nodeName ?? "")) continue;
    const text = (node as Text).data.toLowerCase();
    for (let at = text.indexOf(wanted); at !== -1 && found.length < MAX; at = text.indexOf(wanted, at + wanted.length)) {
      const range = document.createRange();
      range.setStart(node, at);
      range.setEnd(node, at + wanted.length);
      found.push(range);
    }
  }
  return found;
}

/** Mark every match, and `at` more strongly; nothing for no matches. */
export function markFound(ranges: Range[], at: number): void {
  if (typeof CSS !== "undefined" && "highlights" in CSS && typeof Highlight !== "undefined") {
    CSS.highlights.set("regards-find", new Highlight(...ranges));
    CSS.highlights.set("regards-find-now", ranges[at] ? new Highlight(ranges[at]) : new Highlight());
    return;
  }
  const selection = window.getSelection();
  selection?.removeAllRanges();
  if (ranges[at]) selection?.addRange(ranges[at]);
}

export function clearFound(): void {
  if (typeof CSS !== "undefined" && "highlights" in CSS) {
    CSS.highlights.delete("regards-find");
    CSS.highlights.delete("regards-find-now");
  }
}

/**
 * Scroll the pane so that `top` to `bottom` (in `element`'s own coordinates)
 * is in view, a third of the way down, unless it is already.
 */
export function reveal(element: Element, top: number, bottom: number): void {
  let scroller = element.parentElement;
  while (scroller && !/(auto|scroll)/.test(getComputedStyle(scroller).overflowY)) scroller = scroller.parentElement;
  if (!scroller) return;
  const box = scroller.getBoundingClientRect();
  const origin = element.getBoundingClientRect().top;
  const [from, to] = [origin + top, origin + bottom];
  if (from >= box.top + 32 && to <= box.bottom - 32) return;
  scroller.scrollTop += from - box.top - box.height / 3;
}
