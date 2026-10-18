# ADR-0007: State model: core-owned state, intents, change feed, undo

- **Status:** Accepted
- **Date:** 2026-10-18
- **Deciders:** Project owner

## Context

The "Spark feel" comes from a few properties (01 §5.5 (private research notes), 02 §7 (private research notes)):

- instant UI reactions
- actions that are always correctly enabled or disabled
- undo for nearly everything
- identical behaviour from hotkeys, swipes, menus and agents

Spark achieves this with a core that owns state, validates actions and records undo information. Alice, by contrast, polls the whole state every 2 seconds and has no undo.

## Decision

1. **The core is the single source of truth.** Clients hold only view state (selection, scroll position, open panels).
2. **Intents.** Every mutation is an `Intent` (a typed Rust enum: archive, done, pin, snooze, send, change-category, accept-sender, …) carrying a target selection and the **actor** (`user(source: hotkey|menu|toolbar|swipe)` or `agent(id)`). The pipeline is:
   - `validate(intent, selection, actor) -> Validation { allowed, reason?, confirm? }`. Clients call it to enable or disable UI and to decide on confirmation dialogs. The same function gates agents.
   - `apply`: an atomic local transaction that updates projected state, enqueues provider operations (ADR-0010), and writes an **inverse** to `undo_log`.
   - `undo(undo_id)`: itself an intent. It is valid while the inverse still applies, for example while the thread has not changed since.
3. **Change feed.**
   - Every committed transaction increments a global `revision`.
   - Clients **subscribe to views**: a list with its parameters, a thread, counters, the outbox, holds.
   - The daemon pushes typed **deltas** (insert, update, remove, move, count change) tagged with the revision.
   - Clients can resume from a revision after reconnecting, or request a snapshot.
   - **No polling.**
   - **Implementation (Phase 1, first step):** `changes.subscribe` dedicates a connection to `changes` notifications.
     - **Payload:** `{version, revision}`.
     - **Version:** moves on every committed write and every sync-status change. Bursts coalesce into one notification.
     - **Client behaviour:** clients refetch what they show.
     - **Desktop:** re-emits the notifications to the WebView as `changes` events, and reconnects with backoff.
     - **Later:** typed per-view deltas build on this channel.
4. **List engine.**
   - Sections and lists (Smart Inbox sections, folders, Outbox, Snoozed, Set aside, search results) are **materialised memberships** with counts.
   - They are maintained incrementally on each transaction, so opening a list or rendering counts never scans all messages.
5. **Generated contracts.** Intents, views and deltas are defined once in Rust; TypeScript types and the JSON schema for CLI/MCP are **generated** (tool chosen in the Phase-0 spike, for example `specta` or `ts-rs`).
6. **Undo UX.** A toast with Undo for at least 10 seconds after a user action; the undo history is visible in a panel; agent actions are undoable from the audit log (ADR-0012).

## Alternatives considered

- **Request/response with polling (Alice).** Simple, but O(n) work every poll, laggy, and no live counts. Rejected.
- **CRDT-replicated state in every client.** Overkill for single-user local state; cross-device sync is handled later and separately (ADR-0011/0014). Rejected for v1.
- **Undo implemented per UI component.** Inconsistent, and unavailable to agents. Rejected.

## Consequences

- Up-front design work on intent and inverse definitions. Property tests must show that `apply` followed by `undo` restores the exact prior state.
- The materialised-list maintenance logic is the core's most complex area and needs a 100k-message benchmark fixture from Phase 1.
- CLI, MCP and UI parity becomes structural rather than a matter of discipline.

## References

- [03 Implementation plan §3.1](../research/03-independent-implementation-plan.md)
