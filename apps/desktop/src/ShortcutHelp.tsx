import { ShortcutsTab } from "./Settings";
import { useShortcuts } from "./shortcuts";

/** The list of keyboard shortcuts; while it shows, other shortcuts are paused. */
export function ShortcutHelp({ onClose }: { onClose: () => void }) {
  useShortcuts({ Escape: onClose, "?": onClose }, { modal: true });
  return (
    <div className="backdrop" onMouseDown={onClose}>
      <section
        className="dialog help"
        role="dialog"
        aria-modal="true"
        aria-label="Keyboard shortcuts"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <ShortcutsTab />
        <div className="dialog-actions">
          <button type="button" className="btn primary" onClick={onClose} autoFocus>
            Close
          </button>
        </div>
      </section>
    </div>
  );
}
