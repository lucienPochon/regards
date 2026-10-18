# ADR-0014: Business model: open-source freemium, paid hosted services only

- **Status:** **Accepted** (2026-10-18). The Free and Pro tiers are accepted; the Pro price is a hypothesis to validate with early users. **Sequencing amended by [ADR-0019](0019-send-while-off-in-1-1.md):** 1.0 is Free only, and Pro (starting with the Send Service) launches with **1.1**.
- **Date:** 2026-10-18
- **Deciders:** Project owner

## Context

The project will remain open source. The [cost analysis](../research/06-costs-and-business-model.md) shows two kinds of cost:

- **A local-only client** has modest **fixed** costs (code signing, test accounts, domain, trademark, OAuth verification effort) and near-zero cost per extra user.
- **Hosted services** create **per-user and compliance** costs:
  - servers for "send when the computer is off" and cross-device sync
  - hosted AI tokens
  - Google's annual CASA security assessment once Gmail data passes through our servers
  - GDPR / Swiss revFADP obligations

In an open-source client, any feature that is "paid" only because of client-side code can be unlocked by anyone who forks it. A paywall on local code is therefore both ineffective and against the spirit of the project.

## Decision (proposed)

1. **Rule: charge only for what costs us money to run.** The client, core, CLI and MCP server are identical for free and paid users: no DRM, no feature flags tied to payment in local code. Paid value lives in **hosted services**, which are open source (AGPL, ADR-0004) and **self-hostable**. People pay for convenience, reliability and support.
2. **Tiers:**

   | Tier | Price (hypothesis) | Includes |
   | --- | --- | --- |
   | **Free (local)** | €0 | The complete desktop client on Windows, macOS and Linux: unlimited accounts; Smart Inbox, Done/Pin/Set aside; snooze, reminders and send later **while the computer is on** (background mode); send-while-off **when the mail provider itself holds the message** (SMTP FUTURERELEASE, or Exchange-held delivery if verified), because that costs us nothing; undo and undo-send; local keyword and semantic search; templates and signatures; BYOK AI; agent CLI/MCP with access levels, holds and audit; **self-hosting the Send Service** (open source) |
   | **Pro** | about €3–5 / month or €30–48 / year; **30-day free trial** | **Send while your computer is off, with any email provider**, through our hosted, sealed Regards Send Service (ADR-0018): any IMAP/SMTP host with a dedicated password, plus Gmail and Microsoft through send-only keys. Also used by agent-scheduled emails. Later in 1.x: **encrypted cross-device sync** of app state (settings, sender rules, snoozes, pins, templates, signatures; not mail), **hosted AI** with a monthly quota (no API key needed), priority support |
   | **Teams** (later) | per seat | Shared inboxes, assignment, comments, shared drafts. Requires the collaboration server; separate future ADR |

3. **Sequencing (revised 2026-10-18):**
   - **1.0 launches Free + Pro.** Pro's first and only 1.0 feature is the hosted Send Service, because the owner wants send-while-off in 1.0 for any provider.
   - Sync, hosted AI and priority support join Pro during 1.x.
   - The free trial lets every user try send-while-off. The per-user cost is tiny (a few KB per job and one SMTP or API call), so a trial costs us almost nothing.
4. **Minimal Regards account (Pro only):**
   - Sign-in with **passkeys** (WebAuthn), with no passwords for our own accounts, plus an email address for receipts.
   - Devices are paired to the account; the Send Service checks the Pro entitlement before accepting jobs.
   - The free client never requires an account.
5. **Complementary income:** GitHub Sponsors or Open Collective donations, paid support or onboarding for organisations, and later grants for open-source sustainability.
6. **Billing:**
   - Use a **merchant-of-record** payment provider (it handles VAT/GST worldwide, which matters when selling from Switzerland to EU and other customers), or Stripe with tax handling.
   - Chosen in Phase 3, since Pro now ships with 1.0.
7. **Hosting region:** prefer a Swiss or EU region for the Send Service (for example AWS Zurich, `eu-central-2`). Enclave availability per region and instance type must be verified.
8. **Ethics:**
   - No ads.
   - No selling or mining of data.
   - No tracking pixels or read receipts.
   - Pricing pages state exactly what data each paid service processes.

## Alternatives considered

- **Paywalling local features (open-core client).** Forks remove the paywall; it creates community friction and contradicts "remain open source". Rejected.
- **Donations only.** Doesn't cover hosted-service costs or sustained development. Insufficient alone, so kept as a complement.
- **Fully paid app with open source code.** Possible, but weak adoption, and anyone can build it themselves. Rejected.
- **Ads or data monetisation.** Incompatible with an email client's trust model. Rejected.

## Consequences

- **1.0 needs three server pieces** (all AGPL, all small):
  - the sealed Send Service (ADR-0018)
  - the account service with passkeys
  - billing integration

  Plan Phase 3 grows accordingly.
- **What the UI says:** free users scheduling while the computer may be off see "Your computer must be on at 09:00", with the options: keep Regards running, **try Pro**, or self-host the Send Service. No feature in the local app is locked.
- **Self-hosting stays free** for anyone running the Send Service themselves, in personal mode on their own hardware. Pro pays for our sealed, maintained, monitored instance.
- Pricing is a hypothesis to validate with early users; see the break-even notes in [06](../research/06-costs-and-business-model.md).

## References

- [06 Costs & business model](../research/06-costs-and-business-model.md); [ADR-0004](0004-licensing-and-trademark.md), [ADR-0011](0011-scheduled-work.md), [ADR-0015](0015-privacy-telemetry-and-ai.md)
