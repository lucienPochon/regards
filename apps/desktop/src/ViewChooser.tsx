import { useLayoutEffect, useRef, useState } from "react";
import { Icon, type IconName } from "./icons";
import type { Anchor } from "./Menu";
import { useShortcuts } from "./shortcuts";
import { useDialogFocus } from "./dialogFocus";
import { radioKeys, radioTab } from "./radios";

export type InboxView = "smart" | "date" | "cards";

const MARGIN = 8;

// The ids are kept in the saved layout: "smart" is By priority, "cards" By category.
const VIEWS: { id: InboxView; label: string; kind: string; about: string; badge: IconName }[] = [
  {
    id: "smart",
    label: "By priority",
    kind: "Smart Inbox",
    about: "Priority first, then people by date; notifications and newsletters in one row each.",
    badge: "boltFilled",
  },
  {
    id: "cards",
    label: "By category",
    kind: "Smart Inbox",
    about: "Unread mail under a heading per category; what you have seen goes below.",
    badge: "sections",
  },
  { id: "date", label: "By date", kind: "Classic inbox", about: "Everything together, newest first.", badge: "calendar" },
];

type Hue = "accent" | "warm" | "blue" | "violet";

/** A message in a picture: who (a dot, or a glyph) over the sender and a line of text. */
function PicRow({ hue, glyph }: { hue?: Hue; glyph?: IconName }) {
  return (
    <span className="pic-row">
      {glyph ? (
        <span className="pic-glyph">
          <Icon name={glyph} size={10} />
        </span>
      ) : (
        <span className={`pic-dot ${hue ?? ""}`} />
      )}
      <span className="pic-text">
        <span className="pic-name" />
        <span className="pic-line" />
      </span>
    </span>
  );
}

/** A heading in a picture: a category's glyph, or just a short bar (a day). */
function PicHead({ glyph }: { glyph?: IconName }) {
  return (
    <span className="pic-head">
      {glyph && <Icon name={glyph} size={10} />}
      <span className="pic-bar" />
    </span>
  );
}

/** A folded group's row: its glyph, then a few of its senders. */
function PicGroup({ glyph, hues }: { glyph: IconName; hues: Hue[] }) {
  return (
    <span className="pic-row">
      <span className="pic-glyph">
        <Icon name={glyph} size={10} />
      </span>
      <span className="pic-senders">
        {hues.map((hue, index) => (
          <span key={index} className={`pic-mini ${hue}`} />
        ))}
      </span>
    </span>
  );
}

/** The view, drawn small: what sets it apart is how its rows are arranged. */
function Picture({ view }: { view: InboxView }) {
  if (view === "smart") {
    return (
      <>
        <PicHead glyph="boltFilled" />
        <PicRow hue="warm" />
        <PicRow hue="blue" />
        <PicHead />
        <PicRow hue="accent" />
        <PicGroup glyph="bell" hues={["warm", "blue", "violet"]} />
        <PicGroup glyph="newspaper" hues={["accent", "warm"]} />
      </>
    );
  }
  if (view === "cards") {
    return (
      <>
        <PicHead glyph="people" />
        <PicRow hue="warm" />
        <PicRow hue="blue" />
        <PicHead glyph="bell" />
        <PicRow hue="violet" />
        <PicHead glyph="newspaper" />
        <PicRow hue="accent" />
      </>
    );
  }
  return (
    <>
      {(["blue", "accent", "warm", "blue", "violet", "accent"] as Hue[]).map((hue, index) => (
        <PicRow key={index} hue={hue} />
      ))}
    </>
  );
}

/**
 * Under the inbox's title: how the inbox is shown, as three pictures to
 * choose from, and with several accounts, which of them.
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
                <Picture view={view.id} />
                <span className="pic-badge">
                  <Icon name={view.badge} size={13} />
                </span>
              </span>
              <strong>{view.label}</strong>
              <span className="muted">{view.kind}</span>
              <span className="sr-only">{view.about}</span>
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
