import { invoke } from "@tauri-apps/api/core";

/**
 * What agents (Claude Code, Codex, Cursor and others, through the
 * `regards` CLI or MCP) may do with an account (ADR-0012). Off until its
 * owner allows them; only the app can change it, never an agent.
 */
export type AgentAccess = "off" | "read" | "triage" | "send";

/** Mirrors `AgentsSettings`: the switch for every agent at once. */
export interface AgentsSettings {
  /**
   * On by default. Off, the CLI and MCP get nothing ("agents are turned off
   * in Regards"), whatever each account allows; the accounts keep their levels.
   */
  enabled: boolean;
  /** Agents connected now. */
  connected: number;
  /** The agent apps connected now, as they name themselves (`claude-code`, say). */
  apps: string[];
}

export const agentsSettings = () => invoke<AgentsSettings>("agents_settings");

/** An agent app the Agents menu knows, and how a request is handed to it. */
export interface AgentApp {
  id: "claude-code" | "claude-app" | "codex" | "cursor";
  label: string;
  /** Opened with a request ready (`openInAgent`), or only copied to. */
  opens: boolean;
  /** What it does with a request handed to it. */
  hint: string;
  /** Whether a connected app's own name is this one. */
  is: (name: string) => boolean;
}

export const AGENT_APPS: AgentApp[] = [
  {
    id: "claude-code",
    label: "Claude Code",
    opens: true,
    hint: "Opens in a new terminal with the request ready: press Enter there.",
    is: (name) => /claude/i.test(name) && /code/i.test(name),
  },
  {
    id: "claude-app",
    label: "Claude",
    opens: true,
    hint: "Opens a new chat in the Claude app with the request ready: send it there.",
    is: (name) => /claude/i.test(name) && !/code/i.test(name),
  },
  { id: "codex", label: "Codex", opens: false, hint: "", is: (name) => /codex/i.test(name) },
  { id: "cursor", label: "Cursor", opens: false, hint: "", is: (name) => /cursor/i.test(name) },
];

/** Whether an agent app is connected now. */
export const isConnected = (app: AgentApp, settings: AgentsSettings | null) =>
  Boolean(settings?.apps?.some((name) => app.is(name)));

/** Open Claude Code or the Claude app with a request ready to send (nothing is sent until the person does). */
export const openInAgent = (app: "claude-code" | "claude-app", prompt: string) =>
  invoke<void>("open_in_agent", { app, prompt });

/** Off also ends the connections of the agents connected now. */
export const setAgentsEnabled = (enabled: boolean) => invoke<AgentsSettings>("agents_set_enabled", { enabled });

/** End every agent's connection now; they may connect again. Returns how many there were. */
export const disconnectAgents = () => invoke<number>("agents_disconnect");

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

/** The undo window an agent's sends may go with, in seconds (ADR-0012 §3). */
export const AGENT_SEND_WINDOW = { min: 10, max: 600 } as const;

/**
 * How an agent's sends go from an account (at Send access): each waits for
 * approval (`null`, the default), or goes after `windowSeconds` (10 to 600),
 * during which the person is told and can call it back from the Outbox.
 * At most 20 an hour go so; more wait for approval.
 */
export const setAgentSend = (accountId: string, windowSeconds: number | null) =>
  invoke<{ agent_send_window: number | null }>("account_set_agent_send", { accountId, windowSeconds }).then(
    (result) => result.agent_send_window,
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

/** Why an agent's request waits for the person: `voice` is a suggested voice, `note` a note with a link, an address or bank details (ADR-0020). */
export type HoldReason = "sending" | "bulk" | "delete" | "voice" | "note";
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
/** Approve a hold; `text`: a suggested voice (`reason: "voice"`) as the person edited it. */
export const approveHold = (id: number, text?: string) =>
  invoke<Hold>("hold_approve", text === undefined ? { id } : { id, text });
export const rejectHold = (id: number) => invoke<Hold>("hold_reject", { id });
