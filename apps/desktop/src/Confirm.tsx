import { useShortcuts } from "./shortcuts";

export interface ConfirmRequest {
  title: string;
  body: string;
  /** The label of the button that goes ahead. */
  confirm: string;
  danger?: boolean;
}

/** Asks before something that cannot be undone. Cancel has the focus, so a stray Enter cancels. */
export function Confirm({ request, onAnswer }: { request: ConfirmRequest; onAnswer: (ok: boolean) => void }) {
  useShortcuts({ Escape: () => onAnswer(false) }, { modal: true });
  return (
    <div className="backdrop" onMouseDown={() => onAnswer(false)}>
      <section
        className="dialog confirm-dialog"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="confirm-title"
        aria-describedby="confirm-body"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <h2 id="confirm-title">{request.title}</h2>
        <p id="confirm-body">{request.body}</p>
        <div className="dialog-actions">
          <button type="button" className="btn" onClick={() => onAnswer(false)} autoFocus>
            Cancel
          </button>
          <button
            type="button"
            className={request.danger ? "btn danger-fill" : "btn primary"}
            onClick={() => onAnswer(true)}
          >
            {request.confirm}
          </button>
        </div>
      </section>
    </div>
  );
}
