import { invoke } from "@tauri-apps/api/core";
import type { MessageCursor, MessageSummary } from "./accounts";

/**
 * Search (plan §5.10), Gmail-style:
 *
 * - **Words** match the subject, the people and the text; the last one also
 *   as the start of a word, so results come while typing.
 * - `"a phrase"` matches as written; `-word` leaves out.
 * - `from:` `to:` `subject:` look in one place (`from:"Lena Müller"`);
 *   `cc:` only among those copied.
 * - `has:attachment`, `is:unread` `is:read` `is:flagged` `is:pinned`.
 * - `in:<folder>` by name or role (`in:sent`); Trash and Junk are left out
 *   unless named, or with `in:anywhere`.
 * - `before:` / `after:` `YYYY-MM-DD`; and Spark's "today", "yesterday",
 *   "this week", "last week", "this month", "last month", "last 3 days"
 *   (weeks, months), in English or French, in this computer's days.
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
  /**
   * When nothing was found: the search with its unknown words replaced by
   * the nearest known ones, which does find something. Offer it ("Did you
   * mean …?"); searching it is the person's choice.
   */
  suggestion?: string;
}

export interface SearchOptions {
  /** Only this account; every account when left out. */
  accountId?: string | null;
  before?: MessageCursor | null;
  /** 1 to 500; 50 when left out. */
  limit?: number | null;
}

/** This computer's time minus UTC, in minutes, for relative dates. */
const localOffset = () => -new Date().getTimezoneOffset();

export const searchMessages = (query: string, options: SearchOptions = {}) =>
  invoke<SearchResult>("search_messages", {
    query,
    accountId: options.accountId ?? null,
    before: options.before ?? null,
    limit: options.limit ?? null,
    utcOffsetMinutes: localOffset(),
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
    utcOffsetMinutes: localOffset(),
  });

/**
 * One term of a search, for an action that starts one (such as "Mail from
 * this sender"): `from:lena@example.net`, `from:"Lena Müller"`.
 */
export function searchTerm(field: "from" | "to" | "subject" | "in", value: string): string {
  const clean = value.replace(/"/g, "").trim();
  return /\s/.test(clean) ? `${field}:"${clean}"` : `${field}:${clean}`;
}

/**
 * A saved search (Spark's Smart Folder): kept by name, and run again with
 * `searchMessages(query, { accountId })` whenever it is opened from the
 * sidebar. Kept on this device. The mail is never moved or copied.
 */
export interface SavedSearch {
  id: number;
  name: string;
  query: string;
  /** Only this account; `null`: every account. */
  account_id: string | null;
  /** `#rrggbb`, or none. */
  color: string | null;
}

/** The saved searches, in the sidebar's order. */
export const listSavedSearches = () =>
  invoke<{ searches: SavedSearch[] }>("saved_search_list").then((result) => result.searches);

/**
 * Keep a search by name (no `id`: last in the order), or change one: its
 * name, what it looks for, its account or colour. Throws if the name is
 * empty, there is nothing to search for, or the colour is not `#rrggbb`.
 */
export const saveSearch = (search: {
  id?: number | null;
  name: string;
  query: string;
  accountId?: string | null;
  color?: string | null;
}) =>
  invoke<SavedSearch>("saved_search_save", {
    search: {
      id: search.id ?? null,
      name: search.name,
      query: search.query,
      account_id: search.accountId ?? null,
      color: search.color ?? null,
    },
  });

/** Put the saved searches in this order (dragged in the sidebar); the new list. */
export const reorderSavedSearches = (ids: number[]) =>
  invoke<{ searches: SavedSearch[] }>("saved_search_reorder", { ids }).then((result) => result.searches);

/** Forget a saved search; its mail stays where it is. */
export const removeSavedSearch = (id: number) => invoke<void>("saved_search_remove", { id });

/** A filter offered as a chip under the search field: what it adds to the search. */
export interface SearchChip {
  label: string;
  term: string;
}

/** The chips, in order: each adds its words to the search, or takes them out. */
export const SEARCH_CHIPS: SearchChip[] = [
  { label: "Unread", term: "is:unread" },
  { label: "Flagged", term: "is:flagged" },
  { label: "Pinned", term: "is:pinned" },
  { label: "With files", term: "has:attachment" },
  { label: "Sent", term: "in:sent" },
  { label: "Last 7 days", term: "last 7 days" },
];

/** `term` as whole words of `query`, in any case. */
const termPattern = (term: string) => {
  const words = term.split(" ").map((word) => word.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
  return new RegExp(`(^|\\s)${words.join("\\s+")}(?=\\s|$)`, "i");
};

/** Whether the search has `term` among its words. */
export const hasTerm = (query: string, term: string) => termPattern(term).test(query);

/** The search with `term` taken out if it is there, or added at the end. */
export function toggleTerm(query: string, term: string): string {
  if (hasTerm(query, term)) return query.replace(termPattern(term), "$1").replace(/\s{2,}/g, " ").trim();
  const kept = query.trimEnd();
  return kept === "" ? term : `${kept} ${term}`;
}
