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
