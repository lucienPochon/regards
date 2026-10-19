import { useLayoutEffect, useRef, useState } from "react";
import { Icon } from "./icons";
import type { Anchor } from "./Menu";
import { useShortcuts } from "./shortcuts";
import { useDialogFocus } from "./dialogFocus";
import { radioKeys, radioTab } from "./radios";

export type InboxView = "smart" | "date";

const MARGIN = 8;

const VIEWS: { id: InboxView; label: string; caption: string }[] = [
  { id: "smart", label: "Smart", caption: "Priority first, then people. Notifications and newsletters grouped." },
  { id: "date", label: "By date", caption: "Everything together, newest first." },
];

/**
 * Under the inbox's title: how the inbox is shown, as two pictures to choose
 * from, and with several accounts, which of them.
 */
export function ViewChooser({
  anchor,
  current,
  onChoose,
  accounts,
  onSettings,
  onClose,
}: {
  anchor: Anchor;
  current: InboxView;
  onChoose: (view: InboxView) => void;
  /** With several accounts: all of them, or one. */
  accounts: { label: string; active: boolean; onSelect: () => void }[];
  onSettings: () => void;
  onClose: () => void;
}) {
  const [position, setPosition] = useState<{ left: number; top: number } | null>(null);
  const card = useRef<HTMLDivElement>(null);
  useDialogFocus(card);
  useShortcuts({ Escape: onClose }, { modal: true });

  useLayoutEffect(() => {
    const box = card.current?.getBoundingClientRect();
    if (!box) return;
    const left = "rect" in anchor ? anchor.rect.left : anchor.x;
    const top = "rect" in anchor ? anchor.rect.bottom + 6 : anchor.y;
    setPosition({
      left: Math.max(MARGIN, Math.min(left, window.innerWidth - box.width - MARGIN)),
      top: Math.max(MARGIN, Math.min(top, window.innerHeight - box.height - MARGIN)),
    });
  }, [anchor]);

  return (
    <div className="menu-layer" onMouseDown={onClose}>
      <div
        ref={card}
        className={position ? "view-chooser" : "view-chooser measuring"}
        role="dialog"
        aria-label="Inbox view"
        tabIndex={-1}
        style={position ?? undefined}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="chooser-head">
          <strong>Choose how the inbox looks</strong>
          <button
            type="button"
            className="icon-btn small"
            title="Inbox settings"
            aria-label="Inbox settings"
            onClick={() => {
              onClose();
              onSettings();
            }}
          >
            <Icon name="settings" size={15} />
          </button>
        </div>
        <div className="chooser-tiles" role="radiogroup" aria-label="Inbox view" onKeyDown={(event) => radioKeys(event, false)}>
          {VIEWS.map((view, index) => (
            <button
              key={view.id}
              type="button"
              role="radio"
              aria-checked={current === view.id}
              tabIndex={radioTab(current === view.id, index, VIEWS.some((one) => one.id === current))}
              className={current === view.id ? "chooser-tile chosen" : "chooser-tile"}
              onClick={() => {
                onChoose(view.id);
                onClose();
              }}
            >
              <span className={`chooser-picture ${view.id}`} aria-hidden="true">
                {view.id === "smart" ? (
                  <>
                    <span className="pic-card">
                      <span className="pic-line" />
                      <span className="pic-line short" />
                    </span>
                    <span className="pic-row">
                      <span className="pic-dot" />
                      <span className="pic-line" />
                    </span>
                    <span className="pic-row">
                      <span className="pic-dot" />
                      <span className="pic-chips" />
                    </span>
                  </>
                ) : (
                  Array.from({ length: 5 }, (_, index) => (
                    <span key={index} className="pic-row">
                      <span className="pic-dot" />
                      <span className="pic-line" />
                    </span>
                  ))
                )}
                {current === view.id && (
                  <span className="chooser-badge">
                    <Icon name="check" size={11} />
                  </span>
                )}
              </span>
              <strong>{view.label}</strong>
              <span className="muted">{view.caption}</span>
            </button>
          ))}
        </div>
        {accounts.length > 0 && (
          <div className="chooser-accounts">
            {accounts.map((account) => (
              <button
                key={account.label}
                type="button"
                className={account.active ? "chip chosen" : "chip"}
                onClick={() => {
                  account.onSelect();
                  onClose();
                }}
              >
                {account.label}
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
