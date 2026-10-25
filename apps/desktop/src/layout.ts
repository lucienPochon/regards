import { useEffect, useState } from "react";

/** The window's layout, remembered on this computer. */
export interface Layout {
  /** "single": a message opens in place of the list; "split": the list and the message side by side. */
  mode: "single" | "split";
  /** The sidebar kept open beside the content; otherwise it is a rail of icons that opens over the content. */
  locked: boolean;
  /** The message list's width in split view, in pixels. */
  listWidth: number;
  /** The open sidebar's width, in pixels. */
  sidebarWidth: number;
  /** The inbox by Smart Inbox section ("smart"), or simply newest first ("date"). */
  inboxView: "smart" | "date" | "cards";
  /** In place ("single"): a handle on the list's edge opens the reader beside it. */
  handle: boolean;
}

export const LAYOUT_KEY = "regards.layout";
/**
 * Bumped when what was saved must start over. 2: messages open beside the list
 * again, whatever was saved before; "in place" is chosen anew in Settings.
 */
export const LAYOUT_VERSION = 2;
export const LIST_MIN = 320;
export const LIST_MAX = 640;
const DEFAULT: Layout = { mode: "split", locked: true, listWidth: 460, sidebarWidth: 238, inboxView: "date", handle: true };

export const clampList = (width: number) => Math.round(Math.min(LIST_MAX, Math.max(LIST_MIN, width)));
export const SIDE_MIN = 200;
export const SIDE_MAX = 360;
export const clampSide = (width: number) => Math.round(Math.min(SIDE_MAX, Math.max(SIDE_MIN, width)));

function load(): Layout {
  try {
    const saved = JSON.parse(window.localStorage.getItem(LAYOUT_KEY) ?? "null") as (Partial<Layout> & { version?: number }) | null;
    return {
      mode: saved?.version === LAYOUT_VERSION && saved.mode === "single" ? "single" : "split",
      locked: saved?.locked !== false,
      listWidth: typeof saved?.listWidth === "number" ? clampList(saved.listWidth) : DEFAULT.listWidth,
      sidebarWidth: typeof saved?.sidebarWidth === "number" ? clampSide(saved.sidebarWidth) : DEFAULT.sidebarWidth,
      inboxView:
        saved?.inboxView === "smart" || saved?.inboxView === "cards" || saved?.inboxView === "date"
          ? saved.inboxView
          : DEFAULT.inboxView,
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
      window.localStorage.setItem(LAYOUT_KEY, JSON.stringify({ ...layout, version: LAYOUT_VERSION }));
    } catch {
      // Not remembered; the layout still applies now.
    }
  }, [layout]);
  return [layout, (change) => setLayout((current) => ({ ...current, ...change }))];
}
