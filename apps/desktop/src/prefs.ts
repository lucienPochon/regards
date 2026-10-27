import { useSyncExternalStore } from "react";
import type { PresetPrefs } from "./snooze";

/**
 * How the person likes the app to behave, kept on this computer
 * (localStorage). Each default is how Regards behaved before the setting
 * existed, so nothing changes until the person changes it.
 */
export interface Prefs {
  /** After archiving or deleting the open message: the next one, the previous one, or the list. */
  afterMove: "next" | "previous" | "list";
  /** Archiving a pinned message: keep it pinned, or unpin it. */
  pinnedArchive: "keep" | "unpin";
  /** How long a sent message waits in the Outbox, to be called back (the daemon allows up to 30 s). */
  undoSeconds: number;
  /** In a conversation, show the older messages folded to one line. */
  collapseOlder: boolean;
  /** When an opened message is marked read: at once, after a few seconds, or only by hand. */
  markRead: MarkRead;
  /** Load images from the web in every message, without asking each time. */
  remoteImages: boolean;
  /** Rows of the list when it fills the window. */
  density: "comfortable" | "compact";
  /** Where people's pictures show. */
  avatars: "everywhere" | "reader" | "list" | "nowhere";
  /** Lines of the message's start under each row, beside the reader. */
  previewLines: 0 | 1 | 2;
  /** Each account in its own colour, around pictures and in the sidebar. */
  colourCoding: boolean;
  accountColours: Record<string, string>;
  /** A name for an account, shown in the app instead of its address. */
  accountTitles: Record<string, string>;
  /** Accounts whose mail stays out of the unified inbox (their own inbox still has it). */
  hiddenAccounts: string[];
  /** Messages the Priority card shows before "View all"; 0 for all. */
  priorityRows: number;
  /** Where the groups (Notifications, Newsletters, Pinned) sit: with today, or at their newest message's day. */
  groupsPosition: "today" | "chronological";
  /** Each group as one row, or its mail shown with everyone else's. */
  grouped: { notifications: boolean; newsletters: boolean; invitations: boolean; invitation_replies: boolean; pinned: boolean };
  /** Saved searches shown as groups of the Smart Inbox, by id. */
  searchGroups: number[];
  /** The account a new message is from, unless one is open or chosen; null: the one in view; "last": the last used. */
  defaultFrom: string | null;
  /** The account the last message was sent from, for defaultFrom "last". */
  lastFrom: string | null;
  /** Replies and forwards open over the conversation ("panel") or full size. */
  replyOpens: "panel" | "full";
  /** Once a reply or forward is sent, the conversation leaves the inbox (Done). */
  doneAfterReply: boolean;
  /** People suggested as an address is typed in To, Cc and Bcc. */
  suggestPeople: boolean;
  /** The whole window's size, for reading comfort. */
  zoom: number;
  /**
   * Shortcuts of one character ("e", "#", "Shift+R"). Off: only those with
   * Ctrl/Cmd and named keys (Escape, arrows) act, so speech input or a stray
   * key cannot archive or delete mail (WCAG 2.1.4).
   */
  singleKeys: boolean;
  /** Which keys do what (Settings › Shortcuts): the standard set, or the earlier Regards one. */
  keySet: KeySet;
  /**
   * Keys the person chose for message actions, by action (Settings ›
   * Shortcuts): in place of the key set's; an empty list for none.
   */
  myKeys: Record<string, string[]>;
  /** The language of the app (Settings › General): the system's, when Regards speaks it, or one chosen. */
  language: Language;
  /** The unread counts beside the folders. */
  sidebarCounts: boolean;
  /** The Agents button at the foot of the sidebar: recipes and the agents on this computer. */
  agentsButton: boolean;
  /** The quick choices of each time picker: shown or not, and at what time. */
  snoozePresets: PresetPrefs;
  sendPresets: PresetPrefs;
  reminderPresets: PresetPrefs;
}

/**
 * "standard": the keys desktop mail apps commonly share (E done, S snooze,
 * R reply all, Shift+R reply). "classic": the keys of earlier Regards versions.
 */
export type KeySet = "standard" | "classic";
/** "system": French on a French system, English otherwise. */
export type Language = "system" | "en" | "fr";

export type MarkRead = "now" | "2s" | "5s" | "never";

/** How long an opened message stays open before it is marked read; null: never. */
export const MARK_READ_AFTER: Record<MarkRead, number | null> = { now: 0, "2s": 2000, "5s": 5000, never: null };

