import { type RefObject, useEffect, useRef } from "react";
import { type KeySet, getPrefs } from "./prefs";

/**
 * Keyboard shortcuts. Each component that offers some registers a layer; the
 * most recently shown layer that knows a key handles it. A modal layer (the
 * help panel, say) keeps keys from reaching the layers below it.
 *
 * Key names: a letter as typed ("j"), "Shift+U" for capitals, symbols as
 * typed ("#", "?"), named keys ("Escape", "ArrowDown", "Delete") and
 * "Mod+z" for Ctrl+Z (Cmd+Z on macOS). A digit key also answers to its
 * place on the keyboard: "Shift+1" and "Mod+Alt+1", whatever it types.
 *
 * Keys typed into a field are left alone, and so are combinations with Alt or
 * with Ctrl/Cmd other than our own "Mod+" ones. With single-key shortcuts off
 * (Settings › Accessibility), one-character keys are left alone too.
 */
export type Shortcuts = Partial<Record<string, () => void>>;

interface Layer {
  keys: RefObject<Shortcuts>;
  modal: boolean;
}

const layers: Layer[] = [];
const isMac = /Mac|iPhone|iPad/.test(navigator.platform);
/** How the platform names the key behind "Mod+": Ctrl, or ⌘ on macOS. */
export const MOD_KEY = isMac ? "⌘" : "Ctrl";

export function useShortcuts(shortcuts: Shortcuts, options: { modal?: boolean } = {}) {
  const keys = useRef(shortcuts);
  keys.current = shortcuts;
  const modal = options.modal ?? false;
  useEffect(() => {
    const layer = { keys, modal };
    layers.push(layer);
    if (layers.length === 1) window.addEventListener("keydown", onKeyDown);
    return () => {
      layers.splice(layers.indexOf(layer), 1);
      if (layers.length === 0) window.removeEventListener("keydown", onKeyDown);
    };
  }, [modal]);
}

function onKeyDown(event: KeyboardEvent) {
  if (event.defaultPrevented || event.isComposing || typing(event.target)) return;
  const singleKeys = getPrefs().singleKeys;
  const names = [keyName(event), placeName(event)].filter(
    (name): name is string => name !== null && (singleKeys || !characterKey(name)),
  );
  if (names.length === 0) return;
  for (let index = layers.length - 1; index >= 0; index--) {
    const layer = layers[index];
    const action = names.map((name) => layer.keys.current[name]).find(Boolean);
    if (action) {
      event.preventDefault();
      action();
      return;
    }
    if (layer.modal) return;
  }
}

/** A shortcut of one character, shifted or not ("e", "#", "Shift+R"): not Escape, arrows or "Mod+" ones. */
export const characterKey = (name: string) => /^(Shift\+)?.$/u.test(name);

export function keyName(event: KeyboardEvent): string | null {
  let key = event.key;
  // On many layouts (Swiss, German, French) "#" and others need AltGr, which
  // Windows reports as Ctrl+Alt: a symbol typed that way is a plain key.
  if (event.getModifierState("AltGraph") && key.length === 1 && !/^[a-z]$/i.test(key)) {
    return key;
  }
  const mod = isMac ? event.metaKey : event.ctrlKey;
  const foreign = isMac ? event.ctrlKey : event.metaKey;
  if (event.altKey || foreign) return null;
  if (/^[a-z]$/i.test(key)) {
    // Caps Lock alone does not make a capital.
    key = event.shiftKey ? `Shift+${key.toUpperCase()}` : key.toLowerCase();
  } else if (key.length > 1 && event.shiftKey) {
    key = `Shift+${key}`;
  }
  return mod ? `Mod+${key}` : key;
}

/**
 * A digit key by its place, not what it types: Shift+1 is "!" on an American
 * keyboard, "+" on a Swiss one and "1" on a French one, and Ctrl+Alt+3 is
 * AltGr+3 ("#") on Windows.
 */
export function placeName(event: KeyboardEvent): string | null {
  const digit = /^Digit(\d)$/.exec(event.code)?.[1];
  if (!digit) return null;
  const mod = isMac ? event.metaKey : event.ctrlKey;
  const foreign = isMac ? event.ctrlKey : event.metaKey;
  if (foreign) return null;
  if (mod && event.altKey && !event.shiftKey) return `Mod+Alt+${digit}`;
  if (event.shiftKey && !mod && !event.altKey) return `Shift+${digit}`;
  return null;
}

