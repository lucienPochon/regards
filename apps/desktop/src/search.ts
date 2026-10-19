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

export interface ServerSearchResult {
  /** Found by the servers and on this device: one copy of each (its inbox copy first), newest first. */
  messages: MessageSummary[];
  /** Pass back as `before` for older results; each page asks the servers again. */
  next: MessageCursor | null;
  /**
   * Found by the servers but not on this device yet: older than what has been
   * fetched so far (older mail keeps being fetched in the background), or
   * just arrived (the next sync brings it). They then show in searches here.
   */
  not_here: number;
  /** Ids of the accounts whose server could not be searched now (offline, say). */
  unreachable: string[];
}

/**
 * The same search on the servers: they have the text of mail not opened on
 * this device, which the search here does not (it has their subject, people
 * and preview). Slower, as it asks each server: for a "Search on the server"
 * action, not while typing. Servers match words their own way, often as
 * parts of words.
 */
export const searchServer = (query: string, options: SearchOptions = {}) =>
  invoke<ServerSearchResult>("search_server", {
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
