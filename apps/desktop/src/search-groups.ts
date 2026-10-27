import { useCallback, useEffect, useRef, useState } from "react";
import type { MessageSummary } from "./accounts";
import { useChanges } from "./changes";
import { type SavedSearch, searchMessages } from "./search";

/** A saved search shown as one row of the Smart Inbox: what it finds in the inbox. */
export interface SearchGroup {
  saved: SavedSearch;
  /** Newest first: at least the first page, and as far back as the list below is loaded. */
  messages: MessageSummary[];
}

/** How many of a group's messages are fetched first. */
const GROUP_PAGE = 50;
/** The pages after it, while the list below goes further back. */
const MORE_PAGE = 500;
/** At most this many per group: beyond, older ones show in the list too. */
const GROUP_MOST = 5000;

/** The search a group runs: the saved one, in the inbox only (a later `in:` wins). */
export const groupQuery = (query: string) => `${query.trim()} in:inbox`;

/** The saved searches shown as groups (`ids`), in the sidebar's order; with `accountId`, those not of another account. */
export const groupsFor = (searches: SavedSearch[], ids: number[], accountId: string | null) =>
  searches.filter(
    (saved) => ids.includes(saved.id) && (accountId === null || saved.account_id === null || saved.account_id === accountId),
  );

/**
 * What a group finds: the first page, then more while the oldest found is
 * still as recent as `until` (the oldest the list has loaded), so the list
 * can leave out every one of them it shows.
 */
export async function groupMessages(saved: SavedSearch, accountId: string | null, until: number | null): Promise<MessageSummary[]> {
  const query = groupQuery(saved.query);
  const scope = { accountId: accountId ?? saved.account_id };
  let found = await searchMessages(query, { ...scope, limit: GROUP_PAGE });
  const messages = [...found.messages];
  while (
    found.next &&
    until !== null &&
    messages.length < GROUP_MOST &&
    (messages[messages.length - 1]?.received_at ?? 0) >= until
  ) {
    found = await searchMessages(query, { ...scope, before: found.next, limit: MORE_PAGE });
    messages.push(...found.messages);
  }
  return messages;
}

/** The messages of any group, by id: they show in their group and not in the list below. */
export const groupedIds = (groups: SearchGroup[]) => new Set(groups.flatMap((group) => group.messages.map((message) => message.id)));

/**
 * The saved searches the person shows as groups (`ids`, in the sidebar's
 * order), each with its newest inbox messages, back to `until` when given;
 * of one account when `accountId` is given, leaving out the searches of
 * other accounts. Kept current with the change feed.
 */
export function useSearchGroups(
  searches: SavedSearch[],
  ids: number[],
  accountId: string | null,
  enabled: boolean,
  until: number | null = null,
): SearchGroup[] {
  const [groups, setGroups] = useState<SearchGroup[]>([]);
  const wanted = groupsFor(searches, ids, accountId);
  const key = wanted.map((saved) => `${saved.id}:${saved.query}:${saved.account_id}:${saved.name}:${saved.color}`).join("|");
  const current = useRef(wanted);
  current.current = wanted;
  const generation = useRef(0);
  const refresh = useCallback(() => {
    // Kept while another view shows, so coming back does not flash their messages in the list.
    if (!enabled) return;
    const run = ++generation.current;
    if (!key) {
      setGroups([]);
      return;
    }
    Promise.all(
      current.current.map((saved) =>
        groupMessages(saved, accountId, until).then(
          (messages): SearchGroup => ({ saved, messages }),
          (): SearchGroup => ({ saved, messages: [] }),
        ),
      ),
    ).then((found) => run === generation.current && setGroups(found));
  }, [enabled, key, accountId, until]);
  useEffect(refresh, [refresh]);
  useChanges(refresh);
  return groups;
}
