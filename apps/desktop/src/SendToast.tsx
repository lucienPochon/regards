import { useEffect, useState } from "react";
import { type Queued, undoSend } from "./compose";
import { laterDate } from "./format";
import { Icon } from "./icons";
import { useShortcuts } from "./shortcuts";
import { t } from "./i18n";

/** How long a scheduled message's toast stays; it can be taken back from the Outbox after that. */
const SCHEDULED_SHOW_MS = 10_000;

/** "Sending in 8 s · Undo" until the undo window is over, or "Scheduled for Mon 09:00 · Undo". */
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
  const [shownAt] = useState(Date.now());
  const scheduled = queued.send_at !== null && queued.send_at > shownAt;

  useEffect(() => {
    const tick = window.setInterval(() => setNow(Date.now()), 250);
    return () => window.clearInterval(tick);
  }, []);

  const left = scheduled
    ? Math.ceil((shownAt + SCHEDULED_SHOW_MS - now) / 1000)
    : Math.ceil((queued.undo_until - now) / 1000);
  useEffect(() => {
    if (left <= 0 && !error) {
      const timer = window.setTimeout(onDone, scheduled ? 0 : 1500);
      return () => window.clearTimeout(timer);
    }
  }, [left, error, onDone, scheduled]);

  const undo = async () => {
    try {
      const result = await undoSend(queued.outbox_id);
      onUndone(result.draft_id);
    } catch (failure) {
      setError(String(failure));
    }
  };
  useShortcuts(left > 0 && !error ? { undo } : {});

  return (
    <div className="toast">
      <Icon name={scheduled ? "sendLater" : "send"} size={16} />
      {error ? (
        <span role="alert">{error}</span>
      ) : left > 0 ? (
        <>
          {/* Said once; the ticking seconds would be read out every second. */}
          <span className="sr-only">
            {scheduled ? t("Scheduled for {when}.", { when: laterDate(queued.send_at as number) }) : t("Sending. Undo is still possible.")}
          </span>
          <span aria-hidden="true">
            {scheduled ? t("Scheduled for {when}", { when: laterDate(queued.send_at as number) }) : t("Sending in {seconds} s", { seconds: left })}
          </span>
          <button type="button" onClick={undo}>
            {t("Undo")}
          </button>
        </>
      ) : (
        <span>{t("Sending…")}</span>
      )}
    </div>
  );
}
