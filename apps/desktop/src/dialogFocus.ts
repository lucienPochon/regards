import { type RefObject, useEffect } from "react";

/** What Tab can land on inside an element, in order. */
const FOCUSABLE = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled]):not([type='hidden'])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "[contenteditable='true']",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

export function focusables(root: HTMLElement): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
    (element) => element.getClientRects().length > 0 && !element.closest("[inert], [aria-hidden='true']"),
  );
}

/** Open dialogs, the newest last: only it keeps Tab inside. */
const open: HTMLElement[] = [];

/**
 * Focus for a dialog or a panel over the app (WCAG 2.4.3): on opening, focus
 * goes into it (to what already took it, such as an autofocused button, or
 * else the first control); Tab and Shift+Tab go round inside it; on closing,
 * focus goes back to what had it before, if that is still there. Give the
 * element tabIndex={-1}, so it can hold focus when it has no controls.
 */
export function useDialogFocus(ref: RefObject<HTMLElement | null>) {
  useEffect(() => {
    const root = ref.current;
    if (!root) return;
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    open.push(root);
    if (!root.contains(document.activeElement)) (focusables(root)[0] ?? root).focus({ preventScroll: true });

    const keys = (event: KeyboardEvent) => {
      if (event.key !== "Tab" || open[open.length - 1] !== root) return;
      const active = document.activeElement;
      // Focus in another layer over the dialog (a menu, the palette) is that layer's.
      if (active && active !== document.body && !root.contains(active)) return;
      const items = focusables(root);
      if (items.length === 0) {
        event.preventDefault();
        root.focus();
        return;
      }
      const first = items[0];
      const last = items[items.length - 1];
      if (event.shiftKey && (active === first || active === root || active === document.body)) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && (active === last || active === document.body)) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", keys);
    return () => {
      document.removeEventListener("keydown", keys);
      open.splice(open.indexOf(root), 1);
      // Back to what opened it, unless focus has already gone somewhere on purpose.
      const now = document.activeElement;
      if (before?.isConnected && (!now || now === document.body || root.contains(now) || !now.isConnected)) {
        before.focus({ preventScroll: true });
      }
    };
  }, [ref]);
}
