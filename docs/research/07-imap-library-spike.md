# 07: IMAP library spike (Phase 0, ADR-0009)

**Question:** which Rust IMAP library should the Regards sync engine build on: `async-imap` or `imap-next` + `imap-codec`?

**Answer:** **`imap-next` + `imap-codec`**, wrapped in our own `regards-imap` crate, pinned to exact versions, with four upstream fixes to contribute (§5). `async-imap` loses data silently in ordinary use and can be crashed by a hostile server. Those defects are in its core read path, so we would end up owning a fork of two crates.

- **Date:** 2026-10-18
- **Code:** [`spikes/imap-trial/`](../../spikes/imap-trial/) (not product code; how to reproduce is in its README)
- **Evidence levels:** as in the [research index](README.md): **[V]** observed, **[I]** inferred from code reading plus observation, **[U]** unknown.

## 1. Candidates

| | async-imap | imap-next + imap-codec |
| --- | --- | --- |
| Versions tested | 0.12.0 (parser: imap-proto 0.17.0) | imap-next 0.3.4, imap-codec 2.0.0-alpha.9, imap-types 2.0.0-alpha.7 |
| Latest release (crates.io, 2026-10-18) | 2026-09-30 | imap-codec 2026-07-19; imap-next 2026-02-18 |
| Recent downloads | ≈ 961k (imap-proto ≈ 2.2M) | imap-codec ≈ 70k; imap-next ≈ 27k |
| Main user | Delta Chat | pimalaya (`imap-client`, Himalaya) |
| Style | High-level session, commands built as strings | Typed commands and responses; sans-I/O state machine with an optional tokio stream |
| Licence | MIT OR Apache-2.0 | MIT OR Apache-2.0 |

## 2. Method

1. **Live scenario.** The same steps were run with each library against **Dovecot 2.4.5** in Docker, over TLS 1.3 with a throwaway test CA. Each run used a fresh mailbox. Host: Windows 11, Docker Desktop, localhost.

   The steps:
   1. LOGIN, CAPABILITY, ENABLE QRESYNC.
   2. CREATE `Archive`, then APPEND 3 messages.
   3. SELECT (CONDSTORE); UID FETCH of metadata.
   4. UID STORE, then FETCH CHANGEDSINCE.
   5. UID MOVE.
   6. The path for servers without MOVE: COPY + `\Deleted` + UID EXPUNGE.
   7. 150 more APPENDs, change every message, then re-SELECT with QRESYNC and count the changes delivered.
   8. IDLE push from a second connection; cancel IDLE locally, then DONE and NOOP.
   9. LOGOUT.

   The scenario was repeated 6 times.
2. **Hostile server.** A local fake server answers LOGIN and SELECT normally, then replies to FETCH with malformed bytes. There were 19 hand-written cases and 2,000 fuzzed FETCH responses per library. Release build. Each library ran in a **child process**, so a stack overflow or failed allocation could be recorded instead of ending the test.

## 3. Results: live scenario against Dovecot

