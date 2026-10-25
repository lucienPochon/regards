import {
  type ClipboardEvent,
  type KeyboardEvent,
  type MutableRefObject,
  type Ref,
  useEffect,
  useId,
  useRef,
  useState,
} from "react";
import type { Person } from "./accounts";
import { Avatar } from "./Avatar";
import { forgetPerson, formatRecipients, invalidAddresses, parseRecipients, suggestPeople } from "./compose";
import { Icon } from "./icons";
import { usePrefs } from "./prefs";
import { fieldText, isAddress, marked, pastedList, splitTyped, typingStart, withAdded } from "./recipients";
import { t } from "./i18n";

/**
 * A To, Cc or Bcc field: its people as chips, then what is being typed, with
 * people suggested as it is typed (those the account writes to first), not
 * those in this field already or in `besides` (the other fields).
 *
 * - Enter or Tab takes the suggestion chosen (the arrows choose) and leaves
 *   the field ready for the next one. With no list, Enter or Tab takes the
 *   best one for what is typed, or what is typed as it is; so do a comma or
 *   a semicolon, and a space after a whole address. Tab with nothing typed
 *   moves on, as in any field.
 * - Backspace with nothing typed goes to the last chip, and again removes it.
 *   The arrows go from chip to chip; Delete removes one; Enter or a double
 *   click puts it back to be edited.
 * - A pasted list (by commas, lines or tabs) becomes chips; so does what is
 *   typed when the field is left, so nothing typed is lost.
 * - Shift+Delete on a suggestion, or its ×, stops suggesting that person.
 *
 * `value` stays text (`Name <address>, …`, then what is being typed): what
 * the composer saves and sends.
 */
