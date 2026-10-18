# ADR-0015: Privacy, telemetry and AI data handling

- **Status:** Accepted
- **Date:** 2026-10-18
- **Deciders:** Project owner

## Context

- **Spark:** bundles Sentry, Amplitude, Braze and OpenTelemetry. It uploads some contacts when Gatekeeper or Priority is used, supports read-receipt tracking, and routes AI through its services (01 §9–10 (private research notes)).
- **Our position:** a local-first, open-source client is only credible if its data handling is minimal and transparent.
- **Our users:** we operate from Switzerland (revFADP) and will serve EU users (GDPR).

## Decision

1. **No third-party analytics or marketing SDKs** in the client.
2. **Crash reports are opt-in.** Shown on first crash, with a preview of what is sent. Reports are scrubbed of email content, addresses and tokens, and go to a self-hosted or privacy-respecting endpoint. Local logs never contain message bodies or secrets, and they rotate.
3. **Local-first by default.** Mail, contacts, classification, rules, search indexes and agent audit logs stay on the device. The free tier makes **no network calls except** to the user's mail, contact and calendar providers, the update check (which can be disabled), and services the user explicitly enables.
4. **Reading privacy:**
   - Remote images are blocked by default (per-sender allow-list).
   - Link tracking parameters can be stripped (optional).
   - No read receipts or tracking pixels in mail we send.
5. **AI:**
   - **BYOK by default:** OpenAI, Anthropic or any OpenAI-compatible endpoint, including local servers. Keys are stored in the OS credential store and scoped to the endpoint origin.
   - Nothing is sent to an AI provider without an explicit user action, or a rule the user created naming the AI action.
   - The UI shows what will be sent (message count, character caps).
   - Verification-code and sensitive messages are excluded from AI context.
   - Model output is untrusted. Structured outputs are schema-validated, and proposed actions require user approval, or agent policy per ADR-0012.
   - **On-device models** (embeddings for semantic search) run locally via ONNX Runtime. Model weights must have licences permitting redistribution.
   - **Hosted AI (Pro, ADR-0014)** is opt-in. It uses providers' no-training / limited-retention terms, and its data flow and retention are documented.
6. **Hosted services (when built):** data minimisation, end-to-end encryption for synced state, a published privacy policy and data processing agreement, region selection where feasible, and deletion on account closure.

## Alternatives considered

- **Opt-out telemetry.** Common, but erodes trust in an email client. Rejected.
- **Bundled cloud AI by default.** Ongoing cost, and private mail sent to a third party without explicit consent. Rejected.

## Consequences

- Product decisions rely on opt-in feedback, user interviews and public issue trackers rather than behavioural analytics.
- A privacy review is part of the release checklist; the network calls a build makes are listed in documentation and verified in CI with an egress allow-list test.

## References

- [ADR-0012](0012-agent-gateway.md), [ADR-0014](0014-business-model-open-source-freemium.md)
