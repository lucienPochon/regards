import { type KeyboardEvent, useLayoutEffect, useRef, useState } from "react";
import { Icon, type IconName } from "./icons";
import { useShortcuts } from "./shortcuts";

export interface MenuItem {
  label: string;
  icon?: IconName;
  /** A shortcut to show beside it. */
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
  anchor: Anchor;
  entries: MenuEntry[];
  /** Show a filter field with this placeholder (for long lists such as folders). */
  filter?: string;
  label: string;
  /** Called when it closes without a pick. */
  onDismiss?: () => void;
}

export function anchorOf(element: Element, align: "start" | "end" = "start"): Anchor {
  return { rect: element.getBoundingClientRect(), align };
}

const MARGIN = 8;

/**
 * A pop-up menu. Arrow keys move, Enter picks, Escape or a click outside
 * closes; while it is open, other shortcuts are paused.
 */
export function Menu({ request, onClose }: { request: MenuRequest; onClose: () => void }) {
  const [query, setQuery] = useState("");
  const [position, setPosition] = useState<{ left: number; top: number } | null>(null);
  const menu = useRef<HTMLDivElement>(null);
  const dismiss = () => {
    request.onDismiss?.();
    onClose();
  };
  useShortcuts({ Escape: dismiss }, { modal: true });

  const items = request.entries.filter(
    (entry): entry is MenuItem => entry !== "separator" && entry.label.toLowerCase().includes(query.toLowerCase()),
  );
  const shown: MenuEntry[] = query ? items : request.entries;

  // Place it, then keep it on screen.
  useLayoutEffect(() => {
    const box = menu.current?.getBoundingClientRect();
    if (!box) return;
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
  }, [request]);

  useLayoutEffect(() => {
    if (!position) return;
    const first = request.filter
      ? menu.current?.querySelector<HTMLElement>("input")
      : menu.current?.querySelector<HTMLElement>(".menu-item:not(:disabled)");
    first?.focus();
  }, [position, request.filter]);

  const pick = (item: MenuItem) => {
    if (item.disabled) return;
    onClose();
    item.onSelect();
  };

  const keys = (event: KeyboardEvent) => {
    const buttons = Array.from(menu.current?.querySelectorAll<HTMLElement>(".menu-item:not(:disabled)") ?? []);
    const at = buttons.indexOf(document.activeElement as HTMLElement);
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      const next = at === -1 ? (step > 0 ? 0 : buttons.length - 1) : (at + step + buttons.length) % buttons.length;
      buttons[next]?.focus();
    } else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      dismiss();
    } else if (event.key === "Enter" && event.target instanceof HTMLInputElement) {
      event.preventDefault();
      const first = items.find((item) => !item.disabled);
      if (first) pick(first);
    } else if (event.key === "Tab") {
      event.preventDefault();
    }
  };

  return (
    <div className="menu-layer" onMouseDown={dismiss} onContextMenu={(event) => event.preventDefault()}>
      <div
        ref={menu}
        className={position ? "menu" : "menu measuring"}
        role="menu"
        aria-label={request.label}
        style={position ?? undefined}
        onMouseDown={(event) => event.stopPropagation()}
        onKeyDown={keys}
      >
        {request.filter && (
          <input
            className="menu-filter"
            value={query}
            placeholder={request.filter}
            aria-label={request.filter}
            onChange={(event) => setQuery(event.target.value)}
          />
        )}
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
                entry.depth ? `depth-${Math.min(entry.depth, 4)}` : "",
              ].join(" ")}
              disabled={entry.disabled}
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
  );
}
