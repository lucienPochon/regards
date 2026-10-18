import { type Action, actionKeys, choose } from "./actions";
import { Icon } from "./icons";
import { anchorOf } from "./Menu";
import { useShortcuts } from "./shortcuts";

/** In place of the reading pane while several messages are selected: what to do with all of them. */
export function BulkPane({ count, actions, onClear }: { count: number; actions: Action[]; onClear: () => void }) {
  useShortcuts({ Escape: onClear, ...actionKeys(actions) });
  const buttons = choose(actions, "archive", "snooze", "move", "pin", "aside", "read", "unread", "flag", "delete");
  return (
    <section className="reader bulk" aria-label="Selected messages">
      <div className="empty">
        <div className="stack" aria-hidden="true">
          <span />
          <span />
          <span>{count}</span>
        </div>
        <h2>
          {count} {count === 1 ? "message" : "messages"} selected
        </h2>
        <div className="bulk-actions">
          {buttons.map((action) => (
            <button
              key={action.id}
              type="button"
              className={action.danger ? "btn danger" : "btn"}
              title={action.keys[0]}
              onClick={(event) => action.run(anchorOf(event.currentTarget))}
            >
              <Icon name={action.icon} size={16} />
              {action.label}
            </button>
          ))}
        </div>
        <button type="button" className="btn ghost" onClick={onClear}>
          Clear selection <kbd>Esc</kbd>
        </button>
      </div>
    </section>
  );
}