/** A key name as shown to people: "Mod" is Ctrl, or ⌘ on macOS. */
export const keyText = (name: string) => name.replace(/^Mod\+/, `${MOD_KEY}+`);

function typing(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName);
}

/** A line of the help panel; `ids`: the message actions whose keys it shows. */
interface Shortcut {
  keys: string;
  action: string;
  ids?: string[];
}

/**
 * For the help panel: the shortcuts of each key set, with the keys the
 * person chose for message actions (`mine`, by action) in place of the
 * set's; `keys` are every action's keys now, `labels` their names.
 */
export function shortcutList(
  set: KeySet,
  mine: Record<string, string[]> = {},
  keys: Record<string, string[]> = {},
  labels: Record<string, string> = {},
): { keys: string; action: string }[] {
  const changed = (id: string) => Array.isArray(mine[id]);
  const shown = (ids: string[]) =>
    [...new Set(ids.flatMap((id) => keys[id] ?? []))].map((name) => (name === " " ? "Space" : keyText(name))).join(" / ");
  const listed = new Set<string>();
  const lines: { keys: string; action: string }[] = [];
  for (const line of set === "classic" ? CLASSIC : STANDARD) {
    for (const id of line.ids ?? []) listed.add(id);
    if (!line.ids?.some(changed)) lines.push({ keys: line.keys, action: line.action });
    else if (shown(line.ids)) lines.push({ keys: shown(line.ids), action: line.action });
  }
  for (const id of Object.keys(mine)) {
    if (!listed.has(id) && shown([id])) lines.push({ keys: shown([id]), action: labels[id] ?? id });
  }
  return lines;
}

/**
 * Keys that do something other than a message action, by what they do:
 * not to be given to one (Settings › Shortcuts). `p` prints in the
 * standard set.
 */
export function takenKey(set: KeySet, name: string): string | null {
  if (set === "standard" && name === "p") return "Print the open message";
  return TAKEN[name] ?? null;
}

const TAKEN: Record<string, string> = {
  j: "Next message",
  k: "Previous message",
  ArrowDown: "Next message",
  ArrowUp: "Previous message",
  ArrowRight: "Next message",
  ArrowLeft: "Previous message",
  Enter: "Open the highlighted message",
  Escape: "Back to the list",
  Tab: "Moving between controls",
  "Shift+Tab": "Moving between controls",
  "/": "Show or fold the sidebar",
  "Mod+/": "Show or fold the sidebar",
  "Mod+k": "Commands",
  "Mod+f": "Search the list",
  "Mod+Shift+F": "Find in the open message",
  " ": "Preview the attachments",
  x: "Select the open message",
  "Mod+a": "Select every message",
  z: "Undo",
  c: "New message",
  o: "Fold the older messages",
  "Shift+O": "Open every message of the conversation",
  "Mod+g": "Go to Set aside",
  "Mod+d": "Go to Pinned",
  "Mod+s": "Go to Snoozed",
  "Mod+Shift+G": "Go to a folder",
  "Mod+Shift+U": "New folder",
  "Mod+r": "Check for new mail",
  "Mod+Enter": "Send",
  "Mod+Shift+T": "Insert a template",
  "Mod+e": "Show Done mail",
  "Mod+,": "Settings",
  "?": "Show these shortcuts",
};

