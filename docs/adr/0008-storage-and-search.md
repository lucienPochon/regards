# ADR-0008: Storage and search: SQLite with migrations, FTS5 first

- **Status:** Accepted
- **Date:** 2026-10-18
- **Deciders:** Project owner

## Context

We need durable local storage for mail metadata, bodies, drafts, state overlays, queues, audit logs and search indexes, scaling to at least 100k messages per user with instant list rendering.

- **Spark:** about ten SQLite databases, heavily denormalised, with FTS5 plus embeddings (01 §7 (private research notes)).
- **Alice:** JSON blobs in one SQLite connection behind a mutex, with ad-hoc migrations and a full scan for header search ([04 §3](../research/04-alice-compatibility-assessment.md)).

## Decision

1. **Engine and layout:**
   - **SQLite** via `rusqlite` (MIT), bundled build, **WAL mode**, foreign keys on.
   - **One writer task** owns the write connection and serialises transactions. A small **read connection pool** serves queries.
   - **One main database** (`mail.db`). Large or derivable data goes in separate files:
     - `search.db` for indexes, which can be rebuilt
     - a content-addressed `blobs/` directory for raw messages and attachments, compressed
     - **Implementation (Phase 1):**
       - **Naming:** `blobs/<first 2 hex>/<SHA-256>`, written atomically.
       - **What is stored:** a message's source is stored when it is first opened.
       - **Not yet:** compression is not done yet, and blobs of deleted messages are not yet cleaned up. Both come later.
2. **Normalised schema with real columns** for anything queried or sorted (the plan §4 sketch is the starting point). JSON columns are only for opaque provider metadata. Projections (`thread_view`, `list_membership`, counters) are derived tables maintained by the core (ADR-0007).
3. **Migrations:**
   - Versioned, forward-only SQL migrations embedded in the binary.
   - **Library: `rusqlite_migration` 2.6** (Apache-2.0), chosen 2026-10-18.
     - **Why:** it supports rusqlite 0.40, records the version in SQLite's own `user_version`, and its only dependency is `log`.
     - **Not refinery:** `refinery` targets many databases and keeps its own history table, which is more than we need.
   - An automatic backup is taken before a migration: `VACUUM INTO backups/mail-v<N>-<time>.db`, newest three kept. A database from a newer Regards is refused, not modified.
   - Migrations are tested on fixture databases from every released version.
4. **Search, Phase 0–1:**
   - SQLite **FTS5**, external-content tables over subject, participants and body text, with **BM25** ranking.
   - Header fields are indexed columns, not scanned.
   - The query language is Gmail-style (spec in plan §5.10).
   - Typo tolerance via a trigram or spell-suggestion side index.
   - **As built (2026-10-19):**
     - A regular FTS5 table in `search.db`, which holds its own copy of the indexed text. FTS5 reads external content only from its own database, and the text is in `mail.db`.
     - The copy is small: subject, people, and the preview, plus the full text of downloaded messages.
     - **Typo tolerance (2026-10-19):** the side index is FTS5's own word list (an `fts5vocab` table, which stores nothing). When a search finds nothing, each unknown word is compared with the known words that start with its letter. Those within one edit, or two from five letters on, are candidates, and the most common wins. The corrected search is offered ("did you mean") only if it finds something within the same scope.
5. **Search, later and optional:** semantic search with a local embedding model (ONNX Runtime via `ort`) and a vector table (for example `sqlite-vec`, evaluated then), using hybrid ranking (reciprocal rank fusion). If FTS5 limits are hit, migrate the keyword index to `tantivy` (MIT).
6. **Encryption at rest:** v1 relies on OS full-disk encryption and documents that clearly. A later ADR may add database encryption (for example SQLCipher) if users need it.
7. **Secrets are never stored in SQLite.** Only OS credential-store references are (ADR-0013).

## Alternatives considered

- **Many databases like Spark.** Splitting by domain adds cross-database consistency problems. Only rebuildable data (search) is split out. Rejected as the default.
- **JSON-blob documents (Alice).** Prevents indexing and efficient projections. Rejected.
- **An embedded key-value store (sled/redb) plus a custom index.** Loses SQL, FTS5 and tooling. Rejected.
- **tantivy from day one.** Powerful, but a second storage engine to keep consistent; FTS5 is sufficient to start. Deferred.

## Consequences

- Schema design is a Phase-1 deliverable with ADR-level review.
- `search.db` can be deleted and rebuilt at any time; the UI shows indexing progress.
- Benchmarks at 100k and 1M messages gate Phase 1 and Phase 6.

## References

- [SQLite FTS5](https://www.sqlite.org/fts5.html)
- [03 plan §4 data model](../research/03-independent-implementation-plan.md)
