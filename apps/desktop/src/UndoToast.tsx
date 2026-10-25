import { useEffect, useState } from "react";
import { type UndoInfo, applyUndo } from "./accounts";
import { Icon } from "./icons";
import { useShortcuts } from "./shortcuts";
import { t } from "./i18n";
import { fromService } from "./i18n-daemon";

/** How long the toast offers undo (ADR-0007: at least 10 seconds). */
const SHOW_FOR_MS = 10_000;

/** One change to undo, or several made together (each account's part of one action). */
export type UndoBar = UndoInfo & { also?: number[] };

export function UndoToast({ undo, onDone }: { undo: UndoBar; onDone: () => void }) {
  const [error, setError] = useState<string | null>(null);
  // Under the pointer or the keyboard, it waits (WCAG 2.2.1); it starts again on leaving.
  const [held, setHeld] = useState(false);

  useEffect(() => {
    if (held) return;
    const timer = window.setTimeout(onDone, SHOW_FOR_MS);
    return () => window.clearTimeout(timer);
  }, [undo, onDone, held]);

  const revert = async () => {
    try {
      await applyUndo(undo.id);
      for (const id of undo.also ?? []) await applyUndo(id);
      onDone();
    } catch (failure) {
      setError(String(failure));
    }
  };
  useShortcuts(error ? {} : { z: revert, "Mod+z": revert });

  return (
    <div
      className="toast"
      onMouseEnter={() => setHeld(true)}
      onMouseLeave={() => setHeld(false)}
      onFocus={() => setHeld(true)}
      onBlur={() => setHeld(false)}
    >
      <Icon name={error ? "alert" : "check"} size={16} />
      <span>{error ?? fromService(undo.description)}</span>
      {!error && (
        <button type="button" onClick={revert}>
          {t("Undo")}
        </button>
      )}
    </div>
  );
}
