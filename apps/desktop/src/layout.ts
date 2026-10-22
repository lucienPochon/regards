import { useEffect, useState } from "react";

/** The window's layout, remembered on this computer. */
export interface Layout {
  /** "single": a message opens in place of the list; "split": the list and the message side by side. */
  mode: "single" | "split";
  /** The sidebar kept open beside the content; otherwise it is a rail of icons that opens over the content. */
  locked: boolean;
  /** The message list's width in split view, in pixels. */
  listWidth: number;
  /** The inbox by Smart Inbox section ("smart"), or simply newest first ("date"). */
  inboxView: "smart" | "date" | "cards";
  /** In place ("single"): a handle on the list's edge opens the reader beside it. */
  handle: boolean;
}

const KEY = "regards.layout";
export const LIST_MIN = 320;
export const LIST_MAX = 640;
const DEFAULT: Layout = { mode: "split", locked: true, listWidth: 460, inboxView: "smart", handle: true };

export const clampList = (width: number) => Math.round(Math.min(LIST_MAX, Math.max(LIST_MIN, width)));

function load(): Layout {
  try {
    const saved = JSON.parse(window.localStorage.getItem(KEY) ?? "null") as Partial<Layout> | null;
    return {
      mode: saved?.mode === "single" ? "single" : "split",
      locked: saved?.locked !== false,
      listWidth: typeof saved?.listWidth === "number" ? clampList(saved.listWidth) : DEFAULT.listWidth,
      inboxView: saved?.inboxView === "date" || saved?.inboxView === "cards" ? saved.inboxView : "smart",
      handle: saved?.handle !== false,
    };
  } catch {
    return DEFAULT;
  }
}

export function useLayout(): [Layout, (change: Partial<Layout>) => void] {
  const [layout, setLayout] = useState<Layout>(load);
  useEffect(() => {
    try {
      window.localStorage.setItem(KEY, JSON.stringify(layout));
    } catch {
      // Not remembered; the layout still applies now.
    }
  }, [layout]);
  return [layout, (change) => setLayout((current) => ({ ...current, ...change }))];
}
