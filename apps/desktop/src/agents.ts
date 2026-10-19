import { invoke } from "@tauri-apps/api/core";

/**
 * What agents (Claude Code, Codex, Cursor and others, through the
 * `regards` CLI or MCP) may do with an account (ADR-0012). Off until its
 * owner allows them; only the app can change it, never an agent.
 */
export type AgentAccess = "off" | "read" | "triage" | "send";

export interface AgentAccessLevel {
  value: AgentAccess;
  label: string;
  /** What it allows, for a settings page. */
  allows: string;
}

/** The levels, each allowing what the one before it does, and more. */
export const AGENT_ACCESS_LEVELS: AgentAccessLevel[] = [
  { value: "off", label: "Off", allows: "Agents don't see this account." },
  { value: "read", label: "Read", allows: "List, search and read mail. Reading doesn't mark it read." },
  {
    value: "triage",
    label: "Triage",
    allows: "Read, and write drafts and organise: archive, move, flag, pin, snooze, senders.",
  },
  { value: "send", label: "Send", allows: "Triage, and send." },
];

export const setAgentAccess = (accountId: string, access: AgentAccess) =>
  invoke<{ agent_access: AgentAccess }>("account_set_agent_access", { accountId, access }).then(
    (result) => result.agent_access,
  );

/** What became of an agent's request. `held`, `approved` and `rejected` come with send approvals. */
export type AgentDecision = "allowed" | "denied" | "held" | "approved" | "rejected";

/** One request an agent made (ADR-0012 §4). */
export interface AgentActivity {
  id: number;
  /** Unix ms. */
  at: number;
  /** The agent's connection: requests made together share it. */
  session: string;
  /** What it came through: "cli" or "mcp". */
  client: string;
  /** The daemon method, such as "message.get" or "search.query". */
  method: string;
  /** Its parameters, secrets taken out; `{ cut: "…" }` when long. */
  args: unknown;
  /** The account it acted on, when it named one, and what that account allowed then. */
  account_id: string | null;
  access: AgentAccess | null;
  decision: AgentDecision;
  /** Set when it failed; `decision` is "denied" when it was not allowed. */
  error_code: number | null;
  error: string | null;
  duration_ms: number;
  /** For a change it made: how what it changed stood before (messages, a draft or a sender). */
  before: unknown;
  /**
   * What reverses that change, for `applyUndo` (accounts.ts), while
   * `undoable`: undo lasts ten minutes, and works once.
   */
  undo_id: number | null;
  undoable: boolean;
}

export interface AgentActivityPage {
  entries: AgentActivity[];
  /** Pass back as `before` for older entries. */
  next: number | null;
}

/**
 * What agents did, newest first: every request, allowed or refused, kept 90
 * days. Recording wakes no client, so refresh while the page is open.
 */
export const listAgentActivity = (
  options: { accountId?: string | null; before?: number | null; limit?: number | null } = {},
) =>
  invoke<AgentActivityPage>("agent_activity", {
    accountId: options.accountId ?? null,
    before: options.before ?? null,
    limit: options.limit ?? null,
  });

/** Why an agent's request waits for the person. */
export type HoldReason = "sending" | "bulk" | "delete";
export type HoldStatus = "pending" | "approved" | "rejected" | "expired" | "failed";

/** What an agent asked that waits (or waited) for the person (ADR-0012 §3). */
export interface Hold {
  id: number;
  /** Unix ms. Undecided by `expires_at` (a day later), it expires. */
  created_at: number;
  expires_at: number;
  session: string;
  client: string;
  account_id: string;
  /** The request as the agent made it: run as is once approved. */
  method: string;
  params: unknown;
  /** For the person: "Send “Plans” to Bob", "Archive 120 messages", "Delete 1 message for good". */
  description: string;
  reason: HoldReason;
  /** For a send: the draft (`getDraft`). Changed since it was held, it is not sent on approval. */
  draft_id: number | null;
  status: HoldStatus;
  decided_at: number | null;
  /** Once approved: what running it gave, or (status "failed") why it could not run. */
  result: unknown;
  error: string | null;
}

/** Holds, newest first; only those still waiting with `pending`. */
export const listHolds = (pending = true) =>
  invoke<{ holds: Hold[] }>("hold_list", { pending }).then((result) => result.holds);
/** It runs as the agent asked; the hold comes back with its result. */
export const approveHold = (id: number) => invoke<Hold>("hold_approve", { id });
export const rejectHold = (id: number) => invoke<Hold>("hold_reject", { id });
