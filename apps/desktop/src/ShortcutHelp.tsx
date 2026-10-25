import { useRef } from "react";
import { ShortcutsTab } from "./Settings";
import { useShortcuts } from "./shortcuts";
import { useDialogFocus } from "./dialogFocus";
import { t } from "./i18n";

/** The list of keyboard shortcuts; while it shows, other shortcuts are paused. */
export function ShortcutHelp({ onClose }: { onClose: () => void }) {
  useShortcuts({ Escape: onClose, "?": onClose }, { modal: true });
  const box = useRef<HTMLElement>(null);
  useDialogFocus(box);
  return (
    <div className="backdrop" onMouseDown={onClose}>
      <section
        ref={box}
        tabIndex={-1}
        className="dialog help"
        role="dialog"
        aria-modal="true"
        aria-label={t("Keyboard shortcuts")}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <ShortcutsTab />
        <div className="dialog-actions">
          <button type="button" className="btn primary" onClick={onClose} autoFocus>
            {t("Close")}
          </button>
        </div>
      </section>
    </div>
  );
}
