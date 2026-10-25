import { type RefObject, useEffect, useRef } from "react";
import { spoken, t } from "./i18n";
import { type KeySet, getPrefs } from "./prefs";

/**
 * Keyboard shortcuts. Each component that offers some registers a layer; the
 * most recently shown layer that knows a key handles it. A modal layer (the
 * help panel, say) keeps keys from reaching the layers below it.
 *
 * A layer names commands ("next", "search": see COMMANDS), which answer to
 * their keys now (the key set's, or the person's: Settings › Shortcuts), or
 * keys that never change ("Escape", "ArrowDown", see FIXED). Message actions
 * come with their keys already (actions.ts).
 *
 * Key names: a letter as typed ("j"), "Shift+U" for capitals, symbols as
 * typed ("#", "?"), named keys ("Escape", "ArrowDown", "Delete") and
 * "Mod+z" for Ctrl+Z (Cmd+Z on macOS). A digit key also answers to its
 * place on the keyboard: "Shift+1", "Mod+1" and "Mod+Alt+1", whatever it types.
 *
 * Keys typed into a field are left alone, and so are combinations with Alt or
 * with Ctrl/Cmd other than our own "Mod+" ones. With single-key shortcuts off
 * (Settings › Accessibility), one-character keys are left alone too.
 */
export type Shortcuts = Partial<Record<CommandId | string, () => void>>;

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
  const names = pressedNames(event).filter((name) => singleKeys || !characterKey(name));
  if (names.length === 0) return;
  const keys = currentKeys();
  const wanted = [...names, ...COMMANDS.filter(({ id }) => keys[id].some((key) => names.includes(key))).map(({ id }) => id)];
  for (let index = layers.length - 1; index >= 0; index--) {
    const layer = layers[index];
    const action = wanted.map((name) => layer.keys.current[name]).find(Boolean);
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

/** Keys named by their place on an American keyboard, whatever they type here. */
const PLACES: Record<string, string> = { Backslash: "\\" };

/**
 * A digit key by its place, not what it types: Shift+1 is "!" on an American
 * keyboard, "+" on a Swiss one and "1" on a French one, and Ctrl+Alt+3 is
 * AltGr+3 ("#") on Windows. The backslash key too, for Ctrl+Shift+\.
 */
export function placeName(event: KeyboardEvent): string | null {
  const place = /^Digit(\d)$/.exec(event.code)?.[1] ?? PLACES[event.code];
  if (!place) return null;
  const mod = isMac ? event.metaKey : event.ctrlKey;
  const foreign = isMac ? event.ctrlKey : event.metaKey;
  if (foreign) return null;
  if (mod && event.altKey && !event.shiftKey) return `Mod+Alt+${place}`;
  if (event.altKey) return null;
  if (mod) return event.shiftKey ? `Mod+Shift+${place}` : `Mod+${place}`;
  return event.shiftKey ? `Shift+${place}` : null;
}

/**
 * A key pressed by its names, the one to keep first: its place for a digit
 * ("Shift+1" on any layout), but a symbol typed with AltGr as typed ("#").
 */
export function pressedNames(event: KeyboardEvent): string[] {
  const typed = keyName(event);
  const place = placeName(event);
  const names = event.getModifierState("AltGraph") ? [typed, place] : [place, typed];
  return [...new Set(names.filter((name): name is string => name !== null))];
}

/** Keys named as French keyboards name them. */
const FRENCH_KEYS: Record<string, string> = {
  Shift: "Maj",
  Delete: "Suppr",
  Backspace: "Retour arrière",
  Escape: "Échap",
  Esc: "Échap",
  Enter: "Entrée",
  Space: "Espace",
};

/** Key names in a text, as the app's language names them. */
const localKeys = (text: string) =>
  spoken() === "fr" ? text.replace(/\b(Shift|Delete|Backspace|Escape|Esc|Enter|Space)\b/g, (key) => FRENCH_KEYS[key]) : text;

/** Keys shown by a sign or a shorter name. */
const SHOWN: Record<string, string> = { " ": "Space", ArrowDown: "↓", ArrowUp: "↑", ArrowRight: "→", ArrowLeft: "←", Escape: "Esc" };

/**
 * A key name as shown to people: "Mod" is Ctrl, or ⌘ on macOS, with a capital
 * letter ("Ctrl+K"); arrows as arrows; in French, Maj, Suppr and the like.
 */
export const keyText = (name: string) =>
  localKeys((SHOWN[name] ?? name).replace(/^Mod\+(.)$/u, (_, key: string) => `Mod+${key.toUpperCase()}`).replace(/^Mod\+/, `${MOD_KEY}+`));

function typing(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName);
}

