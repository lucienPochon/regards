import { useState, useRef } from "react";
import { useShortcuts } from "./shortcuts";
import { useDialogFocus } from "./dialogFocus";

/** "2026-10-21T09:00" for a datetime-local field, in local time. */
function localValue(time: number): string {
  const date = new Date(time);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

/** Choose a date and time (local), for a snooze or a later send. Resolves with Unix ms, or null. */
export function TimePicker({
  title,
  confirm,
  initial,
  onAnswer,
}: {
  title: string;
  confirm: string;
  initial: number;
  onAnswer: (time: number | null) => void;
}) {
  const [value, setValue] = useState(localValue(initial));
  useShortcuts({ Escape: () => onAnswer(null) }, { modal: true });
  const box = useRef<HTMLElement>(null);
  useDialogFocus(box);
  // The field gives local wall-clock time; Date parses it in the local zone.
  const time = value ? new Date(value).getTime() : Number.NaN;
  const valid = Number.isFinite(time) && time > Date.now();
  return (
    <div className="backdrop" onMouseDown={() => onAnswer(null)}>
      <section
        ref={box}
        tabIndex={-1}
        className="dialog confirm-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="time-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <h2 id="time-title">{title}</h2>
        <input
          type="datetime-local"
          className="input"
          value={value}
          min={localValue(Date.now())}
          onChange={(event) => setValue(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && valid) onAnswer(time);
          }}
          aria-label="Date and time"
          autoFocus
        />
        {!valid && value && <p className="muted small">Choose a time in the future.</p>}
        <div className="dialog-actions">
          <button type="button" className="btn" onClick={() => onAnswer(null)}>
            Cancel
          </button>
          <button type="button" className="btn primary" disabled={!valid} onClick={() => onAnswer(time)}>
            {confirm}
          </button>
        </div>
      </section>
    </div>
  );
}
