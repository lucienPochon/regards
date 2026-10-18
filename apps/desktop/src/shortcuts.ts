import { type RefObject, useEffect, useRef } from "react";

/**
 * Keyboard shortcuts. Each component that offers some registers a layer; the
 * most recently shown layer that knows a key handles it. A modal layer (the
 * help panel, say) keeps keys from reaching the layers below it.
 *
 * Key names: a letter as typed ("j"), "Shift+U" for capitals, symbols as
 * typed ("#", "?"), named keys ("Escape", "ArrowDown", "Delete") and
 * "Mod+z" for Ctrl+Z (Cmd+Z on macOS).
 *
 * Keys typed into a field are left alone, and so are combinations with Alt or
 * with Ctrl/Cmd other than our own "Mod+" ones.
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
  const name = keyName(event);
  if (!name) return;
  for (let index = layers.length - 1; index >= 0; index--) {
    const layer = layers[index];
    const action = layer.keys.current[name];
    if (action) {
      event.preventDefault();
      action();
      return;
    }
    if (layer.modal) return;
  }
}

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

function typing(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName);
}

/** For the help panel. */
export const SHORTCUTS: { keys: string; action: string }[] = [
  { keys: "j / ↓", action: "Next message" },
  { keys: "k / ↑", action: "Previous message" },
  { keys: "Esc", action: "Close the message" },
  { keys: "e", action: "Archive" },
  { keys: "# / Delete", action: "Delete (in Trash or Junk: for good, after confirming)" },
  { keys: "s", action: "Flag or unflag" },
  { keys: "Shift+U", action: "Mark unread" },
  { keys: "Shift+I", action: "Mark read" },
  { keys: "z", action: "Undo, while the undo bar shows (also undo send)" },
  { keys: "c", action: "New message" },
  { keys: "r / a / f", action: "Reply, reply all, forward" },
  { keys: `${MOD_KEY}+Enter`, action: "Send, in the composer" },
  { keys: "?", action: "Show or hide these shortcuts" },
];
