# ADR-0004: Licensing, contributions and trademark

- **Status:** **Accepted** (2026-10-18): the app code is `Apache-2.0 OR MIT`, and server components are `AGPL-3.0-only`. Trademark registration is optional (see item 5).
- **Date:** 2026-10-18
- **Deciders:** Project owner

## Context

- **The project must remain open source,** and it may fund itself through paid hosted services (ADR-0014).
- **Licence freedom is why we build from scratch** (ADR-0003).
- **Two kinds of code exist:**
  - code users run on their own machines: client, core, CLI, MCP server
  - code we would run as a service: relay, sync and hosted-AI gateway (ADR-0011, ADR-0014)
- **Dependency licences:** all planned dependencies are permissive. Licences were checked on crates.io on 2026-10-18 for tauri, tokio, async-imap, imap-codec, imap-next, lettre, mail-parser, mail-builder, rusqlite, tantivy, keyring, oauth2, axum, rustls and ort. All are MIT, Apache-2.0 or similar.

## Decision (proposed)

1. **Client, core, CLI, MCP server and shared crates: `Apache-2.0 OR MIT`** (dual licence, the Rust ecosystem convention).
   - Maximum adoption and contributor friendliness.
   - Compatible with every dependency.
   - Apache-2.0 adds an explicit patent grant.
2. **Server components (relay, sync, AI gateway, account/billing service): `AGPL-3.0-only`.** Anyone who runs a modified version as a network service must publish their changes, which protects our paid services from closed-source free-riding while keeping them open and self-hostable.
3. **Separate repositories or crates per licence.** No AGPL code is linked into the client. The client talks to servers only over documented network protocols.
4. **Contributions: Developer Certificate of Origin (DCO) sign-off** (`Signed-off-by:`) on every commit; no CLA. Inbound = outbound licence.
5. **Trademark:**
   - The product name is **Regards** (decided 2026-10-18).
   - **Before filing**, run a free clearance search in Swissreg (IPI), EUIPO eSearch, the USPTO and the WIPO Global Brand Database for software and email classes (9, 38, 42). "Regards" is a common word in email sign-offs, so the office may see it as weakly distinctive for email software, and prior marks may exist. If needed, the protected mark can be a word-plus-logo combination or a distinctive form, while the app is still called Regards.
   - **Registration is optional and not needed for 1.0.** Do the free clearance search now. Register (IPI filing CHF 450, no lawyer required) only **once the project has real users, or before Pro launches in 1.1**. Its purpose is to let us take down fake or malicious "Regards" builds and prevent someone else from registering the name; the open-source licence covers the code, not the name. Start in Switzerland, and add the EU and US only if needed.
   - Publish a trademark policy in the style of other open-source projects: forks are welcome, but modified builds must be renamed and must not use our logo, and the name may be used to refer to the project.
   - The brand is the main protection for the paid service, since the code is open.
6. **Compliance tooling:** `cargo-deny` (licences, advisories, sources) and an npm licence check in CI; a `THIRD_PARTY_NOTICES` file generated at build time; SPDX headers in source files.

## Alternatives considered

| Option | Pros | Cons |
| --- | --- | --- |
| **A. Permissive client + AGPL server (proposed)** | Broad adoption; protects hosted services; standard for open-core projects | Someone can ship a closed-source fork of the client (mitigated by trademark) |
| B. GPL-3.0 client + AGPL server | Keeps every client fork open | Fewer corporate contributors and embedders; GPL/App Store friction for future mobile builds |
| C. Everything AGPL-3.0 | Maximum copyleft; would also allow Alice code reuse | Mobile app-store friction; deters contributors; heavy obligations for embedders |
| D. Everything Apache/MIT | Simplest | A competitor could run a modified closed hosted service built on our server code |

## Consequences

- We may never move server code to a proprietary licence without every contributor's consent. That is consistent with "remain open source anyway".
- **If we ever want to dual-license the server commercially**, we would need a CLA *from the start* for server repositories. That decision has to be made before the first external server contribution.
- Free-tier features must not depend on AGPL server code being linked into the client (rule 3).
- A lawyer should review this ADR, the trademark policy and the service terms before launch. This ADR is not legal advice.

## References

- [04 Alice assessment §6](../research/04-alice-compatibility-assessment.md); [06 Costs & business model](../research/06-costs-and-business-model.md)
- [Developer Certificate of Origin](https://developercertificate.org/)
- [Swiss IPI trademark fees](https://ige.ch/en/protecting-your-ip/trade-marks/national-applications/costs-and-fees.html)
