# Costs and business model

**Date:** 2026-10-18
**Prices:** verified on that date from the sources listed at the end. Prices are in USD unless marked CHF or EUR. Items marked *estimate* are our assumptions, not quotes. Re-verify every number before committing budget.
**Related decisions:** [ADR-0004 (licensing)](../adr/0004-licensing-and-trademark.md), [ADR-0014 (business model, accepted)](../adr/0014-business-model-open-source-freemium.md), [ADR-0015 (privacy and AI)](../adr/0015-privacy-telemetry-and-ai.md), [ADR-0018 (1.0 scope and Send Service, accepted)](../adr/0018-modern-minimal-v1.md)

---

## 1. Summary

**Yes, there are costs, but most of them only appear once we run servers.**

| Scenario | Recurring cost (approx.) | Scales with users? |
| --- | --- | --- |
| **Regards 1.0 app** (free for everyone) | **$0 required.** About $10–110 per year recommended (domain, Apple signing); everything else optional (§1a) | No: each extra user costs us nothing |
| **Pro Send Service** (launches with **1.1**, ADR-0019) | **about $5,500–17,500 in its first year**, then about $3,300–11,000 per year (§1b) | Barely: almost all of it is fixed (security review, legal, an always-on host) |
| Later Pro additions (sync, hosted AI) | Small hosting plus per-user AI tokens (§4) | Yes, through AI usage |

The dominant real cost in both scenarios is **developer time**. One full-time developer costs far more than every line above combined.

This supports the model in [ADR-0014](../adr/0014-business-model-open-source-freemium.md):

- **Free tier:** the complete local client. It costs almost nothing to serve.
- **Paid tier:** hosted services, exactly the things that create running and compliance costs.

## 1a. Regards 1.0 (ADR-0018): what we pay for the app itself

**Regards 1.0 is free for everyone. It has no price, because there is nothing to pay for:**
- It is open source.
- It runs entirely on the user's computer.
- It uses no Regards servers.

The items below are **our own optional expenses**, mostly for polish on macOS. **Required total: $0.**

| Item | Required? | Cost | What happens if we skip it |
| --- | --- | --- | --- |
| Windows code signing | Free option | **$0** via SignPath Foundation (open-source projects) | — |
| Linux packages, CI on all 3 OSes, website hosting (GitHub Pages) | Free | **$0** | — |
| Microsoft sign-in registration | Free | **$0** | — (publisher verification also free; it needs a domain to show "verified") |
| Test mailboxes | Free | **$0**: free Gmail, Outlook.com, iCloud, Yahoo, GMX and Infomaniak accounts plus Dovecot in Docker | Paid business test seats (about $7 / month) are optional |
| AI | — | $0 (users bring their own key) | — |
| Privacy statement | Free | $0 ("Regards collects no data and runs no servers") | — |
| **Domain name** | Recommended | about **$10–15 / year** for a cheap domain (*estimate*) | We can start on a free GitHub Pages address. Google's sign-in review needs a site we can prove we own, and whether a github.io address qualifies must be tested. Without Google sign-in, Gmail still works with app passwords |
| **Apple Developer Program** | Recommended once there are Mac users | **$99 / year** | Mac users see a "may contain malware" warning and must allow the app in System Settings. Updates may make the Keychain ask for the password again; a free self-signed certificate might avoid that (to test). We can launch without it and add it later, for example funded by sponsors |
| Trademark "Regards" | Optional | Search $0; filing CHF 450 | Only matters if the project grows; search first, because "Regards" is a common word |

**Bottom line:**

| Budget level | Cost |
| --- | --- |
| **Minimum** | **$0** |
| Comfortable | about $10–15 per year (a domain) |
| Polished on Mac | about $110 per year (domain + Apple) |

**Pricing only exists for the optional 1.1 Pro service**, because that server costs money to run (§1b). If you'd rather have no pricing at all, the alternatives are:
- keep sending-while-off to the free options (provider-held delivery and self-hosting)
- fund the hosted Send Service through sponsors or donations instead of Pro

### When further costs appear

| Milestone | New costs |
| --- | --- |
| Pro: sync and hosted AI (1.x) | Small storage and hosting; AI tokens per user (§4) |
| Full Gmail API / Graph sync adapters (1.x) | $0 in fees, as long as Gmail data stays on the device |
| Mobile apps (later) | Apple: covered by the same $99. Google Play: a one-time registration fee |

## 1b. Regards 1.1: "send while the computer is off", with Pro (ADR-0019, ADR-0018, ADR-0014)

