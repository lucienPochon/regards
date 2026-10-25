import { useCallback, useEffect, useRef, useState } from "react";
import type { MessageSummary } from "./accounts";
import { useChanges } from "./changes";
import { type SavedSearch, searchMessages } from "./search";

/** A saved search shown as one row of the Smart Inbox: what it finds in the inbox. */
export interface SearchGroup {
  saved: SavedSearch;
  messages: MessageSummary[];
}

/** How many of a group's messages are fetched; the row counts up to it. */
const GROUP_PAGE = 50;

/** The search a group runs: the saved one, in the inbox only (a later `in:` wins). */
export const groupQuery = (query: string) => `${query.trim()} in:inbox`;

/** The saved searches shown as groups (`ids`), in the sidebar's order; with `accountId`, those not of another account. */
export const groupsFor = (searches: SavedSearch[], ids: number[], accountId: string | null) =>
  searches.filter(
    (saved) => ids.includes(saved.id) && (accountId === null || saved.account_id === null || saved.account_id === accountId),
  );

/**
 * The saved searches the person shows as groups (`ids`, in the sidebar's
 * order), each with its newest inbox messages; of one account when
 * `accountId` is given, leaving out the searches of other accounts. Kept
 * current with the change feed.
 */
export function useSearchGroups(searches: SavedSearch[], ids: number[], accountId: string | null, enabled: boolean): SearchGroup[] {
  const [groups, setGroups] = useState<SearchGroup[]>([]);
  const wanted = groupsFor(searches, ids, accountId);
  const key = enabled ? wanted.map((saved) => `${saved.id}:${saved.query}:${saved.account_id}:${saved.name}:${saved.color}`).join("|") : "";
  const current = useRef(wanted);
  current.current = wanted;
  const generation = useRef(0);
  const refresh = useCallback(() => {
    const run = ++generation.current;
    if (!key) {
      setGroups([]);
      return;
    }
    Promise.all(
      current.current.map((saved) =>
        searchMessages(groupQuery(saved.query), { accountId: accountId ?? saved.account_id, limit: GROUP_PAGE }).then(
          (found): SearchGroup => ({ saved, messages: found.messages }),
          (): SearchGroup => ({ saved, messages: [] }),
        ),
      ),
    ).then((found) => run === generation.current && setGroups(found));
  }, [key, accountId]);
  useEffect(refresh, [refresh]);
  useChanges(refresh);
  return groups;
}
