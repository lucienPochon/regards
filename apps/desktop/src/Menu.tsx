import { type KeyboardEvent, useLayoutEffect, useRef, useState, useId } from "react";
import { Icon, type IconName } from "./icons";
import { MOD_KEY, keyText, useShortcuts } from "./shortcuts";
import { t } from "./i18n";

export interface MenuItem {
  label: string;
  icon?: IconName;
  /** A shortcut or a time to show at the end of the row. */
  hint?: string;
  /** A shortcut shown as key caps ("Shift+U" → two caps), in the palette. */
  keys?: string;
  /** A second, quieter line or a path, shown after the label. */
  detail?: string;
  danger?: boolean;
  disabled?: boolean;
  /** Indent, for folders in a tree. */
  depth?: number;
  /** Shown only until something is typed: a quick way to an entry the list has too (a recent folder). */
  quick?: boolean;
  /** A switch, on or off: picking it flips it (`onSelect`) and the menu stays open. */
  checked?: boolean;
  onSelect: () => void;
}

export type MenuEntry = MenuItem | "separator";

/** Where a menu opens: under a button (or at its end), or at the pointer. */
export type Anchor = { rect: DOMRect; align?: "start" | "end" } | { x: number; y: number };

export interface MenuRequest {
  /** Ignored by the palette, which sits in the upper middle of the window. */
  anchor: Anchor;
  entries: MenuEntry[];
  /** Show a filter field with this placeholder (for long lists such as folders, or the palette). */
  filter?: string;
  /** Entries made from what is typed in the filter field (a time, say), shown first. */
  typed?: (query: string) => MenuItem[];
  label: string;
  /**
   * "palette": a wide panel in the upper middle of the window with a title,
   * used for the command palette and the time pickers. Otherwise a pop-up at the anchor.
   */
  variant?: "palette";
  /** Shown above the entries in a palette. */
  title?: string;
  /** Called when it closes without a pick. */
  onDismiss?: () => void;
}

/** "Shift+U" → ["Shift", "U"]; "Mod+k" → [Ctrl or ⌘, "K"]; a "+" key stays a key ("Mod++"); named keys as the language names them. */
export function keyCaps(keys: string): string[] {
  const parts = keys === "+" ? ["+"] : keys.endsWith("++") ? [...keys.slice(0, -2).split("+"), "+"] : keys.split("+");
  return parts.map((part) => (part === "Mod" ? MOD_KEY : part !== " " && part.length === 1 ? part.toUpperCase() : keyText(part)));
}

export function anchorOf(element: Element, align: "start" | "end" = "start"): Anchor {
  return { rect: element.getBoundingClientRect(), align };
}

const MARGIN = 8;

/**
 * A pop-up menu or a palette. Arrow keys move, Enter picks, Escape or a click
 * outside closes; while it is open, other shortcuts are paused. With a filter
 * field, typing narrows the list and the arrows move the highlight.
 */