const STANDARD: Shortcut[] = [
  { keys: `${MOD_KEY}+K`, action: "Commands: everything, by name" },
  { keys: "j / ↓ / →", action: "Next message" },
  { keys: "k / ↑ / ←", action: "Previous message" },
  { keys: "Enter", action: "Open the highlighted message" },
  { keys: "Esc", action: "Back to the list, or close the message" },
  { keys: `/ or ${MOD_KEY}+/`, action: "Show or fold the sidebar" },
  { keys: `${MOD_KEY}+F`, action: "Search the list" },
  { keys: `${MOD_KEY}+Shift+F`, action: "Find in the open message" },
  { keys: "Space", action: "Preview the open message's attachments; again to close" },
  { keys: "e", action: "Done (archive); a new sender is accepted too", ids: ["archive"] },
  { keys: "Delete / Backspace", action: "Delete (in Trash or Junk: for good, after confirming)", ids: ["delete"] },
  { keys: "i", action: "Flag or unflag", ids: ["flag"] },
  { keys: "d", action: "Pin or unpin", ids: ["pin"] },
  { keys: "s", action: "Snooze, or bring back a snoozed message", ids: ["snooze"] },
  { keys: "g", action: "Set aside, or bring back to the inbox", ids: ["aside"] },
  { keys: "u", action: "Mark read or unread", ids: ["read", "unread"] },
  { keys: "Shift+1", action: "Mark as junk", ids: ["junk"] },
  { keys: `${MOD_KEY}+Shift+M`, action: "Move to a folder", ids: ["move"] },
  { keys: "p", action: "Print the open message" },
  { keys: "l", action: "Labels: add or remove them (Gmail)", ids: ["label"] },
  { keys: "x", action: "Select or unselect the open message" },
  { keys: `${MOD_KEY}+A`, action: "Select every message in the list" },
  { keys: "z", action: "Undo, while the undo bar shows (also undo send)" },
  { keys: "c", action: "New message" },
  { keys: "Shift+R / r / f", action: "Reply, reply all, forward", ids: ["reply", "replyAll", "forward"] },
  { keys: "Shift+O / o", action: "Open every message of the conversation, or fold the older ones" },
  { keys: `${MOD_KEY}+G / ${MOD_KEY}+D / ${MOD_KEY}+S`, action: "Go to Set aside, Pinned, Snoozed" },
  { keys: `${MOD_KEY}+Shift+G`, action: "Go to a folder" },
  { keys: `${MOD_KEY}+Shift+U`, action: "New folder (inside the open one)" },
  { keys: `${MOD_KEY}+Alt+1 / 2 / 3`, action: "Inbox by priority, by category, by date" },
  { keys: `${MOD_KEY}+R`, action: "Check for new mail" },
  { keys: `${MOD_KEY}+Enter`, action: "Send, in the composer" },
  { keys: `${MOD_KEY}+Shift+T`, action: "Insert a template, in the composer" },
  { keys: `${MOD_KEY}+E`, action: "Show Done mail in the inbox, or hide it" },
  { keys: `${MOD_KEY}+,`, action: "Settings" },
  { keys: "?", action: "Show or hide these shortcuts" },
];

const CLASSIC: Shortcut[] = [
  { keys: `${MOD_KEY}+K`, action: "Commands: everything, by name" },
  { keys: "j / ↓", action: "Next message" },
  { keys: "k / ↑", action: "Previous message" },
  { keys: "Enter", action: "Open the highlighted message" },
  { keys: "Esc", action: "Back to the list, or close the message" },
  { keys: "/", action: "Show or fold the sidebar" },
  { keys: `${MOD_KEY}+F`, action: "Search the list" },
  { keys: `${MOD_KEY}+Shift+F`, action: "Find in the open message" },
  { keys: "Space", action: "Preview the open message's attachments; again to close" },
  { keys: "e", action: "Archive", ids: ["archive"] },
  { keys: "# / Delete", action: "Delete (in Trash or Junk: for good, after confirming)", ids: ["delete"] },
  { keys: "s", action: "Flag or unflag", ids: ["flag"] },
  { keys: "p", action: "Pin or unpin", ids: ["pin"] },
  { keys: "b", action: "Snooze, or bring back a snoozed message", ids: ["snooze"] },
  { keys: "Shift+R", action: "Remind me if nobody replies, or remove the reminder", ids: ["remind"] },
  { keys: "Shift+A", action: "Set aside, or bring back to the inbox", ids: ["aside"] },
  { keys: "v", action: "Move to a folder", ids: ["move"] },
  { keys: "Shift+U", action: "Mark unread", ids: ["unread"] },
  { keys: "Shift+I", action: "Mark read", ids: ["read"] },
  { keys: "l", action: "Labels: add or remove them (Gmail)", ids: ["label"] },
  { keys: "x", action: "Select or unselect the open message" },
  { keys: `${MOD_KEY}+A`, action: "Select every message in the list" },
  { keys: "z", action: "Undo, while the undo bar shows (also undo send)" },
  { keys: "c", action: "New message" },
  { keys: "r / a / f", action: "Reply, reply all, forward", ids: ["reply", "replyAll", "forward"] },
  { keys: "Shift+O / o", action: "Open every message of the conversation, or fold the older ones" },
  { keys: `${MOD_KEY}+Enter`, action: "Send, in the composer" },
  { keys: `${MOD_KEY}+Shift+T`, action: "Insert a template, in the composer" },
  { keys: `${MOD_KEY}+Shift+U`, action: "New folder (inside the open one)" },
  { keys: `${MOD_KEY}+E`, action: "Show Done mail in the inbox, or hide it" },
  { keys: `${MOD_KEY}+,`, action: "Settings" },
  { keys: "?", action: "Show or hide these shortcuts" },
];
