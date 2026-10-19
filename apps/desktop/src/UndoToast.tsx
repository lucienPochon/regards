import { useEffect, useState } from "react";
import { type UndoInfo, applyUndo } from "./accounts";
import { Icon } from "./icons";
import { useShortcuts } from "./shortcuts";

/** How long the toast offers undo (ADR-0007: at least 10 seconds). */
const SHOW_FOR_MS = 10_000;

export function UndoToast({ undo, onDone }: { undo: UndoInfo; onDone: () => void }) {
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const timer = window.setTimeout(onDone, SHOW_FOR_MS);
    return () => window.clearTimeout(timer);
  }, [undo, onDone]);

  const revert = async () => {
    try {
      await applyUndo(undo.id);
      onDone();
    } catch (failure) {
      setError(String(failure));
    }
  };
  useShortcuts(error ? {} : { z: revert, "Mod+z": revert });

  return (
    <div className="toast" role="status">
      <Icon name={error ? "alert" : "check"} size={16} />
      <span>{error ?? undo.description}</span>
      {!error && (
        <button type="button" onClick={revert}>
          Undo
        </button>
      )}
    </div>
  );
}
