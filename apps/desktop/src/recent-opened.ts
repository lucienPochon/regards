import { useCallback, useEffect, useState } from "react";
import type { Person } from "./accounts";
import type { OpenLink } from "./links";

/** A message opened lately, to open again from the search page. */
export interface Opened {
  /** Its `regards://` link: by Message-ID when it has one, so it still opens once moved. */
  link: string;
  subject: string;
  from: Person | null;
  /** When it was opened, Unix ms. */
  at: number;
}

/** The last messages opened, newest first, remembered on this computer. */
const KEY = "regards.opened";
const KEEP = 8;
/** Told to every `useOpened` when the list changes. */
const CHANGED = "regards-opened";

function load(): Opened[] {
  try {
    const saved = JSON.parse(window.localStorage.getItem(KEY) ?? "[]") as unknown;
    if (!Array.isArray(saved)) return [];
    return saved
      .filter((entry): entry is Opened => typeof entry?.link === "string" && typeof entry?.subject === "string")
      .slice(0, KEEP);
  } catch {
    return [];
  }
}

function save(list: Opened[]) {
  try {
    window.localStorage.setItem(KEY, JSON.stringify(list));
  } catch {
    // Not remembered; nothing else depends on it.
  }
  window.dispatchEvent(new Event(CHANGED));
}

/** A message was opened: first in the list, once. */
export function rememberOpened(entry: Opened) {
  save([entry, ...load().filter((kept) => kept.link !== entry.link)].slice(0, KEEP));
}

/** Take a message off the list (it is no longer here, or the person said so). */
export function forgetOpened(link: string) {
  save(load().filter((kept) => kept.link !== link));
}

/** The messages opened lately, kept current. */
export function useOpened(): Opened[] {
  const [opened, setOpened] = useState<Opened[]>(load);
  const reload = useCallback(() => setOpened(load()), []);
  useEffect(() => {
    window.addEventListener(CHANGED, reload);
    return () => window.removeEventListener(CHANGED, reload);
  }, [reload]);
  return opened;
}

/** What a remembered link opens. */
export function linkOf(link: string): OpenLink | null {
  const byHeader = /^regards:\/\/message-id\/(.+)$/.exec(link);
  if (byHeader) return { kind: "message_id", message_id: decodeURIComponent(byHeader[1]) };
  const byRow = /^regards:\/\/message\/(\d+)$/.exec(link);
  return byRow ? { kind: "message", id: Number(byRow[1]) } : null;
}
