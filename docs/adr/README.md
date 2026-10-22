# Architecture Decision Records

Each ADR records one significant decision: its context, the decision itself, the alternatives considered and the consequences. ADRs are immutable once accepted. To change a decision, write a new ADR that **supersedes** the old one and update the old one's status.

**Status values:** `Proposed` (awaiting the project owner's confirmation) · `Accepted` · `Superseded by ADR-NNNN` · `Rejected`.

## Index

| # | Title | Status |
| --- | --- | --- |
| [0001](0001-clean-room-policy.md) | Clean-room policy: no Spark or Alice code | Accepted |
| [0002](0002-technology-stack.md) | Technology stack: Rust core, Tauri 2 shell, React + TypeScript UI | Accepted |
| [0003](0003-build-from-scratch.md) | Build from scratch; Alice is a design reference only | Accepted |
| [0004](0004-licensing-and-trademark.md) | Licensing (app `Apache-2.0 OR MIT`, servers `AGPL-3.0-only`), contributions (DCO) and trademark (optional) | Accepted |
| [0005](0005-process-model.md) | Process model: one core daemon shared by all clients | Accepted |
| [0006](0006-local-ipc-and-security.md) | Local IPC transport and security boundary | Accepted |
| [0007](0007-state-model-intents-undo.md) | State model: core-owned state, intents, change feed, undo | Accepted |
| [0008](0008-storage-and-search.md) | Storage and search: SQLite with migrations, FTS5 first | Accepted |
| [0009](0009-mail-protocol-stack.md) | Mail protocol stack and provider abstraction | Accepted (IMAP: imap-next + imap-codec, per spike 07; order amended by 0018) |
| [0010](0010-operation-queue-and-send-pipeline.md) | Offline-first operation queue and send pipeline | Accepted |
| [0011](0011-scheduled-work.md) | Scheduled work: snooze, reminders, send later | Accepted (item 6 superseded by 0017/0018) |
| [0012](0012-agent-gateway.md) | Agent gateway: CLI + MCP with access levels, holds, audit, undo | Accepted |
| [0013](0013-cross-platform-support.md) | Windows, macOS and Linux are all tier-1 from day one | Accepted |
| [0014](0014-business-model-open-source-freemium.md) | Business model: Free + Pro; Pro = hosted services only (starting with the Send Service in 1.1) | Accepted (price to validate; sequencing amended by 0019) |
| [0015](0015-privacy-telemetry-and-ai.md) | Privacy, telemetry and AI data handling | Accepted |
| [0016](0016-v1-scope-imap-only.md) | Version 1: IMAP/SMTP with passwords only; OAuth providers later | Superseded by 0018 |
| [0017](0017-send-when-off-relay-in-v1.md) | Send later works when Regards is closed or the computer is off | Partially superseded by 0018 (§1, §2, §5 still in force) |
| [0018](0018-modern-minimal-v1.md) | Modern, minimal 1.0: OAuth-first IMAP and a sealed Send Service for any email host | Accepted (timing and shape amended by 0019) |
| [0019](0019-send-while-off-in-1-1.md) | 1.0 is fully local; sending while the computer is off (Send Service + Pro) ships in 1.1 | **Accepted** |
| [0020](0020-voice-and-notes.md) | A voice per sending address and notes per person, for agents | Accepted |
| [0021](0021-signatures.md) | Several signatures, one default per address | Accepted |

**What Regards is, in one paragraph:**
- **Regards 1.0** is a fully local desktop email client for Windows, macOS and Linux, with no Regards servers. It is built on a Rust core, Tauri 2 and React.
- It syncs any mailbox over IMAP/SMTP: OAuth for Microsoft and Google (Google once approved), app passwords elsewhere.
- It provides Smart Inbox, Done/Pin/Set aside, snooze, reminders, send later while the computer is on, an offline Outbox and undo everywhere, plus an agent CLI/MCP with access levels, holds and an audit log.
- **Regards 1.1** adds sending on time **while the computer is off**:
  - free when the mail provider can hold the message, or with a self-hosted Send Service
  - with **Pro**, through our Send Service, which works with any email host. Send-only keys for Google and Microsoft; a sealed worker for passwords, so nobody at Regards can read them.

**Product name:** **Regards** (decided 2026-10-18). Its CLI binary, pipe, socket and URL scheme are `regards`. The trademark search is still to do (ADR-0004).

Background research for these decisions is in [`docs/research/`](../research/README.md). Costs are in [06-costs-and-business-model.md](../research/06-costs-and-business-model.md).

## Template

```markdown
# ADR-NNNN: Title

- **Status:** Proposed | Accepted | Superseded by ADR-NNNN
- **Date:** YYYY-MM-DD
- **Deciders:** …

## Context
Why a decision is needed; constraints; evidence (link research).

## Decision
What we will do, stated precisely.

## Alternatives considered
Each option with its main pros and cons and why it was not chosen.

## Consequences
Positive, negative, and follow-up work this creates.

## References
Links to research, standards, docs.
```
