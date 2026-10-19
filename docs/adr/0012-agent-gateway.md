# ADR-0012: Agent gateway: CLI + MCP with access levels, holds, audit, undo

- **Status:** Accepted
- **Date:** 2026-10-18
- **Deciders:** Project owner

## Context

AI agents (Claude Code, Codex, Cursor and others) are first-class users of our client.

- **Spark:** enforces Off/Read/Triage/Send per account and per shared inbox **in its core**, can hold sensitive agent actions for approval or undo, keeps an audit log with before-state snapshots, and exposes a CLI, an MCP stdio server and a skill file (01 §6.1 (private research notes), 02 §11 (private research notes)).
- **Alice:** has a single global level, checked only inside its MCP process, and no audit or holds ([04 §3](../research/04-alice-compatibility-assessment.md)).

## Decision

1. **Surfaces:**
   - `regards` CLI with human output by default and `--json`: stable snake_case fields, ISO 8601 dates, and an additive-only schema within a major version.
   - `regards mcp`, an MCP server over stdio. Its tools map 1:1 to CLI commands and core intents.
   - `regards skill [--install <dir>]` prints or installs agent instructions.
   - All three use the `agent` principal over the IPC transport (ADR-0006).
   - Implementation choice: the official Rust MCP SDK versus a minimal in-house JSON-RPC implementation, decided in a Phase-5 spike, subject to licence and maintenance checks.
     - **Decided (2026-10-19): in-house.** MCP over stdio is newline-delimited JSON-RPC 2.0, and Regards needs four methods (`initialize`, `ping`, `tools/list`, `tools/call`). The daemon already speaks JSON-RPC, so an SDK would be a much larger dependency than the code it saves. The tools come from the same list as the CLI's commands (`regards-cli/src/tools.rs`). This can be revisited if Regards needs more of the protocol (resources, prompts, sampling).
2. **Access levels**, set **per account** (and per shared mailbox later), enforced **in the daemon** for every intent, with the default **Off**:

   | Level | Allows |
   | --- | --- |
   | Off | Nothing; the account is invisible to agents |
   | Read | List, search and read mail, contacts and invitations. Reading never marks mail as read unless asked |
   | Triage | Read + drafts + organising intents (archive, done, pin, snooze, label, category, accept/block sender) |
   | Send | Triage + send, schedule and unschedule, calendar replies (iTIP) |

   Only the `ui` principal can change levels. Errors name the setting to change.
3. **Holds.** Policy per level and intent class:
   - **Send-tier intents** default to *require approval*. Configurable: *execute with an N-second undo window*.
   - **Bulk or destructive intents** (more than X targets, permanent delete) always require approval.
   - A held intent returns `pending(hold_id)` to the agent.
   - The desktop shows an approval popup with Approve / Reject / Run now / Undo, and `regards holds list|approve|reject` exists for headless use.
   - Holds expire after a configurable time.
4. **Audit log.** Every agent intent is recorded with timestamp, agent session ID, command and arguments, targets, access level, decision (allowed / denied / held / approved / rejected), result, duration and a **before-state snapshot**.
   - It is viewable and filterable in Settings → Agents.
   - Undo is available while the inverse is valid (ADR-0007).
   - Retention is configurable, defaulting to 90 days.
5. **Safety posture:**
   - Email content is returned to agents as data, clearly delimited.
   - **Drafts never auto-send**; sending is a separate intent.
   - No permanent delete without approval.
   - Agents cannot read credentials or change settings.
   - Per-session rate limits.
   - Responses include **deep links** (`regards://draft/<id>`) so the user can review in the app.
6. **Deleted drafts and unscheduled messages:** the semantics are explicit and documented for agents. For example, unscheduling returns a draft with a new ID, and deleting a draft is permanent.

## Alternatives considered

- **Client-side enforcement (Alice).** Trivially bypassed by any client that talks to the daemon. Rejected.
- **One global level.** Too coarse; users want a personal account at Read and a work account at Send. Rejected.
- **No holds, only undo.** Undo cannot recall an email that has already been delivered. Rejected for the send tier.

## Consequences

- Holds need UI surfaces: a popup window and a tray badge.
- Red-team tests with a prompt-injection corpus (emails containing instructions) must show that no intent fires unless the agent explicitly calls a tool, and that send-tier actions are always held.
- The agent API is a public contract: it is versioned and covered by contract tests.

## References

- [MCP specification](https://modelcontextprotocol.io); [Spark CLI access levels](https://sparkmailapp.com/help/spark-cli/set-up-spark-cli-with-your-ai-agents)
