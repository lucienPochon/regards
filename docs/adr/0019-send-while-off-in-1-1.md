# ADR-0019: 1.0 is fully local; sending while the computer is off (Send Service + Pro) ships in 1.1

- **Status:** **Accepted** (2026-10-18)
- **Deciders:** Project owner
- **Amends:**
  - [ADR-0018](0018-modern-minimal-v1.md): Send Service timing and shape
  - [ADR-0014](0014-business-model-open-source-freemium.md): Pro launches with 1.1
  - [ADR-0017](0017-send-when-off-relay-in-v1.md): FUTURERELEASE moves to 1.1

## Context

The project owner wants **the app to work completely first**, and to add sending while the computer is off afterwards, keeping the agreed design.

A review of an alternative proposal (a plain Rust HTTPS scheduler with PostgreSQL, about $5–15 per month) confirmed the Send Service's shape, but showed one gap. On an ordinary server, "decrypt only when due" is a rule in our code, not a guarantee, and **an IMAP/SMTP password usually opens the whole mailbox**, not only sending.

## Decision

### 1. Regards 1.0 runs no Regards servers

- **Accounts:** OAuth-first IMAP/SMTP as in ADR-0018: Microsoft OAuth, Google OAuth once approved, app passwords elsewhere.
- **Scheduling runs on the device:** send later, snooze, reminders and undo-send all run in the local daemon, with **background mode on by default** (ADR-0017 §1).
- **No internet:** the Outbox holds messages and sends them when the connection returns (ADR-0010).
- **Honest UI:** when a scheduled time arrives while the computer is off, the email goes out as soon as the computer is back on, under the catch-up rules of ADR-0011. The composer and the agent API say so: "your computer must be on at 09:00".
- **No Pro tier, accounts or billing in 1.0.** It is Free only.
- **Prepared for 1.1:**
  - the outbox `executor` field and the executor interface exist from 1.0, so 1.1 adds executors without schema or API redesign
  - the agent holds already apply to scheduled sends

### 2. Regards 1.1 adds sending while the computer is off, and launches Pro

**Free executors** (cost us nothing):
- SMTP FUTURERELEASE when the user's server offers it
- Exchange-held delayed delivery, if the spike confirms it
- the self-hostable Send Service in personal mode

**Pro executor:** our hosted **Regards Send Service**, with a 30-day trial. Its shape:

| Part | Implementation |
| --- | --- |
| API, queue, cancel/reschedule, status | A small **Rust HTTPS service** with an embedded **SQLite** job store. Move to PostgreSQL only if we ever run several instances |
| Sending with **send-only OAuth grants** (Microsoft `Mail.Send` / `SMTP.Send`, Google `gmail.send`) | A **normal isolated worker process**. Grants are encrypted at rest with a KMS-managed key. A breach could at worst send mail as users, never read it |
| Sending with **passwords** (Infomaniak, iCloud, hosting providers …) | A **tiny sealed worker** inside a hardware enclave. Devices verify its attestation (a reproducible build in a public transparency log) and encrypt the dedicated password to it. Only that worker can decrypt; it performs SMTP submission and deletes the job. Nobody at Regards can read passwords |
| Hosting | **One enclave-capable server**. The API and the normal worker run on the host; the sealed worker runs in its enclave. Prefer a Swiss or EU region (verify availability) |
| Accounts and billing | Passkey sign-in, Pro entitlement and trial; a merchant-of-record provider |

The device-side rules of ADR-0018 still apply: hand-off at scheduling time, at-most-once claim, agents' sends approved on the device before hand-off, per-send notifications, revocation.

### 3. Pre-work kept in the 1.0 timeline (no servers)

- Google verification for the **desktop client** starts in Phase 0.
- The `gmail.send`-only client for the Send Service is registered during 1.1.

## Alternatives considered

- **Send Service and Pro in 1.0 (ADR-0018 as accepted).** Delays 1.0 with server, security-review, legal and billing work before the core app is proven. Deferred to 1.1.
- **A plain server for every credential type (the reviewed proposal).** Simplest and cheapest, but our infrastructure could read users' mail passwords. Rejected for password jobs; adopted for send-only OAuth jobs.
- **Sealing the whole service (ADR-0018's wording).** More engineering than needed: only the password-handling step must be sealed. Refined.

## Consequences

- **1.0 is simpler and cheaper:** no servers, no accounts, no billing, no external security review of a hosted service, no data processing agreement for hosted data. The app costs only ([06 §1a](../research/06-costs-and-business-model.md)).
- **1.0 limit:** scheduled mail needs the computer on. This is stated clearly on the website and in the app.
- **1.1 is a focused release:** Send Service, Pro, trial and billing, provider-held delivery, self-hosting. Running cost is about **$20–40 per month** (*estimate*), plus the security review and legal documents ([06 §1b](../research/06-costs-and-business-model.md)).
- **Plan:** the Send Service phase moves after the 1.0 release ([03 §6](../research/03-independent-implementation-plan.md)).

## References

- [ADR-0010](0010-operation-queue-and-send-pipeline.md), [ADR-0011](0011-scheduled-work.md), [ADR-0012](0012-agent-gateway.md), [ADR-0018](0018-modern-minimal-v1.md)
- RFC 4865 (FUTURERELEASE); [AWS KMS condition keys for Nitro Enclaves](https://docs.aws.amazon.com/kms/latest/developerguide/conditions-nitro-enclave.html)
