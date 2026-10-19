import { invoke } from "@tauri-apps/api/core";
import type { MessageCursor, MessageSummary } from "./accounts";

/**
 * Search (plan §5.10), Gmail-style:
 *
 * - **Words** match the subject, the people and the text; the last one also
 *   as the start of a word, so results come while typing.
 * - `"a phrase"` matches as written; `-word` leaves out.
 * - `from:` `to:` `subject:` look in one place (`from:"Lena Müller"`).
 * - `has:attachment`, `is:unread` `is:read` `is:flagged`.
 * - `in:<folder>` by name or role (`in:sent`); Trash and Junk are left out
 *   unless named, or with `in:anywhere`.
 * - `before:` / `after:` `YYYY-MM-DD`.
 *
 * Accents need not be typed: "zurich" finds "Zürich". A message's whole text
 * is searched once it was opened on this device, its start before.
 */
export interface SearchResult {
  /** One copy of each message (its inbox copy, if it has one), newest first. */
  messages: MessageSummary[];
  /**
   * On the first page, when the words match few messages: the best matches,
   * best first. They are among `messages` too; worth showing on their own
   * when there are more results than fit at a glance.
   */
  top: MessageSummary[];
  /** Pass back as `before` for older results. */
  next: MessageCursor | null;
  /** Changes still to index: while above 0, recent mail may be missing (say so, search again later). */
  indexing: number;
}

export interface SearchOptions {
  /** Only this account; every account when left out. */
  accountId?: string | null;
  before?: MessageCursor | null;
  /** 1 to 500; 50 when left out. */
  limit?: number | null;
}

export const searchMessages = (query: string, options: SearchOptions = {}) =>
  invoke<SearchResult>("search_messages", {
    query,
    accountId: options.accountId ?? null,
    before: options.before ?? null,
    limit: options.limit ?? null,
  });

/**
 * One term of a search, for an action that starts one (such as "Mail from
 * this sender"): `from:lena@example.net`, `from:"Lena Müller"`.
 */
export function searchTerm(field: "from" | "to" | "subject" | "in", value: string): string {
  const clean = value.replace(/"/g, "").trim();
  return /\s/.test(clean) ? `${field}:"${clean}"` : `${field}:${clean}`;
}
