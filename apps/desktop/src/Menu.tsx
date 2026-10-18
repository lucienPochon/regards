import { type KeyboardEvent, useLayoutEffect, useRef, useState } from "react";
import { Icon, type IconName } from "./icons";
import { useShortcuts } from "./shortcuts";

export interface MenuItem {
  label: string;
  icon?: IconName;
  /** A shortcut or a time to show at the end of the row. */
  hint?: string;
  danger?: boolean;
  disabled?: boolean;
  /** Indent, for folders in a tree. */
  depth?: number;
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
  const palette = request.variant === "palette";
  const dismiss = () => {
    request.onDismiss?.();
    onClose();
  };
  useShortcuts({ Escape: dismiss }, { modal: true });

  const items = request.entries.filter(
    (entry): entry is MenuItem =>
      entry !== "separator" && !entry.disabled && entry.label.toLowerCase().includes(query.toLowerCase()),
  );
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
    onClose();
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
        role="menu"
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
            onChange={(event) => {
              setQuery(event.target.value);
              setActive(0);
            }}
          />
        )}
        <div className="menu-items">
          {shown.map((entry, index) =>
            entry === "separator" ? (
              <hr key={`separator-${index}`} />
            ) : (
              <button
                key={`${entry.label}-${index}`}
                type="button"
                role="menuitem"
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
                {entry.hint && <kbd className="menu-hint">{entry.hint}</kbd>}
              </button>
            ),
          )}
          {request.filter && query && items.length === 0 && <p className="menu-empty">Nothing matches.</p>}
        </div>
      </div>
    </div>
  );
}