export type CommandId =
  | "next"
  | "previous"
  | "commands"
  | "search"
  | "find"
  | "sidebar"
  | "goAside"
  | "goPinned"
  | "goSnoozed"
  | "goFolder"
  | "inboxPriority"
  | "inboxCategory"
  | "inboxDate"
  | "showDone"
  | "select"
  | "selectAll"
  | "openAll"
  | "foldOlder"
  | "preview"
  | "print"
  | "undo"
  | "compose"
  | "send"
  | "sendDone"
  | "attach"
  | "template"
  | "discard"
  | "toFrom"
  | "toSubject"
  | "toTo"
  | "toCc"
  | "toBcc"
  | "toText"
  | "newFolder"
  | "sync"
  | "zoomIn"
  | "zoomOut"
  | "zoomReset"
  | "settings"
  | "help"
  | "quit";

/** Where a command belongs in Settings › Shortcuts. Message actions have their own part. */
export type CommandPart = "moving" | "messages" | "writing" | "app";

interface Command {
  id: CommandId;
  label: string;
  part: CommandPart;
  /** Its keys in each key set. */
  keys: Record<KeySet, string[]>;
  /** It acts while typing (in the composer, or zoom from anywhere): its keys need Ctrl/⌘. */
  typing?: true;
}

const both = (...keys: string[]): Record<KeySet, string[]> => ({ standard: keys, classic: keys });
const standardOnly = (...keys: string[]): Record<KeySet, string[]> => ({ standard: keys, classic: [] });

/** Everything a key does other than a message action, as Settings › Shortcuts names it. */
export const COMMANDS: Command[] = [
  { id: "next", label: t("Next message"), part: "moving", keys: both("j") },
  { id: "previous", label: t("Previous message"), part: "moving", keys: both("k") },
  { id: "commands", label: t("Commands"), part: "moving", keys: both("Mod+k") },
  { id: "search", label: t("Search the list"), part: "moving", keys: both("Mod+f") },
  { id: "find", label: t("Find in the open message"), part: "moving", keys: both("Mod+Shift+F") },
  { id: "sidebar", label: t("Show or fold the sidebar"), part: "moving", keys: { standard: ["/", "Mod+/"], classic: ["/"] } },
  { id: "goAside", label: t("Go to Set aside"), part: "moving", keys: standardOnly("Mod+g") },
  { id: "goPinned", label: t("Go to Pinned"), part: "moving", keys: standardOnly("Mod+d") },
  { id: "goSnoozed", label: t("Go to Snoozed"), part: "moving", keys: standardOnly("Mod+s") },
  { id: "goFolder", label: t("Go to a folder"), part: "moving", keys: standardOnly("Mod+Shift+G") },
  { id: "inboxPriority", label: t("Inbox by priority"), part: "moving", keys: standardOnly("Mod+Alt+1") },
  { id: "inboxCategory", label: t("Inbox by category"), part: "moving", keys: standardOnly("Mod+Alt+2") },
  { id: "inboxDate", label: t("Inbox by date"), part: "moving", keys: standardOnly("Mod+Alt+3") },
  { id: "showDone", label: t("Show or hide Done mail"), part: "moving", keys: both("Mod+e") },
  { id: "select", label: t("Select or unselect the open message"), part: "messages", keys: both("x") },
  { id: "selectAll", label: t("Select every message"), part: "messages", keys: both("Mod+a") },
  { id: "openAll", label: t("Open every message of the conversation"), part: "messages", keys: both("Shift+O") },
  { id: "foldOlder", label: t("Fold the older messages"), part: "messages", keys: both("o") },
  { id: "preview", label: t("Preview the attachments"), part: "messages", keys: both(" ") },
  { id: "print", label: t("Print the open message"), part: "messages", keys: standardOnly("p") },
  { id: "undo", label: t("Undo"), part: "messages", keys: both("z", "Mod+z") },
  { id: "compose", label: t("New message"), part: "writing", keys: both("c") },
  { id: "send", label: t("Send"), part: "writing", keys: both("Mod+Enter"), typing: true },
  { id: "sendDone", label: t("Send, and the conversation is Done"), part: "writing", keys: both("Mod+Shift+Enter"), typing: true },
  { id: "attach", label: t("Attach files"), part: "writing", keys: both("Mod+Shift+A"), typing: true },
  { id: "template", label: t("Insert a template"), part: "writing", keys: both("Mod+Shift+T"), typing: true },
  { id: "discard", label: t("Discard the draft"), part: "writing", keys: both("Mod+Shift+\\"), typing: true },
  { id: "toFrom", label: t("Go to From"), part: "writing", keys: both("Mod+1"), typing: true },
  { id: "toSubject", label: t("Go to Subject"), part: "writing", keys: both("Mod+2"), typing: true },
  { id: "toTo", label: t("Go to To"), part: "writing", keys: both("Mod+3"), typing: true },
  { id: "toCc", label: t("Go to Cc"), part: "writing", keys: both("Mod+4"), typing: true },
  { id: "toBcc", label: t("Go to Bcc"), part: "writing", keys: both("Mod+5"), typing: true },
  { id: "toText", label: t("Go to the text"), part: "writing", keys: both("Mod+6"), typing: true },
  { id: "newFolder", label: t("New folder"), part: "app", keys: both("Mod+Shift+U") },
  { id: "sync", label: t("Check for new mail"), part: "app", keys: standardOnly("Mod+r") },
  { id: "zoomIn", label: t("Zoom in"), part: "app", keys: both("Mod+=", "Mod++"), typing: true },
  { id: "zoomOut", label: t("Zoom out"), part: "app", keys: both("Mod+-"), typing: true },
  { id: "zoomReset", label: t("Actual size"), part: "app", keys: both("Mod+0"), typing: true },
  { id: "settings", label: t("Settings"), part: "app", keys: both("Mod+,") },
  { id: "help", label: t("Show these shortcuts"), part: "app", keys: both("?") },
  { id: "quit", label: t("Quit Regards"), part: "app", keys: both("Mod+q") },
];

