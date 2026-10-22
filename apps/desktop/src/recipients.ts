import type { Person } from "./accounts";
import { formatRecipients, parseRecipients } from "./compose";

/** Where the address being typed starts: after the last comma or semicolon outside quotes and <…>. */
export function typingStart(text: string): number {
  let quoted = false;
  let angled = false;
  let start = 0;
  for (let at = 0; at < text.length; at++) {
    const char = text[at];
    if (char === '"') quoted = !quoted;
    else if (char === "<" && !quoted) angled = true;
    else if (char === ">" && !quoted) angled = false;
    else if ((char === "," || char === ";") && !quoted && !angled) start = at + 1;
  }
  return start;
}

/** What is being typed now: the last address, trimmed. */
export const typing = (text: string) => text.slice(typingStart(text)).trim();

/** What a field's text holds: the people written out (each followed by a separator), and what is still being typed. */
export function splitTyped(text: string): { people: Person[]; rest: string } {
  const start = typingStart(text);
  return { people: parseRecipients(text.slice(0, start)), rest: text.slice(start) };
}

/** Whether `text` is an address by itself (`name@example.org`), as sending would take it. */
export const isAddress = (text: string) => /^[^\s@<>,";]+@[^\s@<>,";]+\.[^\s@<>,";]+$/.test(text);

/** A field's text: its people, then what is being typed, if anything. */
export function fieldText(people: Person[], typed: string): string {
  const written = formatRecipients(people);
  return typed ? (written ? `${written}, ${typed}` : typed) : written;
}

/** `people` with `added` after them, those already there (by address, any case) left out. */
export function withAdded(people: Person[], added: Person[]): Person[] {
  const seen = new Set(people.map((person) => person.address?.toLowerCase()));
  const kept = [...people];
  for (const person of added) {
    const address = person.address?.toLowerCase();
    if (!address || seen.has(address)) continue;
    seen.add(address);
    kept.push(person);
  }
  return kept;
}

/**
 * Pasted text as a field's text: one address per line, or by tabs (a
 * spreadsheet's column or row), or bare addresses split by spaces, become a
 * list split by commas. A name with its address stays as it is.
 */
export function pastedList(text: string): string {
  const lines = text
    .split(/[\r\n\t]+/)
    .map((line) => line.trim())
    .filter(Boolean)
    .join(", ");
  const words = lines.split(/\s+/);
  return words.length > 1 && words.every((word) => isAddress(word.replace(/[,;]$/, ""))) ? words.join(", ").replace(/([,;]),/g, "$1") : lines;
}

/** Text as one letter folded per letter: lower case, without accents (as the daemon matches). */
const foldLetter = (letter: string) => letter.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();

/**
 * `text` in pieces, those that start with a typed word marked (accents and
 * case aside), as the daemon matched them: the start of a word of a name, or
 * of a part of an address.
 */
export function marked(text: string, typed: string): { text: string; mark: boolean }[] {
  const words = [...typed.matchAll(/[\p{L}\p{N}]+/gu)].map((match) => [...match[0]].map(foldLetter).join(""));
  const pieces: { text: string; mark: boolean }[] = [];
  const add = (piece: string, mark: boolean) => {
    if (!piece) return;
    const last = pieces[pieces.length - 1];
    if (last && last.mark === mark) last.text += piece;
    else pieces.push({ text: piece, mark });
  };
  for (const part of text.split(/([^\p{L}\p{N}]+)/u)) {
    const letters = [...part];
    const folded = letters.map(foldLetter);
    // The longest typed word this part starts with, in letters of the part.
    let longest = 0;
    for (const word of words) {
      let length = 0;
      let at = 0;
      while (at < letters.length && length < word.length) {
        length += folded[at].length;
        at++;
      }
      if (at > longest && folded.slice(0, at).join("") === word) longest = at;
    }
    add(letters.slice(0, longest).join(""), true);
    add(letters.slice(longest).join(""), false);
  }
  return pieces;
}