export function AddressInput({
  value,
  onChange,
  accountId,
  inputRef,
  label,
  placeholder,
  besides = "",
}: {
  value: string;
  onChange: (value: string) => void;
  /** The account the message is from: its people come first. */
  accountId: string | null;
  inputRef?: Ref<HTMLInputElement>;
  label: string;
  placeholder?: string;
  /** The other address fields' text: their people are not suggested here. */
  besides?: string;
}) {
  const [typed, setTyped] = useState("");
  const [open, setOpen] = useState(false);
  const [suggestions, setSuggestions] = useState<Person[]>([]);
  const [active, setActive] = useState(0);
  const listId = useId();
  const field = useRef<HTMLInputElement | null>(null);
  const chips = useRef<(HTMLButtonElement | null)[]>([]);
  // Settings › Composer › Suggest people: off, nothing is suggested.
  const suggesting = usePrefs().suggestPeople;

  // What this field last wrote. Text from elsewhere (a draft opened, a
  // reply's addresses) is all chips, with nothing being typed.
  const written = useRef(value);
  const ours = value === written.current;
  const draft = ours ? typed : "";
  const people = parseRecipients(draft ? value.slice(0, value.length - draft.length) : value);
  useEffect(() => {
    if (value === written.current) return;
    written.current = value;
    setTyped("");
  }, [value]);

  const query = suggesting ? draft.trim() : "";
  const taken = [...people, ...parseRecipients(besides)].map((person) => person.address?.toLowerCase() ?? "");
  const takenKey = taken.join(",");
  const fresh = (found: Person[]) => {
    const seen = new Set(takenKey.split(","));
    return found.filter((person) => !seen.has(person.address?.toLowerCase() ?? ""));
  };

  useEffect(() => {
    if (!open || !query) {
      setSuggestions([]);
      return;
    }
    let current = true;
    const timer = window.setTimeout(() => {
      suggestPeople(query, accountId).then(
        (found) => {
          if (!current) return;
          setSuggestions(fresh(found).slice(0, 8));
          setActive(0);
        },
        () => current && setSuggestions([]),
      );
    }, 120);
    return () => {
      current = false;
      window.clearTimeout(timer);
    };
    // `fresh` reads `takenKey` alone.
  }, [open, query, accountId, takenKey]);

  const shown = open && suggestions.length > 0;
  const chosen = Math.min(active, suggestions.length - 1);

  const write = (nextPeople: Person[], nextTyped: string) => {
    const text = fieldText(nextPeople, nextTyped);
    written.current = text;
    setTyped(nextTyped);
    onChange(text);
  };
  /** What is typed (or `text`) becomes chips, as it is. */
  const commit = (text = draft) => {
    if (text.trim()) write(withAdded(people, parseRecipients(text)), "");
  };
  const pick = (person: Person) => {
    write(withAdded(people, [person]), "");
    setSuggestions([]);
  };
  /** Enter or Tab with no list: the best person for what is typed, else it as it is. */
  const complete = async () => {
    const text = draft.trim();
    if (!suggesting || isAddress(text) || /<[^<>]*>/.test(text)) return commit();
    const asked = written.current;
    const best = fresh(await suggestPeople(text, accountId).catch(() => []))[0];
    // Typing went on meanwhile: that wins.
    if (written.current !== asked) return;
    if (best) pick(best);
    else commit();
  };
  const forget = (person: Person) => {
    setSuggestions((list) => list.filter((other) => other.address !== person.address));
    if (person.address) void forgetPerson(person.address, accountId).catch(() => undefined);
  };
  const remove = (at: number) => write(people.filter((_, index) => index !== at), draft);
  const edit = (at: number) => {
    write(
      people.filter((_, index) => index !== at),
      formatRecipients([people[at]]),
    );
    setOpen(false);
    field.current?.focus();
  };

  const keys = (event: KeyboardEvent<HTMLInputElement>) => {
    const tab = event.key === "Tab" && !event.shiftKey && !event.altKey && !event.ctrlKey && !event.metaKey;
    if (shown) {
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        const step = event.key === "ArrowDown" ? 1 : -1;
        setActive((chosen + step + suggestions.length) % suggestions.length);
        return;
      }
      if (event.key === "Enter" || tab) {
        event.preventDefault();
        pick(suggestions[chosen]);
        return;
      }
      if (event.key === "Escape") {
        // The composer leaves Escape to an open list (see its escape handler).
        event.preventDefault();
        setOpen(false);
        return;
      }
      if (event.key === "Delete" && event.shiftKey) {
        event.preventDefault();
        forget(suggestions[chosen]);
        return;
      }
    }
    if ((event.key === "Enter" || tab) && draft.trim()) {
      event.preventDefault();
      void complete();
    } else if (event.key === " " && isAddress(draft.trim()) && event.currentTarget.selectionStart === draft.length) {
      event.preventDefault();
      commit();
    } else if ((event.key === "Backspace" || event.key === "ArrowLeft") && !draft && people.length > 0) {
      event.preventDefault();
      chips.current[people.length - 1]?.focus();
    }
  };
  const typing = (text: string) => {
    setOpen(true);
    const start = typingStart(text);
    if (start === 0) return write(people, text);
    // A comma or semicolon: what is before it is done; a name half typed
    // there takes the suggestion chosen for it.
    const done = parseRecipients(text.slice(0, start));
    const last = done[done.length - 1];
    if (shown && last && !last.name && !isAddress(last.address ?? "")) done[done.length - 1] = suggestions[chosen];
    write(withAdded(people, done), text.slice(start).trimStart());
    setSuggestions([]);
  };
  const paste = (event: ClipboardEvent<HTMLInputElement>) => {
    const pasted = event.clipboardData.getData("text/plain");
    // One address, or a few letters: typed as they are.
    if (!/[,;\r\n\t]/.test(pasted) && !/\S\s+\S/.test(pasted.trim())) return;
    event.preventDefault();
    const input = event.currentTarget;
    const text =
      draft.slice(0, input.selectionStart ?? draft.length) + pastedList(pasted) + draft.slice(input.selectionEnd ?? draft.length);
    const { people: added, rest } = splitTyped(text);
    const last = rest.trim();
    const whole = isAddress(last) || /<[^<>]+>/.test(last);
    write(withAdded(people, [...added, ...(whole ? parseRecipients(last) : [])]), whole ? "" : rest.trimStart());
  };
  const chipKeys = (at: number) => (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.key === "Backspace" || event.key === "Delete") {
      event.preventDefault();
      remove(at);
      field.current?.focus();
    } else if (event.key === "ArrowLeft") {
      event.preventDefault();
      chips.current[Math.max(0, at - 1)]?.focus();
    } else if (event.key === "ArrowRight") {
      event.preventDefault();
      if (at < people.length - 1) chips.current[at + 1]?.focus();
      else field.current?.focus();
    } else if (event.key === "Enter" || event.key === "F2") {
      event.preventDefault();
      edit(at);
    }
  };
  const highlight = (text: string) =>
    marked(text, query).map((piece, at) => (piece.mark ? <mark key={at}>{piece.text}</mark> : piece.text));

  return (
    <span
      className={shown ? "address-input suggesting" : "address-input"}
      // A click beside the chips goes to the typing.
      onMouseDown={(event) => {
        if (event.target !== event.currentTarget) return;
        event.preventDefault();
        field.current?.focus();
      }}
    >
      {people.map((person, at) => {
        const named = person.name || person.address || "";
        const wrong = invalidAddresses([person]).length > 0;
        const full = formatRecipients([person]);
        return (
          <span key={`${at}-${person.address}`} className={wrong ? "address-chip invalid" : "address-chip"}>
            <button
              type="button"
              tabIndex={-1}
              ref={(node) => {
                chips.current[at] = node;
              }}
              className="address-chip-name"
              title={wrong ? t("Not an address: {address}", { address: person.address ?? "" }) : full}
              aria-label={
                wrong
                  ? t("{person}, not an address. Backspace removes it, Enter edits it.", { person: full })
                  : t("{person}. Backspace removes it, Enter edits it.", { person: full })
              }
              onKeyDown={chipKeys(at)}
              onDoubleClick={() => edit(at)}
            >
              {named}
            </button>
            <button
              type="button"
              tabIndex={-1}
              className="address-chip-remove"
              aria-label={`Remove ${named}`}
              // The typing keeps the focus.
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => remove(at)}
            >
              <Icon name="close" size={12} />
            </button>
          </span>
        );
      })}
      <input
        ref={(node) => {
          field.current = node;
          if (typeof inputRef === "function") inputRef(node);
          else if (inputRef) (inputRef as MutableRefObject<HTMLInputElement | null>).current = node;
        }}
        value={draft}
        placeholder={people.length ? undefined : placeholder}
        aria-label={label}
        role="combobox"
        aria-autocomplete="list"
        aria-expanded={shown}
        aria-controls={listId}
        aria-activedescendant={shown ? `${listId}-${chosen}` : undefined}
        autoComplete="off"
        spellCheck={false}
        onChange={(event) => typing(event.target.value)}
        onKeyDown={keys}
        onPaste={paste}
        onBlur={() => {
          setOpen(false);
          commit();
        }}
      />
      {shown && (
        <ul className="address-suggestions" id={listId} role="listbox" aria-label={t("People for {field}", { field: label })}>
          {suggestions.map((person, at) => (
            <li
              key={person.address}
              id={`${listId}-${at}`}
              role="option"
              aria-selected={at === chosen}
              className={at === chosen ? "active" : undefined}
              // Keep the field focused: a click picks without a blur closing the list first.
              onMouseDown={(event) => event.preventDefault()}
              onMouseEnter={() => setActive(at)}
              onClick={() => pick(person)}
            >
              <Avatar person={person} size="xs" />
              <span className="address-suggestion-text">
                {person.name && <strong>{highlight(person.name)}</strong>}
                <span>{highlight(person.address ?? "")}</span>
              </span>
              <button
                type="button"
                tabIndex={-1}
                className="address-suggestion-forget"
                title={t("Don't suggest (Shift+Delete)")}
                aria-label={`Don't suggest ${person.name || person.address}`}
                onClick={(event) => {
                  event.stopPropagation();
                  forget(person);
                }}
              >
                <Icon name="close" size={12} />
              </button>
            </li>
          ))}
        </ul>
      )}
      <span className="sr-only" aria-live="polite">
        {shown ? `${suggestions.length} ${suggestions.length === 1 ? "person" : "people"} suggested` : ""}
      </span>
    </span>
  );
}
