import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  type AccountSummary,
  type MailboxSummary,
  type MessageCursor,
  type MessageSummary,
  type Role,
  type SyncState,
  listAccounts,
  listMailboxes,
  listMessages,
} from "./accounts";
import { useChanges } from "./changes";
import { type Draft, type OutboxEntry, listDrafts, listOutbox } from "./compose";
import { type DaemonStatus, fetchDaemonStatus } from "./daemon";
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

/** A folder to read messages from, and the account it belongs to. */
export interface Source {
  mailboxId: number;
  accountId: string;
  role: Role | null;
}

/** The folders a view reads from: one, or the folder with that role in every account. */
export function sourcesFor(accounts: AccountMail[], view: View): Source[] {
  const sources: Source[] = [];
  for (const { account, mailboxes } of accounts) {
    for (const mailbox of mailboxes) {
      if (!mailbox.selectable) continue;
      const wanted =
        (view.kind === "role" && mailbox.role === view.role) ||
        (view.kind === "mailbox" && mailbox.id === view.mailboxId);
      if (wanted) sources.push({ mailboxId: mailbox.id, accountId: account.id, role: mailbox.role });
    }
  }
  return sources;
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

/** Newest first; the same order as the daemon's pages. */
function newer(a: { received_at: number; id: number }, b: { received_at: number; id: number }): number {
  return b.received_at - a.received_at || b.id - a.id;
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
  const key = sources.map((source) => source.mailboxId).join(",");
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
        return listMessages(source.mailboxId, null, Math.min(MAX_PAGE, Math.max(PAGE, shown))).then(
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
    listMessages(feed.source.mailboxId, feed.next)
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
          mailboxId: feed.source.mailboxId,
          accountId: feed.source.accountId,
          role: feed.source.role,
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
