# ADR-0009: Mail protocol stack and provider abstraction

- **Status:** Accepted. **IMAP library chosen by the Phase-0 spike (2026-10-18): `imap-next` + `imap-codec`**, under the conditions in §3 ([spike report](../research/07-imap-library-spike.md)). **Adapter order and timing are amended by [ADR-0018](0018-modern-minimal-v1.md):** 1.0 syncs through IMAP/SMTP only, with OAuth sign-in for Microsoft and Google (Google once approved) and app passwords elsewhere. The Gmail API and Graph serve only as **send-only** paths in the Send Service; full Gmail API / Graph sync adapters come after 1.0. The engine must also run efficiently on servers **without** CONDSTORE, QRESYNC or MOVE (Infomaniak's capabilities, ADR-0018).
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
   - **IMAP: `imap-next` 0.3 + `imap-codec` 2.0 (MIT/Apache, typed, sans-I/O).** It was chosen over `async-imap` 0.12 by the [Phase-0 spike](../research/07-imap-library-spike.md).
     - **Rejecting async-imap:** it silently dropped 60 of 151 QRESYNC changes, reported a dropped connection as a complete FETCH, and was killed by a stack overflow from a hostile server.
     - **imap-next in the same tests:** it delivered every change, skipped malformed responses and carried on, and survived every hostile-server case.

     Conditions:
     - **Isolation:** only the `regards-imap` crate depends on imap-next/imap-types, and it exposes Regards types.
     - **Pins:** exact versions, each update reviewed.
     - **Upstream fixes first:**
       - the codec's `nom` feature
       - the final bytes lost before a TLS close
       - a Gmail `X-GM-*` extension, required before Gmail-over-IMAP acceptance in Phase 1
       - an 8-bit quoted-string quirk

       Until merged, these are carried in a fork through `[patch.crates-io]`, allowed by name in `deny.toml`.
     - **Engine rules:**
       - skip and re-fetch a malformed response
       - never infer deletions from an incomplete response
       - fetch headers as literals (`BODY.PEEK[HEADER.FIELDS …]`)
       - download large bodies in chunks
       - tolerate a missing HIGHESTMODSEQ (observed on Dovecot 2.4.5)
   - **SMTP:** `lettre` (MIT), async with rustls.
   - **MIME:** `mail-parser` (Apache-2.0 OR MIT) for parsing; `mail-builder` (Apache-2.0 OR MIT) for building `multipart/alternative` + `related` + attachments.
   - **TLS:** `rustls` with the platform certificate verifier. Certificate validation is always on, with no "accept invalid certs" option in the UI.
     - **Implementation (Phase 1):**
       - **Verifier and crypto:** `rustls-platform-verifier` with the `ring` crypto provider. ring builds with a plain C compiler on every OS.
       - **IMAP connection:** TLS from the first byte (port 993). IMAP STARTTLS is not supported yet.
       - **Extra trust roots:** only tests can add extra root certificates, and debug builds through `REGARDS_DEV_EXTRA_CA` for local test servers. Release builds ignore that variable.
     - **Logging:** imap-next logs raw protocol bytes at TRACE level, so its log target is capped at INFO ([spike 07](../research/07-imap-library-spike.md) §5 item 6).
   - **OAuth2:** the `oauth2` crate (MIT/Apache) with PKCE and a loopback redirect; tokens are refreshed by the daemon.
   - **HTTP:** `reqwest` with rustls (for Graph, the Gmail API and autodiscovery).
4. **Account setup and autoconfiguration:** in order, Mozilla ISPDB-style autoconfig, the provider's well-known endpoints, DNS SRV (RFC 6186), then manual entry. Our own provider presets are written from public provider documentation.
5. **Threading:** JWZ-style `References`/`In-Reply-To` threading, with provider thread IDs (Gmail thread ID, Graph `conversationId`) preferred when available and subject fallback only as a last resort.
   - **Implementation (Phase 1, References first):**
     - **Grouping:** every Message-ID a message has or refers to (Message-ID, In-Reply-To, References) maps to one thread per account (`thread_ids`). A message joins the thread of any ID it shares, so a reply that arrives before its parent still meets it. A message that links two threads merges them.
     - **Across folders:** copies in Inbox, Sent and All Mail share a Message-ID and so a thread. `thread.get` returns the conversation oldest first, one entry per message, with the folder it is in (or on its way to).
     - **When:** each message is threaded as sync stores it. Messages stored before threading existed are threaded in the background at startup, 500 at a time.
     - **Not yet:** the JWZ parent tree (replies are a flat list), subject fallback, and Gmail thread IDs.

## Alternatives considered

- **Blocking `imap` 2.x (Alice).** Unmaintained parser, panics on bad input, IDLE cannot be cancelled. Rejected.
- **`async-imap` 0.12** (more widely used: Delta Chat, about 35× the downloads). Rejected after the spike.
  - **Defects:** silent loss of unsolicited responses over a 100-entry buffer; EOF treated as success; a stack overflow and a 60 s+ stall on hostile input; string-built commands; no APPENDUID.
  - **Cost of fixing:** forking both async-imap and imap-proto.
- **Binding MailCore2/libetpan (Spark's choice) via FFI.** Brings C/C++ into the build on every OS, contrary to ADR-0002. Rejected.
- **EWS.** Microsoft is retiring EWS for Exchange Online in favour of Graph. Rejected.

## Consequences

- Phase 1 is the largest phase (sync correctness is hard). We need containerised Dovecot/Postfix fixtures plus a fault-injection proxy (dropped connections, UIDVALIDITY changes, malformed responses). The spike's hostile-server cases and fuzzer are the starting point.
- **Dependence on an alpha codec with a small user base.** We take on reviewing its updates and contributing fixes upstream. If upstream stalls, we maintain the fork; the `regards-imap` boundary keeps that contained.
- **Open question [U]:** whether Gmail, Microsoft, Infomaniak or iCloud send raw 8-bit bytes in quoted strings. Phase-1 live tests answer it.
- Google and Microsoft OAuth app registrations and verification are required (costs and timeline in [06](../research/06-costs-and-business-model.md)).
- Users may bring their own OAuth client ID as a documented advanced option.

## References

- RFC 9051 (IMAP4rev2), RFC 7162 (CONDSTORE/QRESYNC), RFC 2177 (IDLE), RFC 6851 (MOVE), RFC 4315 (UIDPLUS), RFC 6154 (SPECIAL-USE), RFC 6186 (SRV), RFC 7628 (OAUTHBEARER)