/** Each command's keys: the key set's, or the ones the person chose instead. */
export function commandKeys(set: KeySet, mine: Record<string, string[]>): Record<CommandId, string[]> {
  return Object.fromEntries(
    COMMANDS.map(({ id, keys }) => [id, Array.isArray(mine[id]) ? mine[id] : keys[set]]),
  ) as Record<CommandId, string[]>;
}

const currentKeys = () => commandKeys(getPrefs().keySet, getPrefs().myKeys);

/**
 * Which of `ids` a key pressed is now, for keys that work while typing (the
 * composer's, zoom), which the layers leave alone.
 */
export function commandOf<Id extends CommandId>(event: KeyboardEvent, ids: Id[]): Id | null {
  const names = pressedNames(event);
  if (names.length === 0) return null;
  const keys = currentKeys();
  return ids.find((id) => keys[id].some((key) => names.includes(key))) ?? null;
}

/** The command's first key now ("Mod+k"), if it has one. */
export const firstKey = (id: CommandId): string | undefined => currentKeys()[id][0];

/** The command's first key as shown ("Ctrl+K"), if it has one: for a menu's hint. */
export const commandHint = (id: CommandId): string | undefined => {
  const key = firstKey(id);
  return key === undefined ? undefined : keyText(key);
};

/** A tooltip: `label`, with the command's first key in brackets when it has one. */
export function withKey(label: string, id: CommandId): string {
  const key = commandHint(id);
  return key === undefined ? label : `${label} (${key})`;
}

/**
 * Keys that always do the same thing, by what they do: no shortcut can take
 * them. The arrows step as j and k do, in the standard set also left and right.
 */
const FIXED: Record<string, string> = {
  ArrowDown: t("Next message"),
  ArrowUp: t("Previous message"),
  ArrowRight: t("Next message"),
  ArrowLeft: t("Previous message"),
  Enter: t("Open the highlighted message"),
  Escape: t("Back to the list"),
  Tab: t("Moving between controls"),
  "Shift+Tab": t("Moving between controls"),
  "Mod+c": t("Copy, cut and paste"),
  "Mod+x": t("Copy, cut and paste"),
  "Mod+v": t("Copy, cut and paste"),
};

