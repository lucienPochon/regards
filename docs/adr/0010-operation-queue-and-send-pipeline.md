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
       - **Back online:** when the account's push connection signs in again, everything waiting for a retry is sent at once, not after its backoff.
       - **Tested offline:** a fault-injection proxy drops and refuses connections. Actions taken while offline show at once and reach the server within seconds of the line coming back.
       - **Not yet:** a connection that dies silently (no reset, as on some network changes) is noticed only when IDLE is renewed. OS network-change events would help there.
       - **Refusals:** a server refusal, or a UIDVALIDITY change, drops the operation and resyncs the mailbox, which is the revert. The reason shows in the account status.
     - **Sync:** sync re-applies pending operations after writing server flags (§2), and drops those from another UIDVALIDITY.
     - **Not yet:** coalescing of opposite operations.
   - **Moves (archive, trash, move to):**
     - **Instant:** a moving message keeps its row but records `moving_to`, so lists show it in the target at once.
     - **Runner:** uses `UID MOVE`, or COPY + `\Deleted` + `UID EXPUNGE` with UIDPLUS. It refuses servers with neither, never running a plain EXPUNGE.
     - **New UIDs:** with COPYUID, the row moves in place and keeps its id. Without it, the row is removed and the next sync of the target brings it back.
     - **Re-reading:** the runner re-reads each operation before sending it, so an undo that withdrew it is honoured.
     - **Giving up:** a dropped move shows the messages where they still are on the server.
     - **Opening a moving message:** if the server moved it before we recorded its new UID, the download is retried once the move is recorded.
   - **Deleting for good (`message.delete`):**
     - **Where:** only messages shown in Trash or Junk, and not while one is still being moved there. The daemon enforces this, not just the UI.
     - **No undo:** the app asks for confirmation inline.
     - **Locally:** the message is marked `\Deleted`. Lists and conversations leave out `\Deleted` messages, including ones another client marked.
     - **Runner:** `STORE \Deleted` then `UID EXPUNGE` of exactly those UIDs, and the rows go once the server confirms.
       - **Without UIDPLUS:** it refuses rather than run a plain `EXPUNGE`, and gives up like any refusal, so the message reappears with the reason.
     - **After undo:** an undo entry naming a message being deleted skips it.
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
   - **Implementation (Phase 2):**
     - **Freezing:** `draft.send` builds the MIME (`mail-builder`) and stores it with its SHA-256 in `outbox` (migration 0009).
       - **Message-ID:** our own random ID on the sender's domain.
       - **Date:** UTC.
       - **Bcc:** only in the envelope.
       - **While queued:** the draft cannot be edited or deleted.
     - **Guarded writes:** every state change is one guarded write.
       - **Undo** deletes the entry only while it is `queued` (or `failed`).
       - **Claim:** the runner claims it only while it is `queued` and past `undo_until`, so exactly one of them wins.
     - **SMTP outcomes** (our `regards-smtp` client, ADR-0009):
       - **Sent:** `sent`.
       - **Not sent, retryable:** back to `queued`, with backoff from 5 s to 10 min.
       - **Not sent, permanent:** `failed`, with the reason.
       - **Connection lost after the final `.`:** `uncertain`.
       - **Crash:** a `sending` entry found at startup becomes `uncertain`.
       - **Uncertain:** never retried by itself. The user chooses "send again" (`outbox.send_again`) or "it went" (`outbox.dismiss`). Checking the Sent folder for the Message-ID is still to do.
     - **After sending:**
       - **Draft:** it is deleted.
       - **Answered:** the message it answered is marked `\Answered` through the operation queue.
       - **Sent copy:** appended to Sent, marked read, unless the IMAP host is Gmail's or Microsoft's. That folder is then synced, so the reply appears in its conversation at once. A failed copy is retried up to 10 times.
4. **Undo send.**
   - Every send gets `undo_until = now + delay`: default 10 s, configurable 0–30 s.
   - Undo during the window moves the entry back to `draft` and reopens the composer.
   - Agent-initiated sends use the hold policy instead (ADR-0012).
   - **Implementation (Phase 2):** `outbox.undo` takes the entry back and returns its draft. It also works on a `failed` entry, to edit and resend it.
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
