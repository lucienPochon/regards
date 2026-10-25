import { useEffect, useState } from "react";
import { type View, viewKey } from "./views";

/** What the person changed in the sidebar, remembered on this computer. */
export interface SidebarPrefs {
  /** Folders added from the All folders panel (the star), in the order added. */
  starred: View[];
  /** Views opened from the panel without starring them, newest first. */
  recents: View[];
  /** Standard rows taken out of the sidebar (by view key); they stay in the panel. */
  hidden: string[];
  /** Rows the person put in another order (by view key), each section's together. */
  order: string[];
}

const KEY = "regards.sidebar";
const RECENTS = 5;
const EMPTY: SidebarPrefs = { starred: [], recents: [], hidden: [], order: [] };

function load(): SidebarPrefs {
  try {
    const saved = JSON.parse(window.localStorage.getItem(KEY) ?? "null") as Partial<SidebarPrefs> | null;
    return {
      starred: Array.isArray(saved?.starred) ? saved.starred : [],
      recents: Array.isArray(saved?.recents) ? saved.recents : [],
      hidden: Array.isArray(saved?.hidden) ? saved.hidden.filter((key) => typeof key === "string") : [],
      order: Array.isArray(saved?.order) ? saved.order.filter((key) => typeof key === "string") : [],
    };
  } catch {
    return EMPTY;
  }
}

export interface SidebarChanges {
  star: (view: View) => void;
  unstar: (view: View) => void;
  /** Opened from the panel: listed under Recents unless it is already in the sidebar. */
  visit: (view: View) => void;
  forget: (view: View) => void;
  clearRecents: () => void;
  hide: (view: View) => void;
  show: (view: View) => void;
  /** A section's rows (by view key), in their new order. */
  arrange: (keys: string[]) => void;
}

/** Rows in the order the person put them; any never moved keep their place after those. */
export function arranged<T>(rows: T[], keyOf: (row: T) => string, order: string[]): T[] {
  const place = (row: T, index: number) => {
    const at = order.indexOf(keyOf(row));
    return at === -1 ? order.length + index : at;
  };
  return rows
    .map((row, index) => ({ row, place: place(row, index) }))
    .sort((one, other) => one.place - other.place)
    .map((entry) => entry.row);
}

export function useSidebarPrefs(): [SidebarPrefs, SidebarChanges] {
  const [prefs, setPrefs] = useState<SidebarPrefs>(load);
  useEffect(() => {
    try {
      window.localStorage.setItem(KEY, JSON.stringify(prefs));
    } catch {
      // Not remembered; it still applies now.
    }
  }, [prefs]);

  const without = (list: View[], view: View) => list.filter((other) => viewKey(other) !== viewKey(view));
  const changes: SidebarChanges = {
    star: (view) =>
      setPrefs((now) => ({
        ...now,
        starred: [...without(now.starred, view), view],
        recents: without(now.recents, view),
        hidden: now.hidden.filter((key) => key !== viewKey(view)),
      })),
    unstar: (view) => setPrefs((now) => ({ ...now, starred: without(now.starred, view) })),
    visit: (view) =>
      setPrefs((now) =>
        now.starred.some((other) => viewKey(other) === viewKey(view))
          ? now
          : { ...now, recents: [view, ...without(now.recents, view)].slice(0, RECENTS) },
      ),
    forget: (view) => setPrefs((now) => ({ ...now, recents: without(now.recents, view) })),
    clearRecents: () => setPrefs((now) => ({ ...now, recents: [] })),
    hide: (view) =>
      setPrefs((now) => ({
        ...now,
        starred: without(now.starred, view),
        hidden: [...now.hidden.filter((key) => key !== viewKey(view)), viewKey(view)],
      })),
    show: (view) => setPrefs((now) => ({ ...now, hidden: now.hidden.filter((key) => key !== viewKey(view)) })),
    arrange: (keys) => setPrefs((now) => ({ ...now, order: [...keys, ...now.order.filter((key) => !keys.includes(key))] })),
  };
  return [prefs, changes];
}
