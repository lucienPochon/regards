/** The folders messages were last moved or copied to, by account, newest first; remembered on this computer. */
const KEY = "regards.recentFolders";
const KEEP = 3;

function load(): Record<string, number[]> {
  try {
    const saved = JSON.parse(window.localStorage.getItem(KEY) ?? "{}") as unknown;
    if (!saved || typeof saved !== "object" || Array.isArray(saved)) return {};
    const kept: Record<string, number[]> = {};
    for (const [account, ids] of Object.entries(saved)) {
      if (Array.isArray(ids)) kept[account] = ids.filter((id): id is number => Number.isInteger(id)).slice(0, KEEP);
    }
    return kept;
  } catch {
    return {};
  }
}

/** The account's recent folders, newest first. */
export function recentFolders(accountId: string): number[] {
  return load()[accountId] ?? [];
}

/** Messages were just moved or copied to this folder. */
export function rememberFolder(accountId: string, mailboxId: number): void {
  const all = load();
  all[accountId] = [mailboxId, ...(all[accountId] ?? []).filter((id) => id !== mailboxId)].slice(0, KEEP);
  try {
    window.localStorage.setItem(KEY, JSON.stringify(all));
  } catch {
    // Not remembered; the list of folders still has them all.
  }
}