/**
 * Passing a message with the keyboard (j/k beside the list) is not reading
 * it: marked read only once it stays open this long.
 */
export const STEP_DWELL = 1000;

export const DEFAULT_PREFS: Prefs = {
  afterMove: "next",
  pinnedArchive: "keep",
  undoSeconds: 10,
  collapseOlder: true,
  markRead: "now",
  remoteImages: false,
  density: "comfortable",
  avatars: "everywhere",
  previewLines: 1,
  colourCoding: false,
  accountColours: {},
  accountTitles: {},
  hiddenAccounts: [],
  priorityRows: 3,
  groupsPosition: "today",
  grouped: { notifications: true, newsletters: true, invitations: true, invitation_replies: true, pinned: true },
  searchGroups: [],
  defaultFrom: null,
  lastFrom: null,
  replyOpens: "panel",
  doneAfterReply: false,
  suggestPeople: true,
  zoom: 1,
  singleKeys: true,
  keySet: "standard",
  myKeys: {},
  // French on a French system, English otherwise, until one is chosen.
  language: "system",
  sidebarCounts: true,
  agentsButton: true,
  snoozePresets: {},
  sendPresets: {},
  reminderPresets: {},
};

/** The zoom steps offered in Settings and by Ctrl + / Ctrl −. */
export const ZOOMS = [0.9, 1, 1.1, 1.25, 1.5, 1.75, 2];

/** The next zoom step up (1) or down (-1) from where the window is. */
export function zoomStep(now: number, direction: 1 | -1): number {
  const index = ZOOMS.findIndex((zoom) => Math.abs(zoom - now) < 0.001);
  if (index === -1) return direction > 0 ? (ZOOMS.find((zoom) => zoom > now) ?? now) : ([...ZOOMS].reverse().find((zoom) => zoom < now) ?? now);
  return ZOOMS[Math.min(ZOOMS.length - 1, Math.max(0, index + direction))];
}

/** What each account colour is called, for screen readers and tooltips. */
export const COLOUR_NAMES: Record<string, string> = {
  "#1d7a61": "Green",
  "#3b7dd8": "Blue",
  "#8b5cc8": "Purple",
  "#c8507a": "Pink",
  "#d4891f": "Amber",
  "#2a9d9a": "Teal",
  "#66717f": "Slate",
  "#b5562f": "Rust",
};

/** The colours an account can take, readable on both themes. */
export const ACCOUNT_COLOURS = ["#1d7a61", "#3b7dd8", "#8b5cc8", "#c8507a", "#d4891f", "#2a9d9a", "#66717f", "#b5562f"];

const KEY = "regards.prefs";

function load(): Prefs {
  try {
    const saved = JSON.parse(window.localStorage.getItem(KEY) ?? "null") as Partial<Prefs> | null;
    return {
      ...DEFAULT_PREFS,
      ...(saved ?? {}),
      grouped: { ...DEFAULT_PREFS.grouped, ...(saved?.grouped ?? {}) },
    };
  } catch {
    return DEFAULT_PREFS;
  }
}

let current: Prefs = load();
const listeners = new Set<() => void>();

/** The preferences now, outside React (when an action runs). */
export const getPrefs = (): Prefs => current;

/** Change some preferences: kept, and every screen that reads them updates. */
export function setPrefs(change: Partial<Prefs>) {
  current = { ...current, ...change };
  try {
    window.localStorage.setItem(KEY, JSON.stringify(current));
  } catch {
    // Not kept for next time; it still applies now.
  }
  for (const listener of listeners) listener();
}

const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => listeners.delete(listener);
};

/** The preferences, kept current. */
export function usePrefs(): Prefs {
  return useSyncExternalStore(subscribe, getPrefs);
}

/** An account's name in the app: the one the person gave it, or its address. */
export const accountLabel = (prefs: Prefs, account: { id: string; email: string }) =>
  prefs.accountTitles[account.id]?.trim() || account.email;

/** A colour for an account nobody has chosen one for: the same one every time. */
const fallbackColour = (accountId: string) => {
  let hash = 0;
  for (const char of accountId) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
  return ACCOUNT_COLOURS[hash % ACCOUNT_COLOURS.length];
};

/** An account's colour when accounts are colour-coded; null otherwise. */
export const accountColour = (prefs: Prefs, accountId: string) =>
  prefs.colourCoding ? (prefs.accountColours[accountId] ?? fallbackColour(accountId)) : null;
