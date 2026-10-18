import { useEffect, useState } from "react";

/** The window's layout, remembered on this computer. */
export interface Layout {
  /** Folder names hidden (icons only); null follows the window's width. */
  rail: boolean | null;
  /** The message column's width in pixels. */
  listWidth: number;
}

const KEY = "regards.layout";
export const LIST_MIN = 300;
export const LIST_MAX = 620;
const DEFAULT: Layout = { rail: null, listWidth: 380 };

export const clampList = (width: number) => Math.round(Math.min(LIST_MAX, Math.max(LIST_MIN, width)));

function load(): Layout {
  try {
    const saved = JSON.parse(window.localStorage.getItem(KEY) ?? "null") as Partial<Layout> | null;
    return {
      rail: typeof saved?.rail === "boolean" ? saved.rail : null,
      listWidth: typeof saved?.listWidth === "number" ? clampList(saved.listWidth) : DEFAULT.listWidth,
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

export function useMediaQuery(query: string): boolean {
  const [matches, setMatches] = useState(() => window.matchMedia(query).matches);
  useEffect(() => {
    const list = window.matchMedia(query);
    const update = () => setMatches(list.matches);
    update();
    list.addEventListener("change", update);
    return () => list.removeEventListener("change", update);
  }, [query]);
  return matches;
}
