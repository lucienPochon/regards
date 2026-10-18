# ADR-0009: Mail protocol stack and provider abstraction

- **Status:** Accepted. The IMAP crate choice is pending the Phase-0 spike. **Adapter order and timing are amended by [ADR-0018](0018-modern-minimal-v1.md):** 1.0 syncs through IMAP/SMTP only, with OAuth sign-in for Microsoft and Google (Google once approved) and app passwords elsewhere. The Gmail API and Graph serve only as **send-only** paths in the Send Service; full Gmail API / Graph sync adapters come after 1.0. The engine must also run efficiently on servers **without** CONDSTORE, QRESYNC or MOVE (Infomaniak's capabilities, ADR-0018).
- **Date:** 2026-10-18
- **Deciders:** Project owner

## Context

We must support generic IMAP/SMTP, Gmail and Microsoft 365/Outlook. Tenants may restrict IMAP/SMTP AUTH, and Gmail labels behave differently from folders.

- **Spark:** uses MailCore2/libetpan, a Gmail REST client, Microsoft Graph and EWS (01 §8 (private research notes)).
- **Alice:** uses the blocking `imap` 2.4 crate with a vendored, patched `imap-proto` and native-tls. It has no IDLE, no QRESYNC and no provider trait, and logs in again for every action ([04 §3](../research/04-alice-compatibility-assessment.md)).

## Decision

1. **Provider trait.** A `MailProvider` trait in the core exposes **capabilities**, not protocol details:
   - mailbox/label listing with roles
   - incremental sync (cursor-based)
   - fetch headers, bodies and attachments
   - set flags/keywords
   - move/copy and label add/remove
   - append, draft save/replace
   - send
   - push subscription
   - server-side search (optional)

   Each adapter declares its capabilities. The core plans operations against those capabilities, not against IMAP specifics.
2. **Adapters and order:**
   1. **IMAP + SMTP** (Phase 1). Covers generic hosts, plus Gmail and Outlook.com over IMAP with OAuth2 (XOAUTH2/OAUTHBEARER) or app passwords.
      - IMAP features: CONDSTORE, **QRESYNC**, **IDLE**, MOVE (falling back to COPY + flag + UID EXPUNGE when UIDPLUS is available), SPECIAL-USE, UIDPLUS, COMPRESS where available, and Gmail X-GM-EXT-1 (labels, thread ID).
      - Connections: a **per-account connection actor** holds one long-lived IDLE connection and one command/sync connection, with reconnect and backoff.
   2. **Microsoft Graph** (Phase 2–3). Delta sync, `sendMail`, drafts and change notifications where available. This is the preferred path for Microsoft 365, because organisations may disable IMAP/SMTP AUTH.
   3. **Gmail API** (Phase 7). History-based sync, native labels and threads, and drafts, for performance and label fidelity at scale.
   4. **JMAP** (later). An open standard; Fastmail and others support it.
3. **Libraries:** all permissive, with versions current on crates.io on 2026-10-18.
   - **IMAP:** a Phase-0 spike picks between `async-imap` (0.12, MIT/Apache) and `imap-next` + `imap-codec` (MIT/Apache, sans-I/O, strong extension coverage). Criteria: IDLE cancellation, QRESYNC/VANISHED, COPYUID, X-GM attributes, robustness to malformed server responses (**no panics on bad input**), and maintenance activity.
   - **SMTP:** `lettre` (MIT), async with rustls.
   - **MIME:** `mail-parser` (Apache-2.0 OR MIT) for parsing; `mail-builder` (Apache-2.0 OR MIT) for building `multipart/alternative` + `related` + attachments.
   - **TLS:** `rustls` with the platform certificate verifier. Certificate validation is always on, with no "accept invalid certs" option in the UI.
   - **OAuth2:** the `oauth2` crate (MIT/Apache) with PKCE and a loopback redirect; tokens are refreshed by the daemon.
   - **HTTP:** `reqwest` with rustls (for Graph, the Gmail API and autodiscovery).
4. **Account setup and autoconfiguration:** in order, Mozilla ISPDB-style autoconfig, the provider's well-known endpoints, DNS SRV (RFC 6186), then manual entry. Our own provider presets are written from public provider documentation.
5. **Threading:** JWZ-style `References`/`In-Reply-To` threading, with provider thread IDs (Gmail thread ID, Graph `conversationId`) preferred when available and subject fallback only as a last resort.

## Alternatives considered

- **Blocking `imap` 2.x (Alice).** Unmaintained parser, panics on bad input, IDLE cannot be cancelled. Rejected.
- **Binding MailCore2/libetpan (Spark's choice) via FFI.** Brings C/C++ into the build on every OS, contrary to ADR-0002. Rejected.
- **EWS.** Microsoft is retiring EWS for Exchange Online in favour of Graph. Rejected.

## Consequences

- Phase 1 is the largest phase (sync correctness is hard). We need containerised Dovecot/Postfix fixtures plus a fault-injection proxy (dropped connections, UIDVALIDITY changes, malformed responses).
- Google and Microsoft OAuth app registrations and verification are required (costs and timeline in [06](../research/06-costs-and-business-model.md)).
- Users may bring their own OAuth client ID as a documented advanced option.

## References

- RFC 9051 (IMAP4rev2), RFC 7162 (CONDSTORE/QRESYNC), RFC 2177 (IDLE), RFC 6851 (MOVE), RFC 4315 (UIDPLUS), RFC 6154 (SPECIAL-USE), RFC 6186 (SRV), RFC 7628 (OAUTHBEARER)
