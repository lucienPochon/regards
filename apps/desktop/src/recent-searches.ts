import { useCallback, useState } from "react";

/** The last searches, newest first, remembered on this computer. */
const KEY = "regards.searches";
const KEEP = 8;

function load(): string[] {
  try {
    const saved = JSON.parse(window.localStorage.getItem(KEY) ?? "[]") as unknown;
    return Array.isArray(saved) ? saved.filter((entry): entry is string => typeof entry === "string").slice(0, KEEP) : [];
  } catch {
    return [];
  }
}

function save(searches: string[]) {
  try {
    window.localStorage.setItem(KEY, JSON.stringify(searches));
  } catch {
    // Not remembered; nothing else depends on it.
  }
}

/** Recent searches, and remembering or forgetting one. */
export function useRecentSearches(): [string[], (query: string) => void, (query: string) => void] {
  const [searches, setSearches] = useState<string[]>(load);
  const remember = useCallback((query: string) => {
    const clean = query.trim();
    if (!clean) return;
    setSearches((now) => {
      const next = [clean, ...now.filter((entry) => entry !== clean)].slice(0, KEEP);
      save(next);
      return next;
    });
  }, []);
  const forget = useCallback((query: string) => {
    setSearches((now) => {
      const next = now.filter((entry) => entry !== query);
      save(next);
      return next;
    });
  }, []);
  return [searches, remember, forget];
}
