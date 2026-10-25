import { type RefObject, useEffect, useRef, useState } from "react";
import { clearFound, findRanges, markFound, reveal } from "./find";
import { Icon } from "./icons";
import { t } from "./i18n";

/**
 * Find in the open message (its ⋯ menu, or Ctrl+Shift+F): how many matches
 * and which one, the next (Enter, ↓) or previous (Shift+Enter, ↑); Escape
 * closes. An HTML message is searched by its frame's script, which says how
 * many it found and where; a text one here.
 */
export function FindBar({
  frame,
  text,
  onClose,
}: {
  /** The HTML message's frame, when it is shown. */
  frame: RefObject<HTMLIFrameElement | null>;
  /** The text message, when it is shown instead. */
  text: RefObject<HTMLElement | null>;
  onClose: () => void;
}) {
  const [query, setQuery] = useState("");
  const [found, setFound] = useState<{ count: number; at: number } | null>(null);
  const ranges = useRef<Range[]>([]);

  // The frame's answers: how many, which one, and where it is in the frame.
  useEffect(() => {
    const heard = (event: MessageEvent) => {
      const shown = frame.current;
      if (!shown || event.source !== shown.contentWindow) return;
      const data = (event.data as { regardsMailFound?: Record<string, unknown> } | null)?.regardsMailFound;
      if (!data || typeof data.count !== "number" || typeof data.at !== "number") return;
      setFound({ count: data.count, at: data.at });
      if (data.count > 0 && typeof data.top === "number" && typeof data.bottom === "number") reveal(shown, data.top, data.bottom);
    };
    window.addEventListener("message", heard);
    return () => window.removeEventListener("message", heard);
  }, [frame]);

  // Marks go when the bar does.
  useEffect(
    () => () => {
      clearFound();
      frame.current?.contentWindow?.postMessage({ regardsMailFind: { query: "", step: 0 } }, "*");
    },
    [frame],
  );

  /** Show match `at` of the text message's, and mark them all. */
  const showText = (all: Range[], at: number) => {
    markFound(all, at);
    setFound({ count: all.length, at });
    const root = text.current;
    const range = all[at];
    if (root && range) {
      const box = range.getBoundingClientRect();
      const origin = root.getBoundingClientRect().top;
      reveal(root, box.top - origin, box.bottom - origin);
    }
  };

  const search = (words: string) => {
    setQuery(words);
    const window = frame.current?.contentWindow;
    if (window) {
      // Its frame's origin is its own (sandboxed): "*" is the only way to reach it.
      window.postMessage({ regardsMailFind: { query: words, step: 0 } }, "*");
      return;
    }
    ranges.current = text.current && words ? findRanges(text.current, words) : [];
    showText(ranges.current, 0);
  };

  const step = (by: number) => {
    if (!query) return;
    const window = frame.current?.contentWindow;
    if (window) {
      window.postMessage({ regardsMailFind: { query, step: by } }, "*");
      return;
    }
    const all = ranges.current;
    if (all.length > 0) showText(all, ((found?.at ?? 0) + by + all.length) % all.length);
  };

  return (
    <div className="find-bar" role="search">
      <input
        autoFocus
        value={query}
        placeholder={t("Find in this message")}
        aria-label={t("Find in this message")}
        onChange={(event) => search(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            step(event.shiftKey ? -1 : 1);
          } else if (event.key === "Escape") {
            event.preventDefault();
            event.stopPropagation();
            onClose();
          }
        }}
      />
      <span className="find-count muted small" role="status">
        {query && found ? (found.count === 0 ? t("Not found") : t("{at} of {count}", { at: found.at + 1, count: `${found.count}${found.count >= 1000 ? "+" : ""}` })) : ""}
      </span>
      <button type="button" className="icon-btn small find-previous" aria-label={t("Previous match")} title={t("Previous (Shift+Enter)")} onClick={() => step(-1)}>
        <Icon name="back" size={14} />
      </button>
      <button type="button" className="icon-btn small find-next" aria-label={t("Next match")} title={t("Next (Enter)")} onClick={() => step(1)}>
        <Icon name="back" size={14} />
      </button>
      <button type="button" className="icon-btn small" aria-label={t("Close find")} title={t("Close (Esc)")} onClick={onClose}>
        <Icon name="close" size={14} />
      </button>
    </div>
  );
}
