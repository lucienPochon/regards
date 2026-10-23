import type { MessageDetail, Person } from "./accounts";
import { fullDate, personName } from "./format";

const who = (person: Person | null | undefined) =>
  person?.address && person.name ? `${person.name} <${person.address}>` : (person?.address ?? personName(person));

/** The message's head as label/value pairs: From, To, Cc, Date, Subject. */
function head(detail: MessageDetail): [string, string][] {
  const { message } = detail;
  return [
    ["From", who(message.from)],
    ["To", detail.to.map(who).join(", ")],
    ["Cc", detail.cc.map(who).join(", ")],
    ["Date", fullDate(message.sent_at ?? message.received_at)],
    ["Subject", message.subject || "(no subject)"],
  ].filter(([, value]) => value) as [string, string][];
}

/** The message as Markdown: its head in bold labels, then its text, as it came (quoted lines stay quoted). */
export function asMarkdown(detail: MessageDetail): string {
  const lines = head(detail).map(([label, value]) => `**${label}:** ${value.replace(/([*_`[\]])/g, "\\$1")}`);
  return `${lines.join("  \n")}\n\n${detail.text.trim()}\n`;
}

const escape = (text: string) => text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

/**
 * Print the message: its head and its text, in a frame of its own that is
 * removed afterwards. Text, not the formatted version: it prints the same
 * everywhere and loads nothing from the web.
 */
export function printMessage(detail: MessageDetail): void {
  const frame = document.createElement("iframe");
  // No scripts in it; the same origin, so this page can ask it to print.
  frame.setAttribute("sandbox", "allow-modals allow-same-origin");
  frame.setAttribute("aria-hidden", "true");
  frame.style.position = "fixed";
  frame.style.width = "0";
  frame.style.height = "0";
  frame.style.border = "0";
  const rows = head(detail)
    .map(([label, value]) => `<tr><th>${escape(label)}</th><td>${escape(value)}</td></tr>`)
    .join("");
  frame.srcdoc = `<!doctype html><html><head><meta charset="utf-8"><title>${escape(detail.message.subject || "Message")}</title></head><body><table>${rows}</table><hr><pre>${escape(detail.text.trim())}</pre></body></html>`;
  frame.addEventListener("load", () => {
    const page = frame.contentDocument;
    const window = frame.contentWindow;
    if (!page || !window) return;
    // Set from here: the app's content rules allow no style sheet inside.
    page.body.style.font = "12pt/1.5 system-ui, sans-serif";
    for (const cell of page.querySelectorAll<HTMLElement>("th")) {
      cell.style.textAlign = "left";
      cell.style.paddingRight = "12px";
      cell.style.fontWeight = "600";
    }
    const text = page.querySelector<HTMLElement>("pre");
    if (text) {
      text.style.whiteSpace = "pre-wrap";
      text.style.font = "inherit";
    }
    window.addEventListener("afterprint", () => frame.remove());
    window.focus();
    window.print();
    // If the print dialog never says it closed, the frame goes anyway.
    setTimeout(() => frame.isConnected && frame.remove(), 60_000);
  });
  document.body.append(frame);
}

/** A piece of a plain-text message: words as they are, or a web address that opens. */
export type TextPart = { text: string; url?: undefined } | { text: string; url: string };

// https:// addresses, and www. ones written without it. http:// is left as
// text: only https:// pages are opened from Regards.
const WEB_ADDRESS = /\bhttps:\/\/[^\s<>"]+|\bwww\.[^\s<>"]+\.[^\s<>"]+/gi;

/** Punctuation that ends a sentence, not the address: "see https://x.test/a." */
function trimmed(address: string): string {
  let end = address.length;
  for (;;) {
    const last = address[end - 1];
    if (".,;:!?'\"»…".includes(last)) end--;
    else if (")]}".includes(last)) {
      const open = { ")": "(", "]": "[", "}": "{" }[last] as string;
      const part = address.slice(0, end);
      // A closing bracket the address did not open belongs to the sentence: "(at https://x.test/a)".
      if (part.split(open).length < part.split(last).length) end--;
      else break;
    } else break;
  }
  return address.slice(0, end);
}

/** The text split into its words and its web addresses, for a plain-text message. */
export function textParts(text: string): TextPart[] {
  const parts: TextPart[] = [];
  let at = 0;
  for (const match of text.matchAll(WEB_ADDRESS)) {
    const address = trimmed(match[0]);
    const start = match.index ?? 0;
    if (address.length <= "https://".length && !/^www\./i.test(address)) continue;
    if (start > at) parts.push({ text: text.slice(at, start) });
    parts.push({ text: address, url: /^www\./i.test(address) ? `https://${address}` : address });
    at = start + address.length;
  }
  if (at < text.length) parts.push({ text: text.slice(at) });
  return parts;
}
