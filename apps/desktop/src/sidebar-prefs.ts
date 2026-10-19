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
}

const KEY = "regards.sidebar";
const RECENTS = 5;
const EMPTY: SidebarPrefs = { starred: [], recents: [], hidden: [] };

function load(): SidebarPrefs {
  try {
    const saved = JSON.parse(window.localStorage.getItem(KEY) ?? "null") as Partial<SidebarPrefs> | null;
    return {
      starred: Array.isArray(saved?.starred) ? saved.starred : [],
      recents: Array.isArray(saved?.recents) ? saved.recents : [],
      hidden: Array.isArray(saved?.hidden) ? saved.hidden.filter((key) => typeof key === "string") : [],
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
  };
  return [prefs, changes];
}
