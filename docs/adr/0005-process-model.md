# ADR-0005: Process model: one core daemon shared by all clients

- **Status:** Accepted. **Amended by [ADR-0017](0017-send-when-off-relay-in-v1.md):** background mode (start at login) is the default.
- **Date:** 2026-10-18
- **Deciders:** Project owner

## Context

Several clients must see the same mail, drafts and state: the desktop UI, the CLI/TUI and the MCP server for agents. Scheduled work (snooze, reminders, send later, undo-send) must run when no window is open.

- **Spark:** runs its core inside the desktop app (a utility process), so its CLI works only while the app is open.
- **Alice:** runs a separate daemon that every client shares. (01 §6 (private research notes), [04 §2](../research/04-alice-compatibility-assessment.md))

## Decision

1. **One daemon per OS user.** It is the **single owner** of the SQLite store, account sessions, sync engine, scheduler, operation queue and agent gateway. Clients never open the database directly.
2. **Binaries:**
   - `regards`: the CLI. It also contains the daemon, started with `regards daemon run`, and the MCP server, started with `regards mcp`.
   - The desktop app (Tauri) bundles the CLI binary as a **sidecar**, so a single install provides GUI, CLI and daemon.
3. **Lifecycle:**
   - Any client auto-starts the daemon if it isn't running.
   - The daemon takes an **exclusive lock** on the data directory, so a second daemon fails fast.
   - On connect, client and daemon exchange a **protocol version and build identity**. A mismatched daemon is asked to shut down and is replaced, so a client never talks to a stale daemon after an update.
   - The daemon keeps running when windows close. The user chooses between quitting with the app (the default) and **background mode**, which starts at login and is required for reliable scheduled work (ADR-0011).
   - `regards daemon status|stop` exist for diagnostics.
4. **The core is a library crate** (`core`) with no I/O transport dependencies. The daemon crate wraps it with the IPC server (ADR-0006). Tests run the core in-process.
5. **Crash isolation.** If the UI crashes, scheduled sends and sync continue. If the daemon crashes, the UI shows a reconnecting state and the next client call restarts the daemon.

## Alternatives considered

- **Core in-process inside the desktop app (Spark's model).** Simpler packaging, but the CLI and agents only work while the app is open, and scheduled work stops when the window closes. Rejected.
- **System-wide OS service.** Mail is per-user; the service would need elevation and per-user credential access. Rejected.
- **Each client opens SQLite directly.** Write contention, duplicate sync and no single place to enforce agent policy. Rejected.

## Consequences

- We own daemon lifecycle code: spawning without inheriting handles on Windows, stale-lock recovery and upgrade handover.
- Every feature is defined once, in the core, and exposed to all clients through one API. This is the basis for UI/CLI/MCP parity.
- There is a small IPC latency cost compared with in-process calls. It is mitigated by the change feed (ADR-0007) instead of polling.

## References

- [ADR-0006](0006-local-ipc-and-security.md), [ADR-0007](0007-state-model-intents-undo.md), [ADR-0011](0011-scheduled-work.md)
