import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  type AccountSummary,
  type MailboxSummary,
  type MessageCursor,
  type MessageListResult,
  type MessageSummary,
  type MessageView,
  type Role,
  type SyncState,
  listAccounts,
  listMailboxes,
  listMessages,
  listView,
} from "./accounts";
import { useChanges } from "./changes";
import { type Draft, type OutboxEntry, listDrafts, listOutbox } from "./compose";
import { type DaemonStatus, fetchDaemonStatus } from "./daemon";
import { searchMessages } from "./search";
import { type Section, type SectionCount, smartOverview } from "./smart";
import type { View } from "./views";

export interface AccountMail {
  account: AccountSummary;
  mailboxes: MailboxSummary[];
  sync: SyncState | null;
  /** Why its folders could not be read, if they could not. */
  error: string | null;
}

export type MailState =
  | { state: "loading" }
  | { state: "ready"; accounts: AccountMail[] }
  | { state: "failed"; message: string };

/** Every account with its folders and sync state, kept current by the change feed. */
export function useMail(): MailState {
  const [mail, setMail] = useState<MailState>({ state: "loading" });
  const generation = useRef(0);

  const refresh = useCallback(() => {
    const run = ++generation.current;
    listAccounts()
      .then((accounts) =>
        Promise.all(
          accounts.map((account) =>
            listMailboxes(account.id).then(
              (result): AccountMail => ({ account, mailboxes: result.mailboxes, sync: result.sync, error: null }),
              (error: unknown): AccountMail => ({ account, mailboxes: [], sync: null, error: String(error) }),
            ),
          ),
        ),
      )
      .then((accounts) => run === generation.current && setMail({ state: "ready", accounts }))
      .catch((error: unknown) => run === generation.current && setMail({ state: "failed", message: String(error) }));
  }, []);

  useEffect(refresh, [refresh]);
  useChanges(refresh);
  return mail;
}

/** Where a list reads messages from (a folder, or an account's pinned ones, say), a page at a time. */
export interface Source {
  /** Tells sources apart, within a view and between views. */
  key: string;
  accountId: string;
  /** The folder, when the source is one; otherwise each message says where it is. */
  mailboxId: number | null;
  role: Role | null;
  /** Without a role of its own: the role of the folder each message is in. */
  roleOf?: (mailboxId: number) => Role | null;
  fetch: (before: MessageCursor | null, limit: number | null) => Promise<MessageListResult>;
}

/**
 * What a view reads from: one folder, the folder with that role in every
 * account, or every account's pinned, snoozed or set-aside messages.
 */
export function sourcesFor(accounts: AccountMail[], view: View): Source[] {
  if (view.kind === "section") {
    const { inbox } = view;
    return sectionSources(inbox ? accounts.filter((entry) => entry.account.id === inbox.accountId) : accounts, view.section);
  }
  const sources: Source[] = [];
  for (const { account, mailboxes } of accounts) {
    if (view.kind === "marked") {
      const mark = view.mark;
      sources.push({
        key: `marked:${mark}:${account.id}`,
        accountId: account.id,
        mailboxId: null,
        role: null,
        fetch: (before, limit) => listView(account.id, mark, before, limit),
      });
      continue;
    }
    for (const mailbox of mailboxes) {
      if (!mailbox.selectable) continue;
      const wanted =
        (view.kind === "role" && mailbox.role === view.role) ||
        (view.kind === "mailbox" && mailbox.id === view.mailboxId);
      if (wanted) {
        sources.push({
          key: `mailbox:${mailbox.id}`,
          accountId: account.id,
          mailboxId: mailbox.id,
          role: mailbox.role,
          fetch: (before, limit) => listMessages(mailbox.id, before, limit),
        });
      }
    }
  }
  return sources;
}

/**
 * One Smart Inbox section of every account's inbox; with `seen`, only its
 * mail read (true) or unread (false), as the card view shows it.
 */
export function sectionSources(accounts: AccountMail[], section: Section, seen: boolean | null = null): Source[] {
  return accounts.flatMap(({ account, mailboxes }) =>
    mailboxes
      .filter((mailbox) => mailbox.role === "inbox" && mailbox.selectable)
      .map(
        (mailbox): Source => ({
          key: `section:${section}:${seen ?? "all"}:${mailbox.id}`,
          accountId: account.id,
          mailboxId: mailbox.id,
          role: "inbox",
          fetch: (before, limit) => listMessages(mailbox.id, before, limit, section, seen),
        }),
      ),
  );
}