/** The text editor's own keys, which a key that works while typing cannot take. */
const TEXT_KEYS = [
  "Mod+z",
  "Mod+Shift+Z",
  "Mod+y",
  "Mod+a",
  "Mod+b",
  "Mod+i",
  "Mod+u",
  "Mod+e",
  "Mod+Shift+S",
  "Mod+Shift+B",
  "Mod+Shift+7",
  "Mod+Shift+8",
  "Mod+Shift+9",
  "Mod+Alt+0",
  "Mod+Alt+1",
  "Mod+Alt+2",
  "Mod+Alt+3",
  "Mod+Backspace",
  "Mod+Delete",
  "Shift+Enter",
];

/**
 * Why the keys `names` (one key pressed, by its names) cannot be given to
 * command or action `id`, or null when they can (Settings › Shortcuts).
 */
export function refusedKey(id: string, names: string[]): string | null {
  const fixed = names.map((name) => FIXED[name]).find(Boolean);
  if (fixed) return t("{key} is taken: {what}. Try another key.", { key: keyText(names[0]), what: fixed });
  if (!COMMANDS.find((command) => command.id === id)?.typing) return null;
  if (!names.some((name) => name.startsWith("Mod+"))) {
    return t("This one works while typing, so its key needs {key}.", { key: MOD_KEY });
  }
  if (names.some((name) => TEXT_KEYS.includes(name))) {
    return t("{key} is taken: writing in the text (bold, lists, undo…). Try another key.", { key: keyText(names[0]) });
  }
  return null;
}

/** A line of the help panel: what the keys of the actions and commands `ids` do, with keys that never change. */
interface Line {
  action: string;
  ids?: string[];
  fixed?: string[];
}

/**
 * For the help panel: the shortcuts of each key set, with every action's and
 * command's keys now (`keys`, by id) and names for those it does not list
 * (`labels`). A line whose keys are all gone is left out.
 */
export function shortcutList(set: KeySet, keys: Record<string, string[]>, labels: Record<string, string> = {}): { keys: string[]; action: string }[] {
  const shown = (line: Line) => [...new Set([...(line.ids ?? []).flatMap((id) => keys[id] ?? []), ...(line.fixed ?? [])])].map(keyText);
  const lines = set === "classic" ? CLASSIC : STANDARD;
  const listed = new Set(lines.flatMap((line) => line.ids ?? []));
  const extra = Object.keys(keys)
    .filter((id) => !listed.has(id))
    .map((id): Line => ({ action: labels[id] ?? id, ids: [id] }));
  return [...lines, ...extra].map((line) => ({ keys: shown(line), action: line.action })).filter((line) => line.keys.length > 0);
}

const STANDARD: Line[] = [
  { action: t("Commands: everything, by name"), ids: ["commands"] },
  { action: t("Next message"), ids: ["next"], fixed: ["ArrowDown", "ArrowRight"] },
  { action: t("Previous message"), ids: ["previous"], fixed: ["ArrowUp", "ArrowLeft"] },
  { action: t("Open the highlighted message"), fixed: ["Enter"] },
  { action: t("Back to the list, or close the message"), fixed: ["Escape"] },
  { action: t("Show or fold the sidebar"), ids: ["sidebar"] },
  { action: t("Search the list"), ids: ["search"] },
  { action: t("Find in the open message"), ids: ["find"] },
  { action: t("Preview the open message's attachments; again to close"), ids: ["preview"] },
  { action: t("Done (archive); a new sender is accepted too"), ids: ["archive"] },
  { action: t("Delete (in Trash or Junk: for good, after confirming)"), ids: ["delete"] },
  { action: t("Flag or unflag"), ids: ["flag"] },
  { action: t("Pin or unpin"), ids: ["pin"] },
  { action: t("Snooze, or bring back a snoozed message"), ids: ["snooze"] },
  { action: t("Set aside, or bring back to the inbox"), ids: ["aside"] },
  { action: t("Mark read or unread"), ids: ["read", "unread"] },
  { action: t("Mark as junk"), ids: ["junk"] },
  { action: t("Move to a folder"), ids: ["move"] },
  { action: t("Print the open message"), ids: ["print"] },
  { action: t("Labels: add or remove them (Gmail)"), ids: ["label"] },
  { action: t("Select or unselect the open message"), ids: ["select"] },
  { action: t("Select every message in the list"), ids: ["selectAll"] },
  { action: t("Undo, while the undo bar shows (also undo send)"), ids: ["undo"] },
  { action: t("New message"), ids: ["compose"] },
  { action: t("Reply, reply all, forward"), ids: ["reply", "replyAll", "forward"] },
  { action: t("Open every message of the conversation, or fold the older ones"), ids: ["openAll", "foldOlder"] },
  { action: t("Go to Set aside, Pinned, Snoozed"), ids: ["goAside", "goPinned", "goSnoozed"] },
  { action: t("Go to a folder"), ids: ["goFolder"] },
  { action: t("New folder (inside the open one)"), ids: ["newFolder"] },
  { action: t("Inbox by priority, by category, by date"), ids: ["inboxPriority", "inboxCategory", "inboxDate"] },
  { action: t("Check for new mail"), ids: ["sync"] },
  ...writing(),
  { action: t("Show Done mail in the inbox, or hide it"), ids: ["showDone"] },
  { action: t("Settings"), ids: ["settings"] },
  { action: t("Show or hide these shortcuts"), ids: ["help"] },
];

