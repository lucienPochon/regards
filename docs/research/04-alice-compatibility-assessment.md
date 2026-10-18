# Alice as a foundation: compatibility assessment

**Candidate:** [get-alice/alice](https://github.com/get-alice/alice), "a lightweight client for email", version 0.1.0
**Snapshot assessed:** commit `ba7fac2b8b19`, 2026-10-13, shallow clone
**Method:** a read-only code review (nothing was built or executed). Key claims were spot-checked directly in the source: daemon enforcement, the compose model, category handling, dependencies and Windows status. Tags: **[V]** seen in code or docs, **[I]** inferred.

> **Decision taken (2026-10-18):** the project owner requires licence freedom, so **Option C was chosen**: build from scratch and use Alice only as a design reference ([ADR-0003](../adr/0003-build-from-scratch.md)), under the clean-room rules of [ADR-0001](../adr/0001-clean-room-policy.md). The analysis below is kept as the record behind that decision.

---

## 1. Verdict

**Alice matches our target stack and architecture almost exactly, but its feature set and several foundations are early-stage.** Against our requirements, it is a good **skeleton and reference design**, not a near-complete product to extend.

What matches:

- a Rust 2024 workspace with a Tauri 2 shell and a React 19 UI
- one local daemon that owns SQLite
- secrets in the OS credential store
- CLI + TUI + MCP clients sharing the daemon

What would need to be built or replaced:

- **Spark-like features:** most Spark-like features are absent (Smart Inbox, snooze, send later, undo, reminders, offline queue, IDLE, rich compose).
- **Foundations:** Alice's own roadmap already plans to replace the IMAP stack, migrate the schema, and add an operations queue and a jobs scheduler.

**Recommendation:**

- **If AGPL-3.0 is acceptable:** fork Alice and evolve it with a planned replacement of its sync, storage and compose layers in our Phases 0–1, keeping its well-built pieces (section 5).
- **If we need a permissive licence:** do **not** reuse Alice code. Use it only as an architectural reference and build fresh (Option C, section 6).

## 2. Project maturity and governance [V]

| Signal | Observation |
| --- | --- |
| Age | Project directory dated 2026-10-11; the newest commit is 2026-10-13. The codebase is about one week old today |
| People | One visible maintainer, plus heavy AI-agent authorship: roadmap pages written by an agent in the first person, Codex project paths in reviews, 25 pinned agent skills, design-critique agent outputs |
| Size | About 23k lines of Rust (core 16k, CLI 4.3k, Tauri 2.2k) plus 2.4k lines (219 KB) of TSX/TS. `App.tsx` is a 48 KB monolith |
| Tests / CI | 108 Rust tests; smoke, reader, HTML and TUI scripts; Dovecot/Postfix protocol job; a CI matrix for macOS, Windows and Ubuntu; no frontend unit-test framework |
| Verification | Live-tested only with Gmail and an app password on macOS. **Windows was only cross-compiled**: the native installer, GUI, Credential Manager and TUI have never been run. Google and Microsoft OAuth flows are not verified live |
| Governance | No CONTRIBUTING, CODE_OF_CONDUCT, SECURITY policy, CLA or issue templates |
| Roadmap | `todo.md` has 202 open items. `docs/roadmap/*` is detailed and RFC-cited, and it largely mirrors our requirement list |

Implication: upstream will move fast and break things, and API stability cannot be assumed. Bus-factor risk is high.

## 3. Requirement fit matrix

Fit: ✅ present and usable · 🟡 partial · ❌ absent. Action: **Keep**, **Evolve** or **Replace**.

| Requirement (from 02/03) | Alice today | Fit | Action |
| --- | --- | --- | --- |
| Rust core, Tauri 2, React | Rust 2024, Tauri 2.12, React 19.2, Vite 7 | ✅ | Keep |
| Daemon shared by UI, CLI and MCP | axum on `127.0.0.1:4815`, bearer token, data-dir lock, build-hash handshake with auto-restart | ✅ | Keep and harden (named pipe/UDS + ACL; the token file has no Windows ACL hardening) |
| Push updates to UI | Polling: `/v1/state` every 2 s (each poll scans all rows); desktop watcher every 5 s | ❌ | Replace with a change feed |
| IMAP sync | `imap` 2.4 (blocking, unmaintained) + vendored `imap-proto` 0.10.2 + native-tls; CONDSTORE ✅; QRESYNC ❌; **IDLE ❌**; MOVE required (no fallback); a new TLS login per user action; sync window plus background backfill | 🟡 | Replace stack (async IMAP + rustls, per-account connection actor); keep the sync-window logic |
| SMTP | lettre 0.11 (blocking, native-tls), XOAUTH2 | 🟡 | Evolve (rustls; IMAP APPEND to Sent for hosts that don't keep a copy) |
| Gmail API / Microsoft Graph mail | None for mail (used only for contacts and calendar) | ❌ | Add adapters behind a new provider trait (no trait exists today) |
| OAuth | Hand-rolled PKCE for Google and Microsoft, loopback callback, refresh | ✅ | Keep (review) |
| Credential storage | `keyring` 3 (Keychain / Credential Manager / Secret Service). The whole secret JSON is stored per account [V]; it may exceed the Windows ~2.5 KB blob limit with Microsoft tokens [I] | 🟡 | Evolve (split secrets) |
| Storage schema | rusqlite with one mutex; `messages(id, data JSON)` blobs plus a `message_index`; ad-hoc idempotent migrations | 🟡 | Replace with a normalised schema, a migration framework and a read pool (also on Alice's roadmap) |
| Threading | Subject-key grouping per account, computed per request; no References | ❌ | Replace (JWZ + provider thread IDs) |
| Search | Good Gmail-like query language with typo tolerance; FTS5 for bodies; header matching by full scan; no BM25 or embeddings | 🟡 | Keep the parser; evolve the engine |
| Compose | `Compose{to, subject, body, in_reply_to}`, **plain text only**, no cc/bcc/from display name, **no attachment upload** | ❌ | Replace model + editor (HTML, attachments, identities) |
| Draft autosave | localStorage backup + 400 ms debounced save to the daemon; interrupted drafts reopen | ✅ | Keep the pattern |
| Send safety | **At-most-once delivery claim**, frozen draft, `unknown` blocks retries, Sent reconciliation by Message-ID | ✅ | Keep (extend into the outbox) |
| Undo / undo send | None | ❌ | Build (intent engine + outbox delay) |
| Snooze / send later / reminders | None (the roadmap proposes a `jobs` table and scheduler) | ❌ | Build |
| Offline mutation queue | None; actions require connectivity | ❌ | Build (`operations` table) |
| Smart Inbox / categories | `category` is hard-coded `"personal"`; "Collections" are placeholders | ❌ | Build (classifier, sender policies, sections) |
| Done / Pin / Set aside / Seen | Star, read/unread and archive only | ❌ | Build |
| New-sender screening | None (on the roadmap) | ❌ | Build |
| Desktop UI | Monolithic `App.tsx`, `useState` only, no router, **no list virtualisation**, 50-per-page paging; good shortcut registry, context menus, three layouts, motion with reduced-motion support | 🟡 | Largely rewrite the shell; keep the shortcut registry, menus and i18n |
| HTML reader | DOMPurify + `sandbox` iframe without scripts + per-frame CSP; remote images opt-in; `cid:` inlining; quote folding | ✅ | Keep |
| Notifications / tray | Native notifications (macOS UN, notify-rust), tray, close-to-tray | ✅ | Keep |
| i18n | Typed catalogs for en-US and de in both UI and Rust, plus a consistency linter | ✅ | Keep |
| MCP server | Hand-rolled JSON-RPC over stdio; read tools plus update/draft/send | 🟡 | Evolve |
| Agent access control | **One global level** (Off / ReadOnly / Full), **checked only inside the MCP process**; the daemon never checks it, so any holder of the token has full access | ❌ | Replace (per-account levels enforced in the daemon) |
| Agent holds, audit, undo | None; `send_draft` sends immediately | ❌ | Build |
| BYO AI | OpenAI, Anthropic and compatible endpoints; key per origin; strict output validation; actions proposed and reviewed by the user | ✅ | Keep |
| Contacts, invitations (iTIP), calendar export | Recipient ranking, Google People/Graph/CardDAV import; robust iCalendar parsing and RSVP | ✅ | Keep (bonus) |
| Verification-code (OTP) copy | Careful local detector and protected clipboard | ✅ | Keep (bonus) |
| Windows | Builds; runtime never exercised | 🟡 | Verify in Phase 0 |

Tally across the 31 rows: 11 ✅, 8 🟡, 12 ❌. The ❌ rows are almost exactly our differentiating features.

## 4. Architecture compatibility notes

1. **Daemon-first design is compatible** with our plan (03 §3). The client/daemon split, build handshake and lock are good foundations to keep.
2. **Security boundary needs work before agents get more power:**
   - Move MCP/agent authorisation into the daemon, per account.
   - Give agents a distinct credential or scope from the desktop bridge.
   - Restrict the Tauri `daemon_request` passthrough, which currently lets the WebView reach any `/v1/*` route, including shutdown and the MCP preference.
   - ACL-protect the token or pipe on Windows.
3. **Error semantics.** HTTP status codes are derived by substring-matching error text. Replace this with typed errors before building more clients on the API.
4. **Single SQLite mutex and JSON blobs** will not scale to Smart Inbox counts and materialised sections over 100k messages. Alice's own roadmap reaches the same conclusion.
5. **No provider trait.** The data model is IMAP-shaped (`remote_mailbox`, `remote_uid`, `uid_validity`). Introduce a provider abstraction before adding Gmail API or Graph.

## 5. What to keep versus replace (component level)

| Keep (high value, low coupling) | Evolve | Replace early |
| --- | --- | --- |
| Daemon lifecycle (lock, token, build handshake, auto-spawn, Windows spawn without handle inheritance) | Local API (typed errors, batch endpoint, change feed) | IMAP/SMTP stack (`imap` 2.4 + vendored proto + native-tls) |
| Delivery-claim / at-most-once send | Search engine (BM25 ranking, indexed header columns) | Storage schema + migrations |
| Search query language (`search.rs`) | MCP server (daemon-side policy, more tools) | Threading |
| iCalendar / iTIP (`ical.rs`), DAV client, contact ranking | Credential layout (split secrets) | Compose model and editor |
| HTML sanitiser and sandboxed reader (`emailHtml.ts`) | Tauri bridge (narrow allow-list) | UI shell (`App.tsx` monolith → routed, virtualised, state-managed) |
| OAuth PKCE flows, keyring vault wrapper | Sync engine scheduling (keep window/backfill ideas) | Agent authorisation |
| i18n tooling and catalogs, CI matrix, mail-protocol fixtures | | |
| OTP detector, notifications, tray, clipboard privacy | | |

Estimate [I], by module line counts:

- **Largely unchanged after Phase 1 (about half of the Rust, ~12k of 23k lines):** OAuth, vault, AI, OTP, contacts, calendar/DAV/iCal, i18n, the daemon client, search parser, TUI and Tauri shell modules.
- **Replaced (~7k lines):** store, sync store, sync, remote, providers and MIME.
- **Evolved:** daemon, model, CLI and MCP.

Only a small share of the TSX would survive. That is still a meaningful head start on the unglamorous parts: OAuth, the vault, the daemon lifecycle, iCalendar, CI and i18n.

## 6. Licence analysis [V licence text; implications are not legal advice]

Alice is **AGPL-3.0-only**, copyright "Alice contributors". Its direct and transitive dependencies are permissive or weak-copyleft (MIT, Apache-2.0, BSD, ISC, MPL-2.0 file-level, OFL fonts). No GPL/AGPL dependencies were spotted, so the dependencies impose no extra constraint.

- **Forking or reusing code** makes the combined work a derivative, distributable only under **AGPL-3.0-only**. "Only" means no automatic upgrade to later versions. Relicensing to MIT or Apache, or dual-licensing commercially, would need consent from every copyright holder.
- **Source obligations.** Desktop binaries must ship with or offer the corresponding source. The §13 network clause matters if we ever host a service users interact with, such as a web client or relay. A self-hostable relay would be AGPL too, which is consistent with an open-source stance.
- **Mobile.** App-store distribution of AGPL code is possible but has historically been friction-prone. Note this for the planned mobile phase.
- **Clean-room alternative.** If a permissive licence matters, contributors should avoid copying Alice code. Reading it for ideas is normal practice, but keep a clear separation and document independent implementations.

## 7. Options

| Option | Description | Pros | Cons |
| --- | --- | --- | --- |
| **A. Fork and evolve** (recommended if AGPL is OK) | Fork at a pinned commit, rebrand, and replace layers progressively (strangler pattern); keep module boundaries close to upstream so fixes can flow both ways | Fastest path to a working Tauri/Rust app on all three OSes; inherits OAuth, vault, iCal, CI, i18n; aligned roadmap | AGPL lock-in; early, AI-generated code needs careful review; large rewrites diverge from upstream quickly |
| **B. Upstream-first** | Contribute snooze, the scheduler, the operations queue and the classifier to Alice itself | Shared maintenance; avoids a fork | One maintainer, no contribution process yet; our priorities (agent safety, Smart Inbox UX) may not match theirs; slower |
| **C. Fresh start, Alice as reference** | New workspace with our own schema, provider trait, change feed and UI; choose the licence freely; optionally borrow permissively licensed crates (async-imap, mail-parser/mail-builder, lettre) | Licence freedom; designed for our feature set from day one; no inherited shortcuts | More up-front work (OAuth, vault, packaging, CI); we would rebuild some things Alice already has |

**Suggested decision rule:** pick **A** unless a permissive licence is a hard requirement, in which case pick **C**. Revisit B after 4–6 weeks of watching upstream activity.

## 8. Follow-up actions

Under the chosen Option C (ADR-0003):

1. Optionally, the project owner may ask Alice's maintainer about relicensing or collaboration. Any agreement must be in writing before a new ADR revisits code reuse.
2. Restate Alice's useful ideas in our own specs (ADR-0003 §2) and never give coding agents access to the Alice repository (ADR-0001).
3. Reuse Alice's documented lessons as facts. For example: blocking IMAP crates, Windows Credential Manager blob-size limits, the need for References threading and a change feed.

### Kept for reference: Phase-0 due diligence had we chosen Option A

1. **Pin and track upstream.** Pin a commit and record it in an ADR. Watch upstream for a few weeks: commit cadence, maintainer responsiveness to an issue we open.
2. **Native Windows run on this machine:** installer, first-run setup, Credential Manager write/read with Google and Microsoft OAuth tokens (check the blob size), tray, notifications, daemon auto-start, and the TUI in Windows Terminal.
3. **Threat-model the local API:**
   - token-file ACL
   - WebView passthrough
   - MCP enforcement moved to the daemon
   - prompt-injection tests through `search_mail`/`read_message`
4. **Run the existing test suites** (cargo test, the smoke test and the mail-protocol fixture in Docker) to establish a baseline.
5. **Write ADRs** for the IMAP stack replacement, the schema, the change feed and the provider trait before touching features.
6. **Confirm the licence decision in writing,** and add CONTRIBUTING and a DCO or CLA policy for our fork.
