# ADR-0002: Technology stack: Rust core, Tauri 2 shell, React + TypeScript UI

- **Status:** Accepted
- **Date:** 2026-10-18
- **Deciders:** Project owner

## Context

- **Requirement:** a desktop email client on **Windows, macOS and Linux** (ADR-0013), with mobile possible later.
- **What Spark does:** Spark Desktop is an Electron 42 + React 18 UI over a Swift core shared with Spark's Apple apps, compiled for Windows. The Windows install is several hundred MB, and the main executable alone is 232 MB (01 §3–5 (private research notes)).
- **What we keep from Spark:** the useful part is the *shape* — one shared native core holding all logic and state, a thin web-technology UI, and a local CLI for agents.
- **Our situation:** we have no legacy Swift code, so Spark's reason for that stack doesn't apply to us.

## Decision

| Layer | Choice |
| --- | --- |
| Core (sync, storage, rules, scheduler, agent gateway) | **Rust** (edition 2024, stable toolchain), async on **tokio** |
| Desktop shell | **Tauri 2** (stable 2.x line; not 3.0 alphas) |
| UI | **React** + **TypeScript** (strict), built with Vite |
| CLI / TUI | Rust (`clap`; a TUI is optional, later) |
| Mobile (later) | Tauri 2 mobile targets, or native apps binding the Rust core (for example via UniFFI). Decided in a future ADR |

Rules:
- **Native code stays in Rust.** No C/C++ or Swift in our own code; native OS APIs are reached through Rust crates.
- **Licences.** Dependencies must be permissively licensed (MIT, Apache-2.0, BSD, ISC, Zlib, MPL-2.0 file-level). A `cargo-deny` licence check enforces this in CI.

## Alternatives considered

| Option | Pros | Cons | Verdict |
| --- | --- | --- | --- |
| **Electron + Swift core (Spark's stack)** | Proven by Spark; identical Chromium on all OSes | Swift outside Apple platforms has thin libraries and tooling; Electron is heavy (hundreds of MB, high RAM); two runtimes | Rejected |
| **Electron + Rust core (N-API)** | Consistent Chromium rendering; Rust core | Install size and memory; a large security-hardening surface (fuses, Node in the main process) | Rejected; kept as a fallback if WebKitGTK proves unworkable |
| **Tauri 2 + Rust + React** | Small binaries; Rust end-to-end; capability-based security; mobile path; permissive licence | Uses each OS's own webview (WebView2 / WKWebView / WebKitGTK), so rendering differs and **WebKitGTK on Linux is the weakest** | **Chosen** |
| Flutter / Dart | One UI toolkit everywhere | Rendering HTML email is awkward; weaker desktop mail ecosystem | Rejected |
| Native UI per OS | Best platform feel | Three UIs to build and maintain | Rejected |

## Consequences

- **Rendering tests.** Arbitrary HTML email renders through three different engines. The HTML reader gets a cross-engine visual regression suite from Phase 0, and Linux testing must cover several distros' WebKitGTK versions.
- **Shared Rust core.** The core is a pure Rust library crate, usable by the daemon, CLI, tests and future mobile apps, with no UI dependencies.
- **Rust expertise** becomes the critical hiring and skills requirement.
- **Escape hatch.** Because the core sits behind a local API (ADR-0005/0006), the shell could be swapped to Electron later without rewriting the core.

## References

- 01 Technical investigation (private research notes)
- [Tauri 2](https://tauri.app/) (MIT OR Apache-2.0)
