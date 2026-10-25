import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { MoveResult, Person } from "./accounts";
import { usePendingHolds } from "./AgentHolds";
import { Avatar } from "./Avatar";
import { listDate } from "./format";
import { Icon } from "./icons";
import { type Note, NOTE_MAX, addNote, listNotes, noteSettings, removeNote, updateNote } from "./notes";
import type { Anchor } from "./Menu";
import { useShortcuts } from "./shortcuts";
import { type Explanation, type Policy, SECTION_LABELS, blockSender, blockableDomain, explain, reasonText, setSender, setSenderQuiet } from "./smart";
import { useDialogFocus } from "./dialogFocus";

export interface SenderRequest {
  anchor: Anchor;
  /** The message that led here: "why is this here?" is about it. */
  messageId: number;
  accountId: string;
  person: Person;
}

const MARGIN = 8;
/** Notes shown before "Show all". */
const NOTES_SHOWN = 3;

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
  onHolds,
}: {
  request: SenderRequest;
  onClose: () => void;
  /** Settings › Agents, where an agent's note about them waits. */
  onHolds?: () => void;
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
  useDialogFocus(card);
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
  // Everyone at their company or service; not offered for gmail.com and the like.
  const company = blockableDomain(address);
  const domain = sender?.blocked ? null : company;
  const everyone = explanation?.domain ?? null;
  // Their domain's choice, as one value: a section, priority, or the rules.
  const domainChoice = everyone?.policy ?? (everyone?.priority ? "priority" : "auto");
  const chooseForDomain = (value: string) => {
    if (!company) return;
    const policy = value === "auto" || value === "priority" ? null : (value as Policy);
    setSender(accountId, company, { policy, priority: value === "priority" }).then(load, onFailed);
  };
  const blockDomain = () => {
    if (!domain) return;
    onClose();
    blockSender(accountId, domain).then(onBlocked, onFailed);
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
        tabIndex={-1}
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
        {address && (
          <SenderNotes
            accountId={accountId}
            address={address}
            name={person.name ?? null}
            onHolds={onHolds}
            onFailed={onFailed}
          />
        )}
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
            onClick={() => choose(sender?.policy ?? null, !(sender?.priority ?? false))}
            disabled={!sender || everyone?.priority === true}
          >
            <Icon name="bolt" size={16} />
            <span>
              Priority sender
              {everyone?.priority && <span className="muted sender-row-detail">Everyone at {company?.slice(1)} is</span>}
            </span>
            <span className={sender?.priority ? "toggle on" : "toggle"} aria-hidden="true" />
          </button>
          {company && (
            <div className="sender-row sender-domain">
              <Icon name="people" size={16} />
              <span>
                <span>Everyone at {company.slice(1)}</span>
                <select
                className="sender-section"
                aria-label={`Where mail from everyone at ${company.slice(1)} goes`}
                value={domainChoice}
                disabled={!explanation}
                onChange={(event) => chooseForDomain(event.target.value)}
              >
                <option value="auto">Sorted automatically</option>
                <option value="priority">Priority</option>
                <option value="people">Always People</option>
                <option value="notifications">Always Notifications</option>
                  <option value="newsletters">Always Newsletters</option>
                </select>
              </span>
            </div>
          )}
          <button
            type="button"
            className="sender-row"
            role="switch"
            aria-checked={sender?.quiet ?? false}
            disabled={!sender}
            onClick={() => {
              if (address) setSenderQuiet(accountId, address, !(sender?.quiet ?? false)).then(load, onFailed);
            }}
          >
            <Icon name="bellOff" size={16} />
            <span>
              No notifications
              <span className="muted sender-row-detail">Their mail comes in quietly</span>
            </span>
            <span className={sender?.quiet ? "toggle on" : "toggle"} aria-hidden="true" />
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
          {domain && (
            <button type="button" className="sender-row danger" onClick={blockDomain}>
              <Icon name="thumbDown" size={16} />
              <span>
                Block everyone at {domain.slice(1)}
                <span className="muted sender-row-detail">Their mail goes to Junk, and so does what anyone there sends later</span>
              </span>
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

/**
 * What the account knows of them (ADR-0020): one fact per note, by the
 * person or an agent, kept on this computer. Each can be edited (it is the
 * person's then) or removed; a new one is added at the bottom. Hidden while
 * notes are off.
 */
function SenderNotes({
  accountId,
  address,
  name,
  onHolds,
  onFailed,
}: {
  accountId: string;
  address: string;
  name: string | null;
  onHolds?: () => void;
  onFailed: (error: unknown) => void;
}) {
  const [on, setOn] = useState<boolean | null>(null);
  const [notes, setNotes] = useState<Note[]>([]);
  const [all, setAll] = useState(false);
  const [editing, setEditing] = useState<{ id: number; text: string } | null>(null);
  const [adding, setAdding] = useState("");
  const load = useCallback(() => {
    listNotes(accountId, address).then(setNotes, onFailed);
  }, [accountId, address, onFailed]);
  useEffect(load, [load]);
  useEffect(() => {
    noteSettings().then(
      (settings) => setOn(settings.on),
      () => setOn(false),
    );
  }, []);
  // Agents' notes about them that wait for the person.
  const waiting = usePendingHolds().filter((hold) => {
    const about = (hold.params as { address?: unknown } | null)?.address;
    return hold.reason === "note" && hold.account_id === accountId && String(about ?? "").toLowerCase() === address.toLowerCase();
  }).length;
  if (on !== true) return null;

  const who = name || address;
  const first = who.split(/[\s@]/)[0] || who;
  const shown = all ? notes : notes.slice(0, NOTES_SHOWN);
  const add = () => {
    const text = adding.trim();
    if (!text) return;
    addNote({ account_id: accountId, address, text }).then(() => {
      setAdding("");
      setAll(true);
      load();
    }, onFailed);
  };
  const save = () => {
    const text = editing?.text.trim();
    if (!editing || !text) return;
    updateNote(editing.id, text).then(() => {
      setEditing(null);
      load();
    }, onFailed);
  };
  const remove = (id: number) => {
    removeNote(id).then(load, onFailed);
  };
  const about = (note: Note) =>
    [
      note.by === "person" ? "You" : null,
      listDate(note.updated_at),
      note.from_subject ? `from “${note.from_subject}”` : null,
      note.from_address && note.from_address !== note.address ? `sent by ${note.from_address}` : null,
    ]
      .filter(Boolean)
      .join(" · ");

  return (
    <section className="sender-notes" aria-label={`Notes about ${who}`}>
      <div className="notes-head">
        <Icon name="text" size={16} />
        <strong>Notes</strong>
        {notes.length > 0 && <span className="muted">{notes.length}</span>}
        <span className="notes-where muted">On this computer only</span>
      </div>
      {notes.length === 0 && (
        <p className="notes-empty muted">Nothing yet. Add what&apos;s worth remembering: who decides, what they prefer.</p>
      )}
      {notes.length > 0 && (
        <ul className="notes-list">
          {shown.map((note) => (
            <li key={note.id} className="note-item">
              {editing?.id === note.id ? (
                <div className="note-edit">
                  <input
                    aria-label="Note"
                    maxLength={NOTE_MAX}
                    value={editing.text}
                    onChange={(event) => setEditing({ id: note.id, text: event.target.value })}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") save();
                      if (event.key === "Escape") {
                        event.stopPropagation();
                        setEditing(null);
                      }
                    }}
                    autoFocus
                  />
                  <div className="note-edit-buttons">
                    <button type="button" className="btn small" onClick={() => setEditing(null)}>
                      Cancel
                    </button>
                    <button type="button" className="btn small primary" onClick={save} disabled={!editing.text.trim()}>
                      Save
                    </button>
                  </div>
                </div>
              ) : (
                <>
                  <p className="note-text">{note.text}</p>
                  <p className="note-meta muted">
                    {note.by === "agent" && (
                      <span className="note-agent">
                        <Icon name="agent" size={11} />
                        Agent
                      </span>
                    )}
                    <span>{about(note)}</span>
                  </p>
                  <span className="note-actions">
                    <button
                      type="button"
                      className="icon-btn small"
                      title="Edit the note"
                      aria-label="Edit the note"
                      onClick={() => setEditing({ id: note.id, text: note.text })}
                    >
                      <Icon name="compose" size={13} />
                    </button>
                    <button
                      type="button"
                      className="icon-btn small"
                      title="Remove the note"
                      aria-label="Remove the note"
                      onClick={() => remove(note.id)}
                    >
                      <Icon name="trash" size={13} />
                    </button>
                  </span>
                </>
              )}
            </li>
          ))}
        </ul>
      )}
      {notes.length > NOTES_SHOWN && (
        <button type="button" className="notes-more" onClick={() => setAll(!all)}>
          {all ? "Show fewer" : `Show all ${notes.length}`}
        </button>
      )}
      {waiting > 0 && onHolds && (
        <button type="button" className="notes-waiting" onClick={onHolds}>
          <Icon name="agent" size={13} />
          <span>{waiting === 1 ? "1 note from an agent waits for you" : `${waiting} notes from agents wait for you`}</span>
          <Icon name="chevronRight" size={13} />
        </button>
      )}
      <div className="notes-add">
        <input
          aria-label={`Add a note about ${who}`}
          placeholder={`Add a note about ${first}…`}
          maxLength={NOTE_MAX}
          value={adding}
          onChange={(event) => setAdding(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") add();
          }}
        />
        <button type="button" className="btn small primary" onClick={add} disabled={!adding.trim()}>
          Add
        </button>
      </div>
    </section>
  );
}
