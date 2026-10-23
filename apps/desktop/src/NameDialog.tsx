import { type FormEvent, useRef, useState } from "react";
import { useDialogFocus } from "./dialogFocus";
import { useShortcuts } from "./shortcuts";

export interface NameRequest {
  title: string;
  /** What is named: "Folder name". */
  label: string;
  value: string;
  /** The button that saves: "Create", "Rename". */
  confirm: string;
  /** Does the work; what it throws shows under the field, and the dialog stays. */
  onSave: (name: string) => Promise<unknown>;
}

/** Asks for a name (a new folder's, say) and keeps the server's refusal in view until it is fixed. */
export function NameDialog({ request, onClose }: { request: NameRequest; onClose: () => void }) {
  const [name, setName] = useState(request.value);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useShortcuts({ Escape: onClose }, { modal: true });
  const box = useRef<HTMLFormElement>(null);
  useDialogFocus(box);
  const ready = name.trim() !== "" && name.trim() !== request.value.trim();

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!ready || busy) return;
    setBusy(true);
    setError(null);
    request.onSave(name.trim()).then(onClose, (failure: unknown) => {
      setError(String(failure));
      setBusy(false);
    });
  };

  return (
    <div className="backdrop" onMouseDown={onClose}>
      <form
        ref={box}
        tabIndex={-1}
        className="dialog name-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="name-title"
        onMouseDown={(event) => event.stopPropagation()}
        onSubmit={submit}
      >
        <h2 id="name-title">{request.title}</h2>
        <label className="name-field">
          <span>{request.label}</span>
          <input
            className="input"
            value={name}
            onChange={(event) => setName(event.target.value)}
            onFocus={(event) => event.target.select()}
            autoFocus
            spellCheck={false}
          />
        </label>
        {error && (
          <p className="error small" role="alert">
            {error}
          </p>
        )}
        <div className="dialog-actions">
          <button type="button" className="btn" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button type="submit" className="btn primary" disabled={busy || !ready}>
            {request.confirm}
          </button>
        </div>
      </form>
    </div>
  );
}