| Check | async-imap | imap-next |
| --- | --- | --- |
| TLS 1.3, LOGIN, CAPABILITY, CREATE | pass | pass |
| ENABLE QRESYNC | pass; raw command string | pass; typed |
| APPENDUID (new message's UID) | **not exposed**: `append()` returns `()` | typed, 3 of 3 |
| SELECT (CONDSTORE), UID FETCH of UID, FLAGS, ENVELOPE, BODYSTRUCTURE, MODSEQ | pass | pass |
| UID STORE, then FETCH CHANGEDSINCE | pass; modifier written as raw text | pass; typed |
| UID MOVE: VANISHED and COPYUID | both arrive, untyped, only through a side channel | both typed |
| COPY + `\Deleted` + UID EXPUNGE | works; `uid_copy()` does not return COPYUID | works; COPYUID typed |
| **QRESYNC re-SELECT after changing every message** | **91 of 151 changes delivered, in every run; the rest silently dropped** | **301 of 301**, in every run |
| 150 × APPEND | 1.8–3.8 s (sequential only) | 2.2–4.5 s sequential; 1.7–3.1 s pipelined with LITERAL+ (one outlier at 11.9 s). Limited by the server |
| IDLE: new mail pushed | ≈ 507 ms | ≈ 507–515 ms. The delay is Dovecot's own notification delay |
| IDLE cancel, then DONE, then NOOP | pass | pass. Dropping an in-flight read mid-wait left the connection consistent |
| LOGOUT | pass | **tagged OK lost in 3 of 12 LOGOUTs** ("Stream was closed"); see §5 item 2 |

## 4. Results: hostile server

"Skipped" means the library rejected that one response, kept reading and still completed the command. That is the behaviour a sync engine needs.

| Case | async-imap | imap-next |
| --- | --- | --- |
| Binary garbage greeting | error | error |
| Unbalanced parentheses | error | skipped |
| UID beyond 32 bits / UID 0 / sequence number 0 | **accepted as valid** | skipped |
| NUL or invalid UTF-8 in a quoted string | **accepted** | skipped |
| Valid UTF-8 or Latin-1 bytes in a quoted string | accepted | **skipped** (strictly 7-bit; see §5 item 4) |
| UTF-8 sent as a literal (the correct form) | accepted | accepted |
| One malformed FETCH between two valid ones | **1 of 2 valid delivered**; the stream ends at the error | 2 of 2 valid delivered |
| Literal claiming 1 TiB, then EOF | error | error |
| Truncated response, then EOF | error | error |
| LF-only line endings | error | error (strict by default; a relaxed option exists) |
| **100,000 nested parentheses** | **process killed: stack overflow** | skipped in 8 ms |
| **128 MiB single line** | **no result after 60 s** | rejected by the 100 MiB response limit in about 2 s; command completed |
| **EOF before the tagged reply** (2 cases) | **reported as a complete, successful FETCH** | error |
| 2,000 fuzzed FETCH responses | 1,835 error, 161 accepted, 4 skipped | 1,875 skipped, 107 accepted, 18 error |

Neither library panicked or hung on any fuzzed input.

## 5. Findings

### async-imap: why it is rejected

1. **Silent data loss [V].**
   - **What happens:** responses the library did not ask for, including the FETCH and VANISHED lines a QRESYNC SELECT returns, go into a channel bounded at 100 entries. The send is `try_send(..).ok()`, so anything over the limit is dropped without an error.
   - **How the count fits:** 91 = 100 minus the 9 other untagged SELECT responses.
   - **Effect:** a resync after many changes loses flag changes and deletions.
2. **A dropped connection looks like success [V].** A FETCH stream that ends at EOF, without the tagged reply, yields its items and no error. A sync engine listing UIDs to find deletions would treat every message after the cut-off as deleted.
3. **Hostile-server crashes [V], [I].**
   - **Stack overflow:** deeply nested input overflows the stack in the recursive parser and kills the whole process [V].
   - **Huge line:** a 128 MiB line does not finish within 60 s. Most likely the growing buffer is re-parsed from the start on every read; this was inferred, not profiled [I].
4. **Commands are strings [V].** Flags, FETCH items and modifiers are passed as raw protocol text. Any value that comes from mail or from an agent would need escaping by the caller, which risks command injection.
5. **Protocol results are hard to reach [V].** There is no APPENDUID, no COPYUID from COPY, and no typed ENABLE, QRESYNC or VANISHED. The engine needs all of these.
6. **Misuse panics [V].** Calling IDLE `done()` or `wait_with_timeout()` before `init()` hits an `assert!`.

Fixing items 1–3 means forking both async-imap and imap-proto and changing their core read path and parser.

### imap-next + imap-codec: what we must fix or work around

1. **The alpha codec does not build on its own [V].** imap-codec 2.0.0-alpha.9 turns off nom's default features but uses its `alloc` items. **Workaround:** depend on `nom` with `features = ["alloc"]`. **Upstream:** a one-line manifest fix.
2. **The final reply before a TLS close can be lost [I].**
   - **Cause:** in `stream.rs` `decrypt()`, when decrypted bytes and the server's TLS close notice arrive in the same read, the function returns "closed" and drops the bytes it has already decrypted.
   - **What was observed:** the LOGOUT reply was lost intermittently.
   - **Effect:** small. It would also drop a final `BYE [ALERT] …` reason.
   - **Upstream:** return the pending bytes first and report the close on the next call. Confirm with a patched build before filing.
3. **No Gmail extensions [V].** imap-types has no `X-GM-MSGID`, `X-GM-THRID` or `X-GM-LABELS` (nor `X-GM-RAW`), and no generic fallback. They can be neither requested nor parsed.
   - **Effect:** Gmail over IMAP then works only as folders, without labels or thread IDs.
   - **Upstream:** an `ext_gmail` feature. imap-proto already parses these, which confirms the shape.
4. **Quoted strings are strictly 7-bit [V].**
   - **What happens:** raw 8-bit bytes inside a quoted string make the whole response rejected.
   - **Correctness:** this follows IMAP4rev1, and Dovecot sends such strings as literals [V]. Other servers are **unknown [U]**; check Gmail, Microsoft, Infomaniak and iCloud in Phase 1.
   - **Mitigation:** fetch headers with `BODY.PEEK[HEADER.FIELDS (…)]`, which always arrives as a literal and can carry any bytes, and parse them with `mail-parser`, instead of relying on ENVELOPE.
   - **Upstream:** a quirk feature.
5. **Maturity [V].** The codec is a 2.0 alpha, and imap-next has about 35× fewer recent downloads than async-imap.
6. **Protocol bytes in TRACE logs [V]** (found while building `regards-imap`).
   - **What happens:** `fragment.rs` logs every command and response fragment in plain text at TRACE level, in release builds too. That includes LOGIN passwords and mail content. imap-types' `Secret` hides values in `Debug` only in release builds.
   - **Mitigation:** Regards caps the `imap_next` target at INFO whatever `REGARDS_LOG` says. A test checks that a password never reaches the log.
   - **Upstream:** redact credentials (LOGIN/AUTHENTICATE) in fragment traces.

## 6. Server findings (Dovecot 2.4.5)

These apply whichever library is used.

- **Missing HIGHESTMODSEQ [V].** The first CONDSTORE SELECT of a newly created mailbox returned **neither HIGHESTMODSEQ nor NOMODSEQ**, although RFC 7162 requires one. It appeared after the first MODSEQ fetch. The engine must treat a missing value as unknown and use the highest per-message MODSEQ.
- **IDLE delay [V].** IDLE notifications arrive about 0.5 s after the change.
- **8-bit header values [V].** ENVELOPE strings containing 8-bit bytes are sent as literals.

## 7. Decision and conditions

Use **imap-next + imap-codec**, under these conditions (recorded in ADR-0009):

1. **One crate depends on it.** Only `regards-imap` depends on imap-next and imap-types, and it exposes our own types. Changing library later then touches one crate.
2. **Exact pins.** Pin exact versions (`=0.3.4`, `=2.0.0-alpha.9`, `=2.0.0-alpha.7`) and review every update.
3. **Upstream first.**
   - **Report:** the four items in §5 as issues or pull requests.
   - **Until they are merged:** carry the patches in a fork used through `[patch.crates-io]`, allowed by name in `deny.toml`.
   - **Order:** the Gmail extension must land before Gmail-over-IMAP acceptance in Phase 1.
4. **Engine rules from this spike:**
   - Skip a malformed response, log its UID with no content, and re-fetch that message by a simpler path.
   - Never infer deletions from an incomplete response. Only VANISHED/EXPUNGE, or a command that completed with a tagged OK, count.
   - Fetch headers as literals (`HEADER.FIELDS`).
   - Download large bodies in chunks (partial FETCH), so the response-size limit can stay well under 100 MiB.
   - Allow HIGHESTMODSEQ to be missing.
5. **Regression suite.** The hostile-server cases and the fuzzer become the Phase-1 fault-injection suite.

## 8. Not covered

- Real providers: Gmail, Microsoft 365, Infomaniak and iCloud need test accounts.
- COMPRESS=DEFLATE.
- `imap-client`, pimalaya's higher-level layer over imap-next. We need direct control of pipelining and IDLE, but it is worth a look once `regards-imap` exists.