export function Menu({ request, onClose }: { request: MenuRequest; onClose: () => void }) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const [position, setPosition] = useState<{ left: number; top: number } | null>(null);
  const menu = useRef<HTMLDivElement>(null);
  const listId = useId();
  const palette = request.variant === "palette";
  // With a filter field it is a combobox over a list (focus stays in the field, the keys move a
  // highlight); without one, a menu whose items take focus in turn.
  const searchable = Boolean(request.filter);
  const dismiss = () => {
    request.onDismiss?.();
    onClose();
  };
  useShortcuts({ Escape: dismiss }, { modal: true });

  const items = [
    ...(query.trim() && request.typed ? request.typed(query) : []),
    ...request.entries.filter(
      (entry): entry is MenuItem =>
        entry !== "separator" &&
        !entry.disabled &&
        !(query && entry.quick) &&
        entry.label.toLowerCase().includes(query.toLowerCase()),
    ),
  ];
  const shown: MenuEntry[] = query ? items : request.entries;
  const highlighted = request.filter ? items[Math.min(active, items.length - 1)] : undefined;

  // Place it, then keep it on screen.
  useLayoutEffect(() => {
    const box = menu.current?.getBoundingClientRect();
    if (!box) return;
    if (palette) {
      setPosition({ left: Math.max(MARGIN, (window.innerWidth - box.width) / 2), top: Math.round(window.innerHeight * 0.16) });
      return;
    }
    const { anchor } = request;
    let left: number;
    let top: number;
    if ("rect" in anchor) {
      left = anchor.align === "end" ? anchor.rect.right - box.width : anchor.rect.left;
      top = anchor.rect.bottom + 4;
      if (top + box.height > window.innerHeight - MARGIN) top = anchor.rect.top - box.height - 4;
    } else {
      left = anchor.x;
      top = anchor.y;
      if (top + box.height > window.innerHeight - MARGIN) top = anchor.y - box.height;
    }
    left = Math.max(MARGIN, Math.min(left, window.innerWidth - box.width - MARGIN));
    top = Math.max(MARGIN, Math.min(top, window.innerHeight - box.height - MARGIN));
    setPosition({ left, top });
  }, [request, palette]);

  useLayoutEffect(() => {
    if (!position) return;
    const first = request.filter
      ? menu.current?.querySelector<HTMLElement>("input")
      : menu.current?.querySelector<HTMLElement>(".menu-item:not(:disabled)");
    first?.focus();
  }, [position, request.filter]);

  useLayoutEffect(() => {
    menu.current?.querySelector(".menu-item.highlighted")?.scrollIntoView({ block: "nearest" });
  }, [active, query]);

  const pick = (item: MenuItem) => {
    if (item.disabled) return;
    if (item.checked === undefined) onClose();
    item.onSelect();
  };

  const keys = (event: KeyboardEvent) => {
    const inField = event.target instanceof HTMLInputElement;
    if ((event.key === "ArrowDown" || event.key === "ArrowUp") && inField) {
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      setActive((now) => (Math.min(now, items.length - 1) + step + items.length) % Math.max(1, items.length));
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const buttons = Array.from(menu.current?.querySelectorAll<HTMLElement>(".menu-item:not(:disabled)") ?? []);
      const at = buttons.indexOf(document.activeElement as HTMLElement);
      const step = event.key === "ArrowDown" ? 1 : -1;
      const next = at === -1 ? (step > 0 ? 0 : buttons.length - 1) : (at + step + buttons.length) % buttons.length;
      buttons[next]?.focus();
    } else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      dismiss();
    } else if (event.key === "Enter" && inField) {
      event.preventDefault();
      if (highlighted) pick(highlighted);
    } else if (event.key === "Tab") {
      event.preventDefault();
    }
  };

  return (
    <div
      className={palette ? "menu-layer dim" : "menu-layer"}
      onMouseDown={dismiss}
      onContextMenu={(event) => event.preventDefault()}
    >
      <div
        ref={menu}
        className={["menu", palette ? "palette" : "", position ? "" : "measuring"].join(" ")}
        role={searchable ? "dialog" : "menu"}
        aria-label={request.label}
        style={position ?? undefined}
        onMouseDown={(event) => event.stopPropagation()}
        onKeyDown={keys}
      >
        {palette && request.title && <p className="palette-title">{request.title}</p>}
        {request.filter && (
          <input
            className="menu-filter"
            value={query}
            placeholder={request.filter}
            aria-label={request.filter}
            role="combobox"
            aria-expanded="true"
            aria-autocomplete="list"
            aria-controls={listId}
            aria-activedescendant={highlighted ? `${listId}-${items.indexOf(highlighted)}` : undefined}
            onChange={(event) => {
              setQuery(event.target.value);
              setActive(0);
            }}
          />
        )}
        <div
          className="menu-items"
          id={searchable ? listId : undefined}
          role={searchable ? "listbox" : undefined}
          aria-label={searchable ? request.label : undefined}
        >
          {shown.map((entry, index) =>
            entry === "separator" ? (
              <hr key={`separator-${index}`} role={searchable ? "none" : "separator"} />
            ) : (
              <button
                key={`${entry.label}-${index}`}
                type="button"
                role={searchable ? "option" : entry.checked === undefined ? "menuitem" : "menuitemcheckbox"}
                aria-checked={entry.checked}
                id={searchable && items.includes(entry) ? `${listId}-${items.indexOf(entry)}` : undefined}
                aria-selected={searchable ? entry === highlighted : undefined}
                tabIndex={searchable ? -1 : undefined}
                className={[
                  "menu-item",
                  entry.danger ? "danger" : "",
                  entry === highlighted ? "highlighted" : "",
                  entry.depth ? `depth-${Math.min(entry.depth, 4)}` : "",
                ].join(" ")}
                disabled={entry.disabled}
                onMouseEnter={() => {
                  const at = items.indexOf(entry);
                  if (at !== -1) setActive(at);
                }}
                onClick={() => pick(entry)}
              >
                {entry.icon && <Icon name={entry.icon} size={16} />}
                <span className="menu-label">{entry.label}</span>
                {entry.detail && <span className="menu-detail">{entry.detail}</span>}
                {entry.hint && <kbd className="menu-hint">{entry.hint}</kbd>}
                {entry.checked !== undefined && <span className={entry.checked ? "toggle on" : "toggle"} aria-hidden="true" />}
                {entry.keys && (
                  <span className="keycaps">
                    {keyCaps(entry.keys).map((cap, at) => (
                      <kbd key={at}>{cap}</kbd>
                    ))}
                  </span>
                )}
              </button>
            ),
          )}
          {request.filter && query && items.length === 0 && (
            <p className="menu-empty" role="status">
              {t("Nothing matches.")}
            </p>
          )}
        </div>
      </div>
    </div>
  );
}
