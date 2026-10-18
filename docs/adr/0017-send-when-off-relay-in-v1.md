# ADR-0017: Send later works when Regards is closed or the computer is off, in version 1

- **Status:** **Partially superseded by [ADR-0018](0018-modern-minimal-v1.md)** (2026-10-18).
  - **Still in force:** §1 (background mode on by default, in 1.0), §2 (SMTP FUTURERELEASE, moved to **1.1** by [ADR-0019](0019-send-while-off-in-1-1.md)) and §5 (agents' sends are approved on the device before hand-off).
  - **Replaced:** §3 (self-hosted relay in 1.0) and §4 (hosted-relay tracks), by the sealed Send Service.
- **Date:** 2026-10-18
- **Deciders:** Project owner
- **Amends:**
  - [ADR-0005](0005-process-model.md): background mode becomes the default
  - [ADR-0011](0011-scheduled-work.md): its item 6 ("later, optional relay") is superseded; the relay ships in 1.0
  - [ADR-0014](0014-business-model-open-source-freemium.md): hosted relay timing
  - [ADR-0016](0016-v1-scope-imap-only.md): 1.0 scope

## Context

The project owner wants scheduled emails to go out **even when Regards is not running**. This is central to the agent story: an agent prepares and schedules mail, and it must leave on time even if the laptop is asleep.

"Regards is off" covers three different situations:

| Situation | What can send the email |
| --- | --- |
| A. Window closed, computer on | The local Regards daemon, if it keeps running in the background |
| B. Computer asleep, off or offline | Only **another always-on machine**, or the mail provider itself |
| C. The user's mail server supports delayed release | The provider, via SMTP **FUTURERELEASE** (RFC 4865). Rarely offered, but free when it is |

Constraints:

- **IMAP has no standard scheduled send**, so situation B needs a relay that holds the message until the send time.
- **The relay must hold a credential.** In 1.0 (ADR-0016) the only credential is the user's **mail password or app password**, which usually grants **full mailbox access**, not just sending. A relay therefore stores very sensitive secrets.
- **How Spark does it:** Spark sends scheduled mail from its own servers. Readdle stores account access on those servers: OAuth tokens, or the login and password itself for providers without OAuth, encrypted on AWS. The servers also poll mailboxes for push notifications. This is consistent with the private investigation (private research notes). We want the feature without that level of credential custody.
- **Snooze and reminders don't need a relay.** They change what the user sees, and nobody looks at the inbox while the computer is off. They fire when the device wakes (ADR-0011). Only **sending** needs to happen while the device is off.

## Decision

1. **Background mode is on by default** (situation A).
   - The Regards daemon starts at login and keeps running when windows close, with a tray or menu-bar indicator and a clear setting to turn it off.
   - First-run onboarding explains this.
2. **SMTP FUTURERELEASE is used automatically** when the user's submission server advertises it (situation C). The message is handed to the provider with a release time, and no relay is involved.
3. **Regards Relay ships in 1.0** (situation B). It is a small, headless, open-source service (AGPL-3.0-only, ADR-0004) built from the same Rust crates as Regards.
   - **Distribution:** a single binary and a Docker image. It runs on anything always on: a home server, NAS, Raspberry Pi, or a cheap VPS.
   - **Pairing:** a device pairs with a relay once, with a QR code or code exchange and per-device keys. Traffic is TLS with mutual authentication.
   - **Hand-off:** when the user (or an agent, after its hold is approved, ADR-0012) schedules a send, the device:
     - hands off the **frozen MIME bytes** (ADR-0010), envelope, `send_at` and SMTP endpoint
     - attaches a **per-job copy of the SMTP credential**, encrypted to the relay's key
     - stores the job as `relayed(relay_id, job_id)` in the outbox
   - **Least privilege:** the relay only performs **SMTP submission**. It never logs into IMAP. The device saves the Sent copy and reconciles threading on its next sync (or the provider saves Sent copies itself, as Gmail does).
   - **Credential lifetime:** the credential copy and the payload are **deleted immediately after sending**, or on cancel. No long-lived credential store, no mailbox access, no message content kept after delivery.
   - **Control:** devices can cancel and reschedule until a send claim is taken (at-most-once semantics, ADR-0010). The relay reports status (`queued`, `sent`, `failed`, `uncertain`) back to the devices.
   - **When the device sleeps:** if the device is asleep at `send_at` and a relay is paired, the relay sends. If no relay is paired, the catch-up rules of ADR-0011 apply.
   - **Where it is used:** the UI makes the choice visible and changeable per message: "Sends from: this computer / your relay / your mail server".
4. **Hosted relay ("Regards Cloud Relay").** Same code, run by us. **Owner to choose:**
   - **Track 3 (recommended): send-only keys only. Our servers never store full-access mail passwords.**
     - 1.0 ships background mode + FUTURERELEASE + the **self-hosted** relay.
     - The **hosted** relay opens with the OAuth releases. It holds **send-only OAuth grants**: Microsoft `Mail.Send` / `SMTP.Send` from 1.1, and Google `gmail.send` from 1.3. Google classifies `gmail.send` as *sensitive*, not *restricted*, per the Nylas and EmailEngine guides; one source disagrees, so confirm with Google. That would mean brand verification but no CASA.
     - For Microsoft, first check whether Exchange Online honours server-side deferred delivery (`PidTagDeferredSendTime` via Graph extended properties). If it does, Microsoft users need no relay at all.
     - Password-only accounts (iCloud, Yahoo, generic IMAP) keep using the self-hosted relay.
     - The hosted relay runs on managed infrastructure (a managed scheduler + serverless functions + a cloud KMS/HSM). Grants are decrypted only inside the send function.
   - **Track 1:** as Track 3, but the hosted relay also accepts **passwords** from 1.x, after a security review and penetration test.
   - **Track 2:** the hosted relay is in 1.0 and stores passwords (Spark's model). This adds, before launch: a penetration test, a lawyer-reviewed privacy policy, terms and data processing agreement, an incident-response plan, monitoring and on-call.

   Costs are in [06 §1b](../research/06-costs-and-business-model.md).
5. **Agents** (ADR-0012):
   - Scheduling still requires **Send** access, and holds are completed on the device **before** hand-off, so a relay never receives an unapproved message.
   - Relayed jobs appear in the audit log with their relay status.
   - Agents can query and cancel relayed jobs through the same `regards` CLI/MCP commands.

## Alternatives considered

- **Local daemon only (the previous ADR-0011 plan).** Fails whenever the laptop sleeps, which is exactly the case the owner cares about. Rejected for 1.0.
- **Hosted relay only, no self-hosted option.** Forces every user to trust us with their mail password, and makes 1.0 depend on running secure infrastructure. Rejected as the *only* option.
- **Relay with IMAP access** (to save Sent copies and detect replies). Broader privileges for little gain, since the device can do this later. Rejected.
- **Encrypting the credential so the relay cannot read it.** Impossible: the relay must use the credential to log in at send time. Mitigated instead by per-job copies, immediate deletion and send-only behaviour.
- **Third-party sending APIs** (Resend, Postmark, SendGrid, Mailgun scheduled sending). They send from their own infrastructure, so mail from personal addresses (`@gmail.com`, `@icloud.com` …) fails SPF/DKIM/DMARC alignment and is not filed in the user's Sent folder. They only work for users who control their own domain's DNS. Rejected as a general mechanism.
- **Email-access aggregators** (for example Nylas-style hosted mailbox APIs). They would give a third company full access to our users' mailboxes, priced per connected account. Rejected on privacy grounds.

## Consequences

- **Scope:** 1.0 gains a second deliverable (the relay, with pairing, a job API and Docker packaging). Size: **M**. It reuses the MIME, outbox and SMTP code; the plan places it in Phase 3.
- **Security:** the relay is a sensitive component even when self-hosted. Its security review moves into the 1.0 release checklist.
- **Credential wording:** users with an app password should be told it grants full access. Where a provider offers **send-only or scoped app passwords**, setup recommends one for the relay.
- **Provider risk:** some providers may challenge SMTP logins from unfamiliar or data-centre IPs. Each major provider must be tested with relay-originated sends before 1.0 (Gmail app passwords in particular).
- **Cost:** Track 1 adds **$0** for us; Track 2 adds the hosted-service costs ([06 §1b](../research/06-costs-and-business-model.md)).
- **Later (OAuth releases):** a relay can hold a **send-only** OAuth grant (Microsoft `SMTP.Send`), which is much safer than a password. With Google OAuth, the hosted relay would trigger CASA, but the self-hosted relay would not, since the user runs it.

## References

- RFC 4865 (SMTP FUTURERELEASE); [ADR-0010](0010-operation-queue-and-send-pipeline.md), [ADR-0011](0011-scheduled-work.md), [ADR-0012](0012-agent-gateway.md)