| How it is delivered | Release | Who gets it | Cost to us |
| --- | --- | --- | --- |
| Background mode on by default (computer on) | 1.0 | Everyone, free | $0 |
| Mail provider holds the message (SMTP FUTURERELEASE; Exchange-held delivery if verified) | 1.1 | Everyone, free | $0 |
| Self-hosted Send Service, personal mode (user's own machine) | 1.1 | Everyone, free | $0 |
| **Our hosted Send Service**: works with **any** IMAP/SMTP host, plus Gmail/Microsoft with send-only keys. Passwords are handled only by a sealed enclave worker | 1.1 | **Pro** (30-day free trial) | See below |

**Costs of the hosted Send Service (all *estimates*):**

| Item | First year of 1.1 | Following years |
| --- | --- | --- |
| One enclave-capable host (API + normal worker + sealed password worker) + KMS + monitoring | about $240–480 (about $20–40 per month) | about $240–480 |
| Account service (passkeys) + billing integration hosting | about $0–240 (small; can share the host) | same |
| Security review / penetration test of the enclave code and service | about $3,000–10,000 | about $3,000–10,000 (yearly) |
| Lawyer: privacy policy, terms, data processing agreement | about CHF 2,000–6,000 | small updates |
| Payment provider fees | about 5–10% of revenue | same |
| Per-user cost (storage of a few KB per job, one SMTP/API call per send) | under $0.01 per user per month | same |
| **Total cash cost** | **about $5,500–17,500** | **about $3,300–11,000** |

**Break-even** at €40/year with about 10% fees, so about $36–40 net per Pro user per year:

| Scope | Pro users needed |
| --- | --- |
| Infrastructure only | about 10–20 |
| First year of 1.1, including security review and legal | about 150–500 |
| Following years | about 80–300 |

Developer time is excluded throughout.

**Why Pro is right for this feature:**
- It is the one part of Regards that needs an always-on, security-reviewed server.
- Free users lose nothing local: they get honest wording ("your computer must be on at 09:00") and three free alternatives.

The sections below are the detailed reference behind §1a and §1b.

## 2. Reference: fixed cost items for the app (detail for §1a)

| Item | Why we need it | Cost | Frequency | Needed by |
| --- | --- | --- | --- | --- |
| Apple Developer Program | Developer ID signing + notarization for macOS. Without it, Gatekeeper warnings appear, and Homebrew casks are reported to require signed and notarized apps from 2026-09-01 | **$99** | yearly | Public macOS beta |
| Windows code signing, option 1 | **SignPath Foundation**: free for open-source projects (OSI licence, public repo, automated builds); the signer shows as "SignPath Foundation" | **$0** | — | Public Windows beta |
| Windows code signing, option 2 | **Azure Artifact Signing** (formerly Trusted Signing) Basic: up to 5,000 signatures per month. Swiss organisations are listed as eligible, but individuals only in the US/Canada, and Swiss users have reported form problems; 3-year organisation history may be required | **$9.99/month** (about $120/year) | monthly | Alternative to option 1 |
| Windows code signing, option 3 | Commercial OV certificate from a CA (hardware or cloud key) | a few hundred $ per year (*estimate*; get quotes) | yearly | Fallback |
| Linux signing | GPG-signed packages and repositories | $0 | — | — |
| CI/CD | GitHub Actions standard runners on **public** repos are free on Linux, Windows and macOS | $0 | — | Day one |
| Test mailboxes | Microsoft 365 Business Basic ($7/user/month on annual billing since 2026-07-01) for real tenant behaviour; Google Workspace Business Starter ($7/user/month annual, $8.40 monthly); plus free consumer Gmail, Outlook.com, iCloud and Yahoo accounts | about **$170** | yearly | Phase 1 |
| Domain + static website | Product site, docs, privacy policy, OAuth homepage; hosting on free static tiers | about $10–40 (*estimate*) | yearly | Before OAuth verification |
| Google OAuth app verification (local client) | Required for public use of Gmail scopes; unverified apps are capped. Google charges **no fee**. A purely local client, where data never passes through our servers, *may* not need the paid CASA assessment, but **confirm with Google** | $0 fees; time for the privacy policy, demo video and scope justification | yearly re-verification | Public beta with Gmail OAuth |
| Microsoft publisher verification | A verified publisher for the Entra app registration (free partner-program ID) | $0 | — | Public beta with Microsoft OAuth |
| Trademark (Switzerland, IPI) | Protect the name (ADR-0004): CHF 450 covers up to 3 classes for 10 years (IPI table, status 2024; one 2026 guide says CHF 550, so check) | **CHF 450** | per 10 years | Before public launch |
| Legal review | Licence and trademark policy, privacy policy, terms | CHF 1,000–5,000 (*estimate*) | one-off | Before public launch |

**Reference total for these items (recurring, with paid test seats and Azure signing as the upper bound):**

| Cost line | Per year |
| --- | --- |
| Apple Developer Program | $99 |
| Windows signing | $0–120 |
| Test mailboxes | about $170 |
| Domain | about $10–40 |
| **Total** | **about $280–430** |

On top of that come about CHF 450 for the trademark and the one-off legal review.

## 3. Reference: hosted-service cost items (detail for §1b and later Pro additions)

These are what the paid **Pro** tier pays for (ADR-0014).

| Item | Trigger | Cost | Notes |
| --- | --- | --- | --- |
| **Send Service / sync / account servers** | The sealed Send Service (1.0); encrypted cross-device sync (1.x); passkey accounts and billing | 1.0 Send Service: see §1b. Sync later: small (*estimate*) | Grows slowly: jobs and synced state are tiny |
| **Google CASA security assessment** | Required only for apps whose **restricted** Gmail data is accessed "from or through a third-party server" | Google charges nothing. Labs: reports of **$540–675** (Tier 2) up to **$3,000+** per year | **Not expected for 1.0**: the desktop app keeps restricted-scope data local, and the Send Service holds only `gmail.send` (sensitive). Confirm with Google |
| Security testing | Any server holding user credentials | Penetration test: several thousand $ (*estimate*) | Before Pro launch, then yearly |
| Data protection compliance | GDPR (EU users) and Swiss revFADP | Privacy policy, data processing agreement, records of processing; a GDPR Art. 27 EU representative may be required for a non-EU company: hundreds to a few thousand $ per year (*estimate*) | Ask counsel |
| Payment processing | Any paid tier | A merchant of record typically takes roughly 5% + a fixed fee per transaction (*estimate*; get quotes); Stripe takes less but leaves VAT handling to us | Favour annual billing, so fixed fees matter less |
| Tax | Selling digital services | A merchant of record handles EU and other VAT. Swiss VAT registration becomes mandatory above CHF 100,000 worldwide turnover | Ask an accountant |
| **Hosted AI tokens** | Pro users who use AI without their own key | See §4 | Per-user, usage-driven |

Most hosted-service cost is **security and compliance, not servers**. That is why [ADR-0018](../adr/0018-modern-minimal-v1.md) makes the Send Service **sealed and send-only**: nobody at Regards can read credentials, and Google's restricted-scope audit is avoided.

## 4. Per-user variable cost: hosted AI

Assumptions for an active user, all *estimates*:

- **Volume:** 10 AI actions per working day × 22 days = **220 actions per month**.
- **Size:** about 3,000 input tokens (thread plus instructions) and 300 output tokens per action.
- **Monthly total:** about 0.66 M input + 0.066 M output tokens.

| Model (Anthropic API list price, per million tokens) | Monthly cost per active user |
| --- | --- |
| Claude Haiku 5.5: $0.10 input / $0.50 output | 0.66 × $0.10 + 0.066 × $0.50 ≈ **$0.10** |
| Claude Sonnet 5.5: $2 input / $10 output (cache reads $0.20) | 0.66 × $2 + 0.066 × $10 ≈ **$1.98** |
| BYOK (user's own key) | **$0 to us** |

- **Prompt caching** of stable instructions reduces input cost further.
- **Semantic search** uses a local model, so it costs us nothing.
- **Pricing implication:** a Pro price of €3–5 per month covers hosted AI comfortably with a small model, and a mid-tier model if quotas apply. Other providers fall in similar ranges; compare before choosing.

## 5. Break-even sketch (cash costs only, excluding salaries)

| Scenario | Yearly cash cost | Net revenue per subscriber* | Subscribers to break even |
| --- | --- | --- | --- |
| App only (§1a), sponsorship-funded | about $110–310 + amortised trademark | — | Covered by about 5–10 sponsors at $3/month |
| App + Pro Send Service (§1b, from 1.1) | first year of 1.1 about $5,600–17,800; then about $3,400–11,300 | about $36–40/year (€40/year minus ~10% fees) | **about 150–500 Pro users in the first year; about 80–300 after** |
| Plus one part-time developer (*estimate* $50k/year) | about $55–68k | about $36–40/year | about 1,400–1,900 Pro users |

*Net revenue per subscriber assumes annual billing at about €40 per year. Hosted AI cost is about $1–24 per year per subscriber, depending on model and usage.

## 6. Freemium design (summary of ADR-0014)

| Capability | Free (local) | Pro (paid) | Why it is paid |
| --- | --- | --- | --- |
| Full client on all three OSes, unlimited accounts | ✅ | ✅ | — |
| Smart Inbox, Done/Pin/Set aside, screening, rules | ✅ | ✅ | — |
| Snooze / reminders / send later while the computer is on | ✅ | ✅ | — |
| **Send while the computer is off, any email provider** | Only where the provider holds the message itself, or with a self-hosted Send Service | ✅ hosted Send Service (from 1.1, 30-day trial) | Always-on server, sealed credentials, security review, compliance |
| **Encrypted cross-device sync of app state** | Self-hosted | ✅ hosted | Server and storage |
| Local search incl. semantic | ✅ | ✅ | — |
| AI with own key (BYOK) | ✅ | ✅ | — |
| **AI without a key (hosted quota)** | — | ✅ | Per-token cost |
| Agent CLI/MCP, holds, audit, undo | ✅ | ✅ | — |
| Priority support | Community | ✅ | Staff time |
| Teams (shared inboxes, comments) | — | Teams tier (later) | Collaboration server |

**Principle:** paid = hosted convenience. All server code is open source (AGPL) and self-hostable, and the client is identical for every user.

## 7. Ways to keep costs down

- **Code signing:** use SignPath Foundation's free open-source signing for Windows.
- **CI and hosting:** keep the repository public for free CI, and host the website on free static hosting.
- **Gmail compliance:** keep all restricted-scope Gmail data on the device, and give the Send Service only a separate `gmail.send` client. That avoids CASA; confirm with Google in Phase 0.
- **Send Service design:** sealed (attested enclave + KMS policy), send-only, and deleting data after each send. This limits breach impact and compliance scope.
- **Provider-held delivery first:** FUTURERELEASE and Exchange-held delivery cost us nothing, so the Send Service only covers what providers can't.
- **Hosted AI:** small models with prompt caching; BYOK stays free.
- **Funding:** apply for open-source funding programmes (check eligibility for a Swiss entity), sponsorships and donations.

## 8. Decisions and information needed from the project owner

1. **Legal entity.** Will you sign and sell as an individual or as a Swiss company (GmbH/Sàrl)?
   - Apple organisation accounts need a D-U-N-S number.
   - Azure signing needs organisation validation.
   - The trademark owner and service contracts follow from this choice.
2. **Budget:**
   - **Up to 1.0:** **$0 required**; about $10–110 per year if we want a domain and Apple signing (§1a).
   - **From the start of 1.1:** the Send Service budget, about $5,500–17,500 in its first year (§1b).
3. **Price validation.** Run a short survey or waitlist of early users at the €3–5/month or €30–48/year points.
4. **AI provider(s)** for hosted AI, and the default model and quotas.

## Sources

- [Apple Developer Program fee: $99/year (secondary sources; confirm on developer.apple.com)](https://ambsandigital.com/?p=11104)
- [Azure Artifact Signing pricing](https://azure.microsoft.com/en-gb/products/artifact-signing) and [eligibility quickstart (Swiss organisations listed)](https://learn.microsoft.com/azure/artifact-signing/quickstart); [Swiss organisation form issue report](https://learn.microsoft.com/fr-ch/answers/questions/2128769/how-to-create-an-identity-validation-request-for-a)
- [SignPath for open-source projects](https://about.signpath.io/product/open-source); [signpath.org](https://signpath.org/)
- [GitHub Actions: standard runners free for public repositories](https://github.blog/changelog/2026-01-01-reduced-pricing-for-github-hosted-runners-usage/)
- [Microsoft 365 business price increase, July 2026](https://office-watch.com/2026/microsoft-365-business-price-increase-2026/); [Google Workspace pricing 2026](https://www.emailvendorselection.com/google-workspace-pricing/)
- [Google CASA overview](https://deepstrike.io/blog/google-casa-security-assessment-2025); [CASA Tier 2 cost report](https://meetorbis.com/blog/how-we-passed-google-casa-tier-2-with-claude); [CASA requirement wording and cost discussion](https://www.agenticfabriq.com/blog/google-oauth-verification-casa); [email client CASA cost note](https://github.com/marlinjai/email-mcp/pull/30)
- [Swiss IPI trademark fees](https://ige.ch/en/protecting-your-ip/trade-marks/national-applications/costs-and-fees.html)
- Anthropic API list prices for Claude Haiku 5.5 and Claude Sonnet 5.5, as of 2026-10-06
