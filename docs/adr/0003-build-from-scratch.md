# ADR-0003: Build from scratch; Alice is a design reference only

- **Status:** Accepted
- **Date:** 2026-10-18
- **Deciders:** Project owner

## Context

Alice ([get-alice/alice](https://github.com/get-alice/alice)) matches our stack (Rust + Tauri 2 + React, with a daemon owning SQLite). The [assessment](../research/04-alice-compatibility-assessment.md) found:

- **Licence:** AGPL-3.0-only. Any fork or code reuse would make our product AGPL-3.0-only.
- **Age and maintenance:** about one week old, largely AI-generated, with one maintainer.
- **Feature fit:** 12 of 31 requirement rows absent, including every Spark-like workflow feature.
- **Foundations:** the IMAP stack, storage schema, compose model and UI shell need replacing; about 7k of its 23k Rust lines would go in Phase 1 anyway.

The project owner wants licence freedom (ADR-0004).

## Decision

1. **New workspace.** We create a new Rust/Tauri workspace from scratch with our own architecture (ADR-0005 to ADR-0012).
2. **Ideas only.** Alice is used **only as a source of ideas**, under the clean-room policy (ADR-0001). We re-specify in our own words, then implement independently:
   - a daemon owning the store, with a data-directory lock and an authenticated local API
   - a client/daemon **build-identity handshake** that restarts a mismatched daemon
   - an **at-most-once send** delivery claim with an `uncertain` state that blocks blind retries
   - a Gmail-style **search query language** with typo tolerance
   - a **sandboxed HTML reader** (sanitiser + script-less frame + per-frame CSP + remote images opt-in)
   - secrets only in the OS credential store
   - BYOK AI with **strictly validated structured output** and user-reviewed actions
   - a **locale-catalogue consistency linter**
   - containerised **IMAP/SMTP test fixtures**
3. **Permissive crates instead.** Where Alice relies on a library, we evaluate that library on its own merits. All libraries planned in ADR-0009 are permissively licensed.
4. **Optional relicensing request.** The project owner may ask Alice's maintainer whether they would relicense (MIT/Apache) or collaborate. If they agree **in writing**, a new ADR may revisit code reuse; until then, this ADR stands.

## Alternatives considered

- **Fork Alice (AGPL).** Fastest start, but locks the whole product into AGPL-3.0-only and inherits foundations we would replace anyway. Rejected while licence freedom is required.
- **Upstream-first contribution to Alice.** Shared maintenance, but tied to a single maintainer's priorities and to AGPL. Rejected for now.
- **Start fresh without studying Alice.** Wastes useful lessons that are legally free to reuse as ideas. Rejected.

## Consequences

- More initial work: OAuth flows, vault, daemon lifecycle, packaging and CI must be written by us. This is planned in Phase 0 of the [implementation plan](../research/03-independent-implementation-plan.md).
- Full control of schema, API and licence from day one.
- Lessons and known pitfalls from Alice's own roadmap are reusable *as facts*. For example: blocking `imap` 2.x and native-tls limitations, Windows Credential Manager blob-size limits, and the need for References threading and a change feed.

## References

- [04 Alice compatibility assessment](../research/04-alice-compatibility-assessment.md)
- [ADR-0001](0001-clean-room-policy.md), [ADR-0004](0004-licensing-and-trademark.md)
