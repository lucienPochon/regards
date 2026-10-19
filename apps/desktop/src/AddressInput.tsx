import { type KeyboardEvent, type Ref, useEffect, useId, useState } from "react";
import type { Person } from "./accounts";
import { Avatar } from "./Avatar";
import { parseRecipients, suggestPeople } from "./compose";
import { typing, typingStart, withPicked } from "./recipients";

/**
 * A To, Cc or Bcc field that suggests people as an address is typed: those the
 * account writes to first, then those who write to it. Arrow keys choose, Enter
 * or Tab puts "Name <address>" in, Escape closes the list. Addresses already in
 * the field are not offered again.
 */
export function AddressInput({
  value,
  onChange,
  accountId,
  inputRef,
  label,
  placeholder,
}: {
  value: string;
  onChange: (value: string) => void;
  /** The account the message is from: its people come first. */
  accountId: string | null;
  inputRef?: Ref<HTMLInputElement>;
  label: string;
  placeholder?: string;
}) {
  const [open, setOpen] = useState(false);
  const [people, setPeople] = useState<Person[]>([]);
  const [active, setActive] = useState(0);
  const listId = useId();
  const typed = typing(value);
  const before = value.slice(0, typingStart(value));

  useEffect(() => {
    if (!open || !typed) {
      setPeople([]);
      return;
    }
    let current = true;
    const timer = window.setTimeout(() => {
      suggestPeople(typed, accountId).then(
        (found) => {
          if (!current) return;
          const already = new Set(parseRecipients(before).map((person) => person.address?.toLowerCase()));
          setPeople(found.filter((person) => !already.has(person.address?.toLowerCase())).slice(0, 8));
          setActive(0);
        },
        () => current && setPeople([]),
      );
    }, 120);
    return () => {
      current = false;
      window.clearTimeout(timer);
    };
  }, [open, typed, accountId, before]);

  const shown = open && people.length > 0;
  const pick = (person: Person) => {
    onChange(withPicked(value, person));
    setPeople([]);
  };
  const keys = (event: KeyboardEvent<HTMLInputElement>) => {
    if (!shown) return;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      setActive((at) => (at + step + people.length) % people.length);
    } else if (event.key === "Enter" || event.key === "Tab") {
      event.preventDefault();
      pick(people[active]);
    } else if (event.key === "Escape") {
      // The composer leaves Escape to an open list (see its escape handler).
      event.preventDefault();
      setOpen(false);
    }
  };

  return (
    <span className={shown ? "address-input suggesting" : "address-input"}>
      <input
        ref={inputRef}
        value={value}
        placeholder={placeholder}
        aria-label={label}
        role="combobox"
        aria-autocomplete="list"
        aria-expanded={shown}
        aria-controls={listId}
        aria-activedescendant={shown ? `${listId}-${active}` : undefined}
        autoComplete="off"
        spellCheck={false}
        onChange={(event) => {
          setOpen(true);
          onChange(event.target.value);
        }}
        onKeyDown={keys}
        onBlur={() => setOpen(false)}
      />
      {shown && (
        <ul className="address-suggestions" id={listId} role="listbox" aria-label={`People for ${label}`}>
          {people.map((person, at) => (
            <li
              key={person.address}
              id={`${listId}-${at}`}
              role="option"
              aria-selected={at === active}
              className={at === active ? "active" : undefined}
              // Keep the field focused: a click picks without a blur closing the list first.
              onMouseDown={(event) => event.preventDefault()}
              onMouseEnter={() => setActive(at)}
              onClick={() => pick(person)}
            >
              <Avatar person={person} size="xs" />
              <span className="address-suggestion-text">
                {person.name && <strong>{person.name}</strong>}
                <span>{person.address}</span>
              </span>
            </li>
          ))}
        </ul>
      )}
    </span>
  );
}
