import { useEffect, useState } from "react";
import { type Queued, undoSend } from "./compose";
import { useShortcuts } from "./shortcuts";

/** "Sending in 8 s · Undo" until the undo window is over. */
export function SendToast({
  queued,
  onUndone,
  onDone,
}: {
  queued: Queued;
  onUndone: (draftId: number | null) => void;
  onDone: () => void;
}) {
  const [now, setNow] = useState(Date.now());
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const tick = window.setInterval(() => setNow(Date.now()), 250);
    return () => window.clearInterval(tick);
  }, []);

  const left = Math.ceil((queued.undo_until - now) / 1000);
  useEffect(() => {
    if (left <= 0 && !error) {
      const timer = window.setTimeout(onDone, 1500);
      return () => window.clearTimeout(timer);
    }
  }, [left, error, onDone]);

  const undo = async () => {
    try {
      const result = await undoSend(queued.outbox_id);
      onUndone(result.draft_id);
    } catch (failure) {
      setError(String(failure));
    }
  };
  useShortcuts(left > 0 && !error ? { z: undo, "Mod+z": undo } : {});

  return (
    <div className="toast" role="status">
      {error ? (
        <span>{error}</span>
      ) : left > 0 ? (
        <>
          <span>Sending in {left} s</span>
          <button type="button" onClick={undo}>
            Undo
          </button>
        </>
      ) : (
        <span>Sending…</span>
      )}
    </div>
  );
}
