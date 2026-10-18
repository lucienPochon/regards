# ADR-0011: Scheduled work: snooze, reminders, send later

- **Status:** Accepted. **Item 6 is superseded:**
  - background mode is on by default ([ADR-0017](0017-send-when-off-relay-in-v1.md) §1)
  - sending while the computer is off uses the sealed Send Service in 1.0 ([ADR-0018](0018-modern-minimal-v1.md)), hosted as a Pro feature ([ADR-0014](0014-business-model-open-source-freemium.md)) and self-hostable for free
- **Date:** 2026-10-18
- **Deciders:** Project owner

## Context

Snooze, follow-up reminders, send later and undo-send all need actions to happen at a future time.

- **Spark:** runs Send Later on its servers ("will be sent even if Spark is closed"), and syncs snoozes across devices through its backend (02 §4, §6 (private research notes)).
- **Our constraints:** we have no backend in v1, and any server that holds users' mail credentials brings security and compliance costs, including Google's CASA assessment ([06](../research/06-costs-and-business-model.md)).

## Decision

1. **Local durable scheduler (v1).**
   - A `jobs` table with: kind (`snooze_wake`, `reminder_check`, `send_at`, `undo_expire`), `due_at` (UTC), the original time zone and wall-clock intent, payload, state and attempts.
   - One scheduler task in the daemon sleeps until the next due job. It re-arms on any change, and on wake from system sleep or a clock change.
2. **Catch-up rules when the daemon starts after downtime:**
   - Overdue **snoozes and reminders** fire immediately, with a summary notification.
   - Overdue **scheduled sends** are sent automatically if they are less than 1 hour late. Otherwise they are **held for confirmation** in the Outbox, because a "9:00 Monday" email sent at 15:00 may no longer be appropriate. The threshold is configurable.
3. **Snooze semantics:**
   - Presets plus a custom time and "Someday".
   - Wake-up returns the thread to the Inbox as new, with an optional notification.
   - **A new inbound message in the thread cancels the snooze.**
   - Storage is a local overlay by default. An optional per-account **server-visible mode** moves the thread to a `Snoozed` mailbox (a label on Gmail) and back, so other clients can see it.
4. **Reminders:** "remind me if no reply by …". Cancelled by any reply from another participant, detected through threading (ADR-0009).
5. **Background mode** (ADR-0005) is offered whenever the user schedules something while it is off. The UI states plainly that **scheduled items run only while the computer is on and the app or daemon is running.**
6. **Later, optional: a relay service** for "send even when my computer is off" and cross-device snooze sync. It is a separate future ADR. It will be:
   - open source (AGPL, ADR-0004) and self-hostable
   - offered as a paid hosted service (ADR-0014)
   - designed so the relay stores only **end-to-end-encrypted payloads** where possible, and the narrowest credentials possible (for example a send-only grant)

   It must not hold full mailbox access unless unavoidable, because that triggers the compliance costs in [06](../research/06-costs-and-business-model.md).
7. **Provider-native deferral** is investigated per provider as an optimisation. For example, Exchange/Microsoft 365 deferred delivery is to be verified; to our knowledge, the Gmail API documents no scheduled-send endpoint. It is never assumed.

## Alternatives considered

- **Server-side scheduling from day one (Spark model).** Best UX, but requires holding credentials server-side, triggers CASA and GDPR obligations, and adds running costs before we have revenue. Deferred to the optional relay.
- **Self-sent bump emails.** Clutters mailboxes. Rejected.
- **OS-level scheduled tasks per job.** Fragile and platform-specific. Rejected.

## Consequences

- Clear UX copy and settings for background mode on all three OSes (ADR-0013).
- Test suite: DST transitions, time-zone changes, clock jumps, sleep/resume, and long downtime.
- The relay becomes the first natural paid feature (ADR-0014).

## References

- [Spark Send Later docs](https://sparkmailapp.com/help/sending-emails/schedule-an-email-to-send-later); [ADR-0010](0010-operation-queue-and-send-pipeline.md)
