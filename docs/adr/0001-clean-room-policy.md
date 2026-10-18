# ADR-0001: Clean-room policy: no Spark or Alice code

- **Status:** Accepted
- **Date:** 2026-10-18
- **Deciders:** Project owner

## Context

We are building a client *inspired by* Spark Desktop (proprietary, Spark Mail Limited) and informed by Alice (open source, AGPL-3.0-only). Both were examined in Phase 1 ([research](../research/README.md)).

Copyright protects code and other expression (assets, UI text, CSS, icons). It does not protect ideas, behaviour, architecture, or interoperability with open standards. We want freedom to choose our own licence (ADR-0004) and no infringement exposure.

## Decision

1. **No copying.** Nothing from Spark, and no code from Alice, enters this repository. That includes source, decompiled or recovered logic, close paraphrase or line-by-line translation of code, tests, assets, icons, sounds, animations, CSS, UI copy, localisation catalogues, and **identifier or type names**.
2. **Specs from behaviour.** Feature specifications are written from behavioural documentation (02 inventory (private research notes)), public product documentation, open standards (IMAP, SMTP, MIME, iTIP, …) and provider API docs. Alice's ideas are taken only from the design summary in [04 §5](../research/04-alice-compatibility-assessment.md) and Alice's public README and docs, restated in our own words.
3. **Separation during implementation:**
   - Do not have Spark artifacts or Alice source open while writing the corresponding code.
   - AI coding agents working on this repository **must not be given access** to the Alice repository or to Spark's install directory. They receive our specs and ADRs only.
   - The Phase-1 analysis scratch data (REA evidence files and the Alice clone) stays outside this repository.
4. **No trademarks.** "Spark", "Readdle", "Alice" and their logos are never used in our product name, UI, packaging or marketing. Neutral factual references ("inspired by Spark-style workflows") are allowed only in documentation.
5. **No private interoperability.** We do not implement Spark's backend protocols, pipe protocol, file formats or deep-link schemes.
6. **Provenance.** Each ADR and feature spec cites the public sources it relies on. Code review checks for violations of rules 1–4.

## Alternatives considered

- **Copy or port Spark's implementation.** Rejected: copyright infringement, and the stack doesn't fit our goals (ADR-0002).
- **Fork Alice.** Rejected for licence freedom (ADR-0003); remains possible if ADR-0004 ever selects AGPL.
- **No formal policy.** Rejected: an informal approach makes later licensing and contribution decisions risky and hard to defend.

## Consequences

- Slightly slower start, because good ideas have to be re-derived and specified rather than copied.
- A defensible provenance story for our own licence choice and for future contributors or investors.
- Contributors must sign off to the policy (see CONTRIBUTING, to be written under ADR-0004).
- This is engineering policy, not legal advice. A lawyer should review it before a commercial launch.

## References

- [Phase-1 research](../research/README.md); [Alice assessment §6 (licence)](../research/04-alice-compatibility-assessment.md)
- EU Software Directive 2009/24/EC, Art. 1(2): ideas and principles underlying software are not protected by copyright.