/** Every account's inbox, only its mail read (true) or unread (false): the card view's last card. */
export function inboxSources(accounts: AccountMail[], seen: boolean): Source[] {
  return accounts.flatMap(({ account, mailboxes }) =>
    mailboxes
      .filter((mailbox) => mailbox.role === "inbox" && mailbox.selectable)
      .map(
        (mailbox): Source => ({
          key: `inbox:${seen}:${mailbox.id}`,
          accountId: account.id,
          mailboxId: mailbox.id,
          role: "inbox",
          fetch: (before, limit) => listMessages(mailbox.id, before, limit, null, seen),
        }),
      ),
  );
}

/**
 * The newest few of some sources, merged: enough for a card or a group row
 * to show who and how many. Kept current.
 */
export function useFirstPage(sources: Source[], limit: number, enabled: boolean): ListItem[] {
  const [items, setItems] = useState<ListItem[]>([]);
  const key = enabled ? sources.map((source) => source.key).join(",") : "";
  const wanted = useRef(sources);
  wanted.current = sources;
  const generation = useRef(0);
  const refresh = useCallback(() => {
    const run = ++generation.current;
    if (!key) {
      setItems([]);
      return;
    }
    Promise.all(
      wanted.current.map((source) =>
        source.fetch(null, limit).then(
          (page) =>
            page.messages.map(
              (message): ListItem => ({
                ...message,
                mailboxId: source.mailboxId ?? message.mailbox_id,
                accountId: source.accountId,
                role: source.role,
              }),
            ),
          () => [] as ListItem[],
        ),
      ),
    ).then((pages) => run === generation.current && setItems(pages.flat().sort(newer).slice(0, limit)));
  }, [key, limit]);
  useEffect(refresh, [refresh]);
  useChanges(refresh);
  return items;
}

/** Every inbox's Smart Inbox sections, counted together: section → total and unread. */
export function useSmartOverview(accounts: AccountMail[], enabled: boolean): Partial<Record<Section, SectionCount>> {
  const [counts, setCounts] = useState<Partial<Record<Section, SectionCount>>>({});
  const ids = enabled ? accounts.map((entry) => entry.account.id).join(",") : "";
  const generation = useRef(0);
  const refresh = useCallback(() => {
    const run = ++generation.current;
    if (!ids) {
      setCounts({});
      return;
    }
    Promise.all(ids.split(",").map((id) => smartOverview(id).catch(() => [] as SectionCount[]))).then((all) => {
      if (run !== generation.current) return;
      const sum: Partial<Record<Section, SectionCount>> = {};
      for (const count of all.flat()) {
        const now = sum[count.section] ?? { section: count.section, total: 0, unread: 0 };
        sum[count.section] = { ...now, total: now.total + count.total, unread: now.unread + count.unread };
      }
      setCounts(sum);
    });
  }, [ids]);
  useEffect(refresh, [refresh]);
  useChanges(refresh);
  return counts;
}

export interface Search {
  /** One source per account searched, for `useFeed`; null without words to search. */
  sources: Source[] | null;
  /** The best few matches, when there are few enough to rank. */
  top: ListItem[];
  /** Changes not indexed yet: recent mail may be missing while above 0. */
  indexing: number;
  /** When nothing was found: a search, typos mended, that finds something ("did you mean"). */
  suggestion: string | null;
}

/**
 * A search of every account, or of one, as sources for `useFeed` (so the
 * results page, merge and stay current like a folder), with the best matches
 * and what is still being indexed from each first page.
 */
