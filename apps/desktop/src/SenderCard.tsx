import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { MoveResult, Person } from "./accounts";
import { Avatar } from "./Avatar";
import { Icon } from "./icons";
import type { Anchor } from "./Menu";
import { useShortcuts } from "./shortcuts";
import { type Explanation, type Policy, SECTION_LABELS, blockSender, explain, reasonText, setSender } from "./smart";

export interface SenderRequest {
  anchor: Anchor;
  /** The message that led here: "why is this here?" is about it. */
  messageId: number;
  accountId: string;
  person: Person;
}

const MARGIN = 8;

/**
 * Who sent a message, and what to do about them: where their mail goes (or
 * let the rules decide), whether they are a priority, why this message is
 * where it is, and blocking them.
 */
export function SenderCard({
  request,
  onClose,
  onWrite,
  onSearch,
  onBlocked,
  onFailed,
}: {
  request: SenderRequest;
  onClose: () => void;
  /** Start a message to them. */
  onWrite: (person: Person) => void;
  /** Search all mail from them. */
  onSearch?: (address: string) => void;
  onBlocked: (result: MoveResult) => void;
  onFailed: (error: unknown) => void;
}) {
  const [explanation, setExplanation] = useState<Explanation | null>(null);
  const [copied, setCopied] = useState(false);
  const [position, setPosition] = useState<{ left: number; top: number } | null>(null);
  const card = useRef<HTMLDivElement>(null);
  useShortcuts({ Escape: onClose }, { modal: true });
  const { person, accountId, messageId } = request;
  const address = explanation?.sender?.address ?? person.address ?? "";
  const sender = explanation?.sender ?? null;

  const load = useCallback(() => {
    explain(messageId).then(setExplanation, onFailed);
  }, [messageId, onFailed]);
  useEffect(load, [load]);

  useLayoutEffect(() => {
    const box = card.current?.getBoundingClientRect();
    if (!box) return;
    const { anchor } = request;
    let left = "rect" in anchor ? anchor.rect.left : anchor.x;
    let top = "rect" in anchor ? anchor.rect.bottom + 6 : anchor.y;
    if (top + box.height > window.innerHeight - MARGIN && "rect" in anchor) top = anchor.rect.top - box.height - 6;
    left = Math.max(MARGIN, Math.min(left, window.innerWidth - box.width - MARGIN));
    top = Math.max(MARGIN, Math.min(top, window.innerHeight - box.height - MARGIN));
    setPosition({ left, top });
  }, [request, explanation]);

  const choose = (policy: Policy | null, priority: boolean) => {
    if (!address) return;
    setSender(accountId, address, { policy, priority }).then(load, onFailed);
  };
  const block = () => {
    if (!address) return;
    onClose();
    blockSender(accountId, address, !sender?.blocked).then(onBlocked, onFailed);
  };
  const copy = () => {
    void navigator.clipboard?.writeText(address).then(() => setCopied(true), () => {});
  };

  return (
    <div className="menu-layer" onMouseDown={onClose}>
      <div
        ref={card}
        className={position ? "sender-card" : "sender-card measuring"}
        role="dialog"
        aria-label={`About ${person.name ?? address}`}
        style={position ?? undefined}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="sender-head">
          <Avatar person={person} size="lg" />
          <div className="sender-who">
            <strong>{person.name || address}</strong>
            <span className="sender-address">
              {address}
              <button type="button" className="icon-btn small" title={copied ? "Copied" : "Copy the address"} aria-label="Copy the address" onClick={copy}>
                <Icon name={copied ? "check" : "copy"} size={13} />
              </button>
            </span>
            <select
              className="sender-section"
              aria-label="Where their mail goes"
              value={sender?.policy ?? "auto"}
              disabled={!sender}
              onChange={(event) => {
                const value = event.target.value;
                choose(value === "auto" ? null : (value as Policy), sender?.priority ?? false);
              }}
            >
              <option value="auto">Sorted automatically</option>
              <option value="people">Always People</option>
              <option value="notifications">Always Notifications</option>
              <option value="newsletters">Always Newsletters</option>
            </select>
          </div>
        </div>
        <div className="sender-rows">
          <button type="button" className="sender-row" onClick={() => onWrite(person)}>
            <Icon name="compose" size={16} />
            <span>Write to them</span>
          </button>
          {onSearch && (
            <button type="button" className="sender-row" onClick={() => onSearch(address)} disabled={!address}>
              <Icon name="search" size={16} />
              <span>
                Search mail from them
                <span className="muted sender-row-detail">{address}</span>
              </span>
            </button>
          )}
          <button
            type="button"
            className="sender-row"
            role="switch"
            aria-checked={sender?.priority ?? false}
            disabled={!sender}
            onClick={() => choose(sender?.policy ?? null, !(sender?.priority ?? false))}
          >
            <Icon name="bolt" size={16} />
            <span>Priority sender</span>
            <span className={sender?.priority ? "toggle on" : "toggle"} aria-hidden="true" />
          </button>
          <div className="sender-row why">
            <Icon name="info" size={16} />
            <span>
              <strong>Why is this here?</strong>
              <span className="muted">
                {explanation ? `In ${SECTION_LABELS[explanation.section]}. ${reasonText(explanation)}` : "…"}
              </span>
            </span>
          </div>
          <button type="button" className="sender-row danger" onClick={block} disabled={!address}>
            <Icon name="thumbDown" size={16} />
            <span>{sender?.blocked ? "Unblock sender" : "Block sender"}</span>
          </button>
        </div>
      </div>
    </div>
  );
}
