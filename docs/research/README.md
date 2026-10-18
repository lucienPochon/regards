# Research: Spark Desktop investigation (Phase 1)

Read-only investigation of the locally installed **Spark Desktop** email client (Spark Mail Limited / Readdle), carried out to inform an **independent, open-source** email client built with Rust, Tauri and React, possibly on top of the open-source **Alice** project.

| Document | Purpose |
| --- | --- |
| [03-independent-implementation-plan.md](03-independent-implementation-plan.md) | Clean-room architecture and phased plan for Regards |
| [04-alice-compatibility-assessment.md](04-alice-compatibility-assessment.md) | Fit of Alice (get-alice/alice) as the foundation, gap analysis and recommendation |
| [06-costs-and-business-model.md](06-costs-and-business-model.md) | Cost inventory (fixed, hosted, per-user), break-even sketch and the open-source freemium model |

**Private notes:** the detailed Spark investigation (01 technical investigation, 02 feature and interaction inventory, 05 evidence ledger) is **not published**. It is kept outside this repository, under the clean-room rule ([ADR-0001](../adr/0001-clean-room-policy.md)): no Spark internals or identifiers in the repository or within reach of coding agents. References to it in public documents read "(private research notes)".

The architecture decisions taken from this research are recorded as ADRs in [`docs/adr/`](../adr/README.md).

## Evidence levels used throughout

| Tag | Meaning |
| --- | --- |
| **[V] Verified** | Directly observed: shipped artifact bytes or manifests, REA tool output, local file metadata, the bundled CLI's own help text, or Spark's official public documentation. |
| **[I] Inferred** | A strong conclusion drawn from several verified observations, not observed at runtime. |
| **[H] Hypothesis** | Plausible but weakly supported. It needs runtime confirmation before it is relied on. |
| **[U] Unknown** | Not determined in this phase. It may be a candidate for a later, still read-only, runtime pass. |

The type and module names quoted in these documents are **identifiers** observed in the shipped artifacts. They are cited as evidence of behaviour and interfaces only. No Spark source code, decompiled code, assets or UI text is reproduced, and none should be copied into our implementation.

## Boundaries respected

- **Spark not modified or run.** Spark was not modified, launched, instrumented, debugged or attached to. It was not running at any point during the investigation.
- **Licensing and authentication untouched.** No licensing, paywall or authentication mechanism was examined for bypass.
- **No credentials read.** Spark's local credential store was not opened. No database rows were read: SQLite databases were inspected **schema-only**, through REA's read-only snapshot (sources opened `O_RDONLY`, never through SQLite). No token, password or key material was viewed.
- **No proprietary code reproduced or recovered.** REA's JavaScript source-recovery and decompilation features were deliberately **not** used.
- **No mail content read.** No emails, contacts or calendar data were read through the `spark` CLI; only `--version` and `--help` were used. Spark's log files were listed by name only.

## Snapshot

- Investigation date: **2026-10-18**
- Subject: Spark Desktop **3.31.6** (file version 3.31.6.141261), Windows x64, per-user install under `%LOCALAPPDATA%\Programs\SparkDesktop`
- Tooling: REA (Reverse Engineer Anything) **6.7.0** CLI, Node.js 24.18, plus small read-only helper scripts kept outside the repository