export function useSearch(accounts: AccountMail[], query: string, accountId: string | null, round: number): Search {
  const key = `${round}:${accountId ?? ""}:${query}`;
  const [firsts, setFirsts] = useState<{
    key: string;
    by: Record<string, { top: ListItem[]; indexing: number; suggestion: string | null }>;
  }>({
    key: "",
    by: {},
  });
  const sources = useMemo(() => {
    if (!query) return null;
    return accounts
      .filter((entry) => accountId === null || entry.account.id === accountId)
      .map(({ account, mailboxes }): Source => {
        const roles = new Map(mailboxes.map((mailbox) => [mailbox.id, mailbox.role]));
        const roleOf = (id: number) => roles.get(id) ?? null;
        return {
          key: `search:${account.id}:${key}`,
          accountId: account.id,
          mailboxId: null,
          role: null,
          roleOf,
          fetch: (before, limit) =>
            searchMessages(query, { accountId: account.id, before, limit }).then((result) => {
              if (!before) {
                const top = result.top.map(
                  (message): ListItem => ({ ...message, mailboxId: message.mailbox_id, accountId: account.id, role: roleOf(message.mailbox_id) }),
                );
                setFirsts((now) => ({
                  key,
                  by: {
                    ...(now.key === key ? now.by : {}),
                    [account.id]: { top, indexing: result.indexing, suggestion: result.suggestion ?? null },
                  },
                }));
              }
              return { messages: result.messages, next: result.next };
            }),
        };
      });
  }, [accounts, query, accountId, key]);
  const found = firsts.key === key ? Object.values(firsts.by) : [];
  // The accounts' best, taken in turns, three in all.
  const top: ListItem[] = [];
  for (let index = 0; top.length < 3 && found.some((one) => one.top.length > index); index++) {
    for (const one of found) if (one.top[index] && top.length < 3) top.push(one.top[index]);
  }
  return {
    sources,
    top,
    indexing: found.reduce((sum, one) => sum + one.indexing, 0),
    // Searching several accounts, the first that has one.
    suggestion: found.find((one) => one.suggestion)?.suggestion ?? null,
  };
}

/** A message in a list, with the folder it was listed from. */
export interface ListItem extends MessageSummary {
  mailboxId: number;
  accountId: string;
  role: Role | null;
}

interface Feed {
  source: Source;
  messages: MessageSummary[];
  next: MessageCursor | null;
}

const PAGE = 50;
const MAX_PAGE = 500;

/** Newest first, by place in the list; the same order as the daemon's pages. */
export function newer(a: { listed_at: number; id: number }, b: { listed_at: number; id: number }): number {
  return b.listed_at - a.listed_at || b.id - a.id;
}

/**
 * Which feed holds the list back: of the feeds with more to fetch, the one
 * whose oldest fetched message is newest. Messages older than that one may
 * still have neighbours to come from it, so they wait.
 */
function bottleneck(feeds: Feed[]): number | null {
  let found: number | null = null;
  feeds.forEach((feed, index) => {
    const last = feed.messages[feed.messages.length - 1];
    if (!feed.next || !last) return;
    const current = found === null ? null : feeds[found].messages[feeds[found].messages.length - 1];
    if (!current || newer(last, current) < 0) found = index;
  });
  return found;
}

export interface MessageFeed {
  items: ListItem[];
  /** Nothing fetched yet for these folders. */
  loading: boolean;
  error: string | null;
  more: boolean;
  loadMore: () => void;
}

/**
 * The messages of one or more folders as one list, newest first. Each folder
 * is paged on its own; a message shows once no folder can still have a newer
 * one to fetch, so the merged order never changes under the reader's eyes.
 */
