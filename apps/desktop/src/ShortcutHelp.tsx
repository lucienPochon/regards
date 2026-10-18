import { SHORTCUTS, useShortcuts } from "./shortcuts";

/** The list of keyboard shortcuts; while it shows, other shortcuts are paused. */
export function ShortcutHelp({ onClose }: { onClose: () => void }) {
  useShortcuts({ Escape: onClose, "?": onClose }, { modal: true });
  return (
    <div className="help-backdrop" onClick={onClose}>
      <section
        className="card help"
        role="dialog"
        aria-modal="true"
        aria-labelledby="help-heading"
        onClick={(event) => event.stopPropagation()}
      >
        <h2 id="help-heading">Keyboard shortcuts</h2>
        <dl className="small">
          {SHORTCUTS.map((shortcut) => (
            <div key={shortcut.keys}>
              <dt>
                <kbd>{shortcut.keys}</kbd>
              </dt>
              <dd>{shortcut.action}</dd>
            </div>
          ))}
        </dl>
        <p className="muted small">After clicking inside a message's text, click outside it for the keys to work again.</p>
        <button type="button" onClick={onClose} autoFocus>
          Close
        </button>
      </section>
    </div>
  );
}
