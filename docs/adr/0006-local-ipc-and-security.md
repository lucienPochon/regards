# ADR-0006: Local IPC transport and security boundary

- **Status:** Accepted
- **Date:** 2026-10-18
- **Deciders:** Project owner

## Context

The daemon (ADR-0005) must be reachable by the desktop app, the CLI and the MCP server, and by nothing else.

- **Spark:** uses a per-user Windows named pipe with JSON-RPC and protocol-version negotiation for its CLI.
- **Alice:** uses loopback TCP with a bearer token kept in a file. That file has no ACL hardening on Windows, and its Tauri bridge passes any `/v1/*` path through from the WebView.

Loopback TCP is reachable by every local user and, through DNS-rebinding mistakes, potentially by web pages.

## Decision

1. **Transport:**
   - **Windows:** a named pipe `\\.\pipe\regards-<hash of user SID>`.
     - Its security descriptor grants access **only to the current user's SID**.
     - It is created with `FILE_FLAG_FIRST_PIPE_INSTANCE` to prevent squatting.
     - On connect, the daemon checks the client process's user with `GetNamedPipeClientProcessId` and the process token.
   - **macOS/Linux:** a Unix domain socket.
     - Location: `$XDG_RUNTIME_DIR/regards/daemon.sock` (Linux) or the per-user Application Support `run/` directory (macOS).
     - The directory is mode `0700` and the socket `0600`.
     - Peer UID is checked with `SO_PEERCRED` / `getpeereid`.
   - **No TCP listener in release builds.** A loopback TCP listener exists only behind a `--dev-tcp` flag, for the browser-based UI dev server, and is never enabled in packaged builds.
2. **Protocol:**
   - **JSON-RPC 2.0** over length-prefixed frames.
   - The first message is `hello {protocol_versions[], build_id, client_kind}`, and the server answers with the negotiated version.
   - Server-to-client **notifications** carry the change feed (ADR-0007).
   - Errors are **typed** with stable codes. HTTP-style status guessing from text is not used.
   - The schema is generated from Rust types, so the TypeScript and CLI JSON contracts never drift.
3. **Principals.** Every connection is labelled by its principal:
   - `ui`: the desktop bridge only. It holds a per-launch secret passed from the daemon to the desktop process through an inherited handle or stdin, never written to disk when the desktop starts the daemon.
   - `agent`: all CLI and MCP connections, which are subject to per-account access levels (ADR-0012).
   - **Changing access levels, accounts or credentials is allowed only for the `ui` principal.**
4. **Tauri hardening:**
   - The WebView gets a **narrow, explicit command set**: no generic passthrough to the daemon, no shutdown or settings-of-agents from JS without a UI gesture.
   - Minimal Tauri capabilities; strict CSP.
   - The email HTML reader runs in a sandboxed iframe without `allow-scripts`, with its own CSP and remote content blocked until the user opts in.
5. **Deep links:** a custom URL scheme (`regards://thread/<id>`, `…/draft/<id>`) is registered by the desktop app. Handlers only navigate; they never perform actions.

## Threat model and limits

- **Protects against:** other OS users, remote and web origins, and accidental or prompt-injected misuse by agents, which are limited by access levels, holds and audit.
- **Does not protect against:** malicious software running **as the same user**, which can read the user's files and memory anyway, just as with Spark or any desktop mail client. Documentation says so explicitly, and recommends running coding agents in their own sandboxes, without read access to the app's data directory.

## Alternatives considered

- **Loopback HTTP + bearer token file (Alice).** Simple, but reachable by other local users and DNS-rebinding prone, and the token-file ACL is weak on Windows. Rejected, except as a dev-only option.
- **gRPC over TCP.** Heavier, and has the same exposure. Rejected.
- **No principal separation.** Would let any agent raise its own access level. Rejected.

## Consequences

- Platform-specific transport code (named pipes and Unix sockets) behind one async transport trait. Tokio provides both primitives.
- Browser-only UI development needs the dev TCP flag. CI checks that release builds have it compiled out.
- A security review of this boundary is a Phase-5 exit criterion.

## References

- 01 §6, §10 (private research notes); [04 §4](../research/04-alice-compatibility-assessment.md)
