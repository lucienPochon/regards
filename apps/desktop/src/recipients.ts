import type { Person } from "./accounts";
import { formatRecipients } from "./compose";

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

/** The field with the address being typed replaced by a chosen person, ready for the next one. */
export function withPicked(text: string, person: Person): string {
  const before = text.slice(0, typingStart(text)).trimEnd();
  const lead = before ? `${before.replace(/[,;]$/, "")}, ` : "";
  return `${lead}${formatRecipients([person])}, `;
}