export function useFeed(sources: Source[]): MessageFeed {
  const key = sources.map((source) => source.key).join(",");
  const [state, setState] = useState<{ key: string; feeds: Feed[] } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const latest = useRef(state);
  latest.current = state;
  const wanted = useRef(sources);
  wanted.current = sources;
  const generation = useRef(0);
  const fetching = useRef(false);

  // Re-read the newest of each folder, as many as are already fetched, so scrolled-in pages stay.
  const refresh = useCallback(() => {
    const run = ++generation.current;
    const current = latest.current?.key === key ? latest.current.feeds : null;
    Promise.all(
      wanted.current.map((source, index) => {
        const shown = current?.[index]?.messages.length ?? 0;
        return source.fetch(null, Math.min(MAX_PAGE, Math.max(PAGE, shown))).then(
          (page): Feed => ({ source, messages: page.messages, next: page.next }),
        );
      }),
    )
      .then((feeds) => {
        if (run !== generation.current) return;
        setState({ key, feeds });
        setError(null);
      })
      .catch((failure: unknown) => run === generation.current && setError(String(failure)));
  }, [key]);

  useEffect(() => {
    setState(null);
    setError(null);
    refresh();
  }, [refresh]);
  useChanges(refresh);

  const loadMore = useCallback(() => {
    const current = latest.current;
    if (!current || current.key !== key || fetching.current) return;
    const index = bottleneck(current.feeds);
    if (index === null) return;
    const feed = current.feeds[index];
    const run = generation.current;
    fetching.current = true;
    feed.source.fetch(feed.next, null)
      .then((page) => {
        if (run !== generation.current) return;
        setState((now) =>
          now && now.key === key
            ? {
                key,
                feeds: now.feeds.map((f, i) =>
                  i === index ? { ...f, messages: [...f.messages, ...page.messages], next: page.next } : f,
                ),
              }
            : now,
        );
      })
      .catch((failure: unknown) => setError(String(failure)))
      .finally(() => {
        fetching.current = false;
      });
  }, [key]);

  const merged = useMemo(() => {
    if (!state || state.key !== key) return null;
    const held = bottleneck(state.feeds);
    const limit = held === null ? null : state.feeds[held].messages[state.feeds[held].messages.length - 1];
    const items: ListItem[] = [];
    for (const feed of state.feeds) {
      for (const message of feed.messages) {
        if (limit && newer(message, limit) > 0) continue;
        items.push({
          ...message,
          mailboxId: feed.source.mailboxId ?? message.mailbox_id,
          accountId: feed.source.accountId,
          role: feed.source.role ?? feed.source.roleOf?.(message.mailbox_id) ?? null,
        });
      }
    }
    items.sort(newer);
    return { items, more: held !== null };
  }, [state, key]);

  return {
    items: merged?.items ?? [],
    loading: merged === null && error === null,
    error,
    more: merged?.more ?? false,
    loadMore,
  };
}

/**
 * Every account's pinned or set-aside messages, newest first: the first page
 * of each, enough for the inbox's group row and bubble to show who and how many.
 */
export function useMarked(accounts: AccountMail[], mark: MessageView, enabled: boolean): MessageSummary[] {
  const [messages, setMessages] = useState<MessageSummary[]>([]);
  const ids = accounts.map((entry) => entry.account.id).join(",");
  const generation = useRef(0);
  const refresh = useCallback(() => {
    const run = ++generation.current;
    if (!enabled || !ids) {
      setMessages([]);
      return;
    }
    Promise.all(
      ids.split(",").map((id) =>
        listView(id, mark, null, MARKED_PAGE).then(
          (page) => page.messages,
          () => [] as MessageSummary[],
        ),
      ),
    ).then((pages) => run === generation.current && setMessages(pages.flat().sort(newer)));
  }, [ids, mark, enabled]);
  useEffect(refresh, [refresh]);
  useChanges(refresh);
  return messages;
}

const MARKED_PAGE = 100;

/** Messages being written, newest change first. */
export function useDrafts(): Draft[] {
  const [drafts, setDrafts] = useState<Draft[]>([]);
  const refresh = useCallback(() => {
    listDrafts()
      .then((result) => setDrafts(result.drafts))
      .catch(() => {});
  }, []);
  useEffect(refresh, [refresh]);
  useChanges(refresh);
  return drafts;
}

/** What has not gone out yet. */
export function useOutbox(): OutboxEntry[] {
  const [entries, setEntries] = useState<OutboxEntry[]>([]);
  const refresh = useCallback(() => {
    listOutbox()
      .then((result) => setEntries(result.entries))
      .catch(() => {});
  }, []);
  useEffect(refresh, [refresh]);
  useChanges(refresh);
  return entries;
}

export type ServiceCheck =
  | { state: "checking" }
  | { state: "done"; status: DaemonStatus }
  | { state: "failed"; message: string };

/** The background service's status, checked at start, on focus and on every change. */
export function useService(): { check: ServiceCheck; recheck: () => void } {
  const [check, setCheck] = useState<ServiceCheck>({ state: "checking" });

  // Without "Checking…" in between, so change events do not make the status flicker.
  const update = useCallback(() => {
    fetchDaemonStatus()
      .then((status) => setCheck({ state: "done", status }))
      .catch((error: unknown) => setCheck({ state: "failed", message: String(error) }));
  }, []);
  const recheck = useCallback(() => {
    setCheck({ state: "checking" });
    update();
  }, [update]);

  useEffect(() => {
    update();
    window.addEventListener("focus", update);
    return () => window.removeEventListener("focus", update);
  }, [update]);
  useChanges(update);
  return { check, recheck };
}