const CLASSIC: Line[] = [
  { action: t("Commands: everything, by name"), ids: ["commands"] },
  { action: t("Next message"), ids: ["next"], fixed: ["ArrowDown"] },
  { action: t("Previous message"), ids: ["previous"], fixed: ["ArrowUp"] },
  { action: t("Open the highlighted message"), fixed: ["Enter"] },
  { action: t("Back to the list, or close the message"), fixed: ["Escape"] },
  { action: t("Show or fold the sidebar"), ids: ["sidebar"] },
  { action: t("Search the list"), ids: ["search"] },
  { action: t("Find in the open message"), ids: ["find"] },
  { action: t("Preview the open message's attachments; again to close"), ids: ["preview"] },
  { action: t("Archive"), ids: ["archive"] },
  { action: t("Delete (in Trash or Junk: for good, after confirming)"), ids: ["delete"] },
  { action: t("Flag or unflag"), ids: ["flag"] },
  { action: t("Pin or unpin"), ids: ["pin"] },
  { action: t("Snooze, or bring back a snoozed message"), ids: ["snooze"] },
  { action: t("Remind me if nobody replies, or remove the reminder"), ids: ["remind"] },
  { action: t("Set aside, or bring back to the inbox"), ids: ["aside"] },
  { action: t("Move to a folder"), ids: ["move"] },
  { action: t("Mark unread"), ids: ["unread"] },
  { action: t("Mark read"), ids: ["read"] },
  { action: t("Labels: add or remove them (Gmail)"), ids: ["label"] },
  { action: t("Select or unselect the open message"), ids: ["select"] },
  { action: t("Select every message in the list"), ids: ["selectAll"] },
  { action: t("Undo, while the undo bar shows (also undo send)"), ids: ["undo"] },
  { action: t("New message"), ids: ["compose"] },
  { action: t("Reply, reply all, forward"), ids: ["reply", "replyAll", "forward"] },
  { action: t("Open every message of the conversation, or fold the older ones"), ids: ["openAll", "foldOlder"] },
  ...writing(),
  { action: t("New folder (inside the open one)"), ids: ["newFolder"] },
  { action: t("Show Done mail in the inbox, or hide it"), ids: ["showDone"] },
  { action: t("Settings"), ids: ["settings"] },
  { action: t("Show or hide these shortcuts"), ids: ["help"] },
];

/** The composer's lines and the zoom's, the same in both sets. */
function writing(): Line[] {
  return [
    { action: t("Send, in the composer"), ids: ["send"] },
    { action: t("Send, and the conversation is Done"), ids: ["sendDone"] },
    { action: t("Attach files, in the composer"), ids: ["attach"] },
    { action: t("Insert a template, in the composer"), ids: ["template"] },
    { action: t("Discard the draft, in the composer"), ids: ["discard"] },
    { action: t("Go to From, Subject, To, Cc, Bcc, the text"), ids: ["toFrom", "toSubject", "toTo", "toCc", "toBcc", "toText"] },
    { action: t("Zoom in, zoom out, actual size"), ids: ["zoomIn", "zoomOut", "zoomReset"] },
    { action: t("Quit Regards"), ids: ["quit"] },
  ];
}
