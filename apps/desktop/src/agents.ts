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
