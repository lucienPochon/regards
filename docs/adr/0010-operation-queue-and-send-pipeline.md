# ADR-0010: Offline-first operation queue and send pipeline

- **Status:** Accepted
- **Date:** 2026-10-18
- **Deciders:** Project owner

## Context

Users expect actions to feel instant and to work offline.

- **Spark:** keeps dirty-field flags, offline flags and per-account push-operation queues (01 §7 (private research notes)).
- **Alice:** requires connectivity for every action, but has an excellent **at-most-once** send guard ([04 §3](../research/04-alice-compatibility-assessment.md)).

Undo send and send later (ADR-0011) both need an outbox.

## Decision

1. **Operation queue.**
   - Every provider mutation produced by an intent (ADR-0007) is persisted in `operations`, with: an idempotency key, target identities (account, mailbox, UID/Message-ID or provider ID), the operation, attempts, `next_attempt_at` and the last error.
   - Local projected state is updated **optimistically** in the same transaction.
   - A runner per account executes operations in order per target, coalesces redundant ones (for example, read then unread becomes no-op), and retries with exponential backoff and jitter.
   - Permanent failures **revert** the optimistic state and surface an actionable error, with undo where meaningful.
   - **Implementation (Phase 1, flags first):**
     - **Actions:** `message.set_flags` (read/unread, flagged) applies the change locally and enqueues `add_flags`/`remove_flags` operations in one transaction. No-ops are skipped.
     - **Mailbox check:** each operation records the mailbox's UIDVALIDITY.
     - **Runner:** a per-account runner sends operations with `UID STORE … .SILENT`, oldest first.
       - **Connection trouble:** retries with backoff from 5 s to 10 min.
       - **Refusals:** a server refusal, or a UIDVALIDITY change, drops the operation and resyncs the mailbox, which is the revert. The reason shows in the account status.
     - **Sync:** sync re-applies pending operations after writing server flags (§2), and drops those from another UIDVALIDITY.
     - **Not yet:** coalescing of opposite operations.
2. **Reconciliation:**
   - Server state wins for fields with no pending local operation.
   - Pending local operations are re-applied on top of fresh server state.
   - Moves are matched by Message-ID when UIDs change.
   - A UIDVALIDITY reset triggers a mailbox resync that preserves local overlays (pin, snooze, done …) keyed by Message-ID.
3. **Outbox state machine:**

   `draft` → `queued(send_at, undo_until)` → `sending` → `sent` | `failed(retryable)` | `uncertain`

   - **Freezing.** When a message is queued, its exact **MIME bytes** are built, stored and hashed. From then on the draft is frozen, and editing returns it to `draft`, which cancels the queue entry.
   - **At-most-once.** Entering `sending` atomically claims the entry. A timeout or crash after the SMTP DATA phase (or after the API call) results in **`uncertain`, never an automatic retry**. The UI then asks the user, after checking the Sent folder for the Message-ID.
   - **Idempotent requests.** Repeated send requests for an already-sent outbox entry return the existing Sent message.
   - **Sent copies.** IMAP APPEND to the Sent mailbox happens only when the provider doesn't save its own copy (detected per provider and verified by sync), so copies are never duplicated.
4. **Undo send.**
   - Every send gets `undo_until = now + delay`: default 10 s, configurable 0–30 s.
   - Undo during the window moves the entry back to `draft` and reopens the composer.
   - Agent-initiated sends use the hold policy instead (ADR-0012).
5. **Visibility.** The Outbox view lists queued, scheduled, failed and uncertain items, each with clear actions.

## Alternatives considered

- **Online-only actions (Alice).** Feels sluggish and fails on flaky networks. Rejected.
- **Retrying sends blindly after a timeout.** Risks duplicate mail. Rejected.
- **Rebuilding MIME at send time.** Payloads can drift between preview and send. Rejected; MIME bytes are frozen instead.

## Consequences

- Fault-injection tests are mandatory: kill the daemon mid-send, drop the connection after DATA, and reset UIDVALIDITY with pending operations.
- Conflict rules are documented per operation type and tested with property tests.

## References

- [ADR-0007](0007-state-model-intents-undo.md), [ADR-0011](0011-scheduled-work.md), [ADR-0012](0012-agent-gateway.md)
