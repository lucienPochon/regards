# Regards

Regards is a local-first, open-source email client for **Windows, macOS and Linux**. It offers a calm, sectioned inbox, done / pin / set aside, snooze, reminders, send later, undo everywhere, and a safe interface for AI agents. It is built with a Rust core, Tauri 2 and React.

> **Status: pre-alpha (Phase 0, foundation).** No mail features yet. What exists today:
> - the background service (daemon) and its local API
> - the `regards` CLI
> - a minimal desktop window that shows the service's status
>
> See the [plan](docs/research/03-independent-implementation-plan.md).

## How it is built

- **One background service per user (the daemon)** owns all mail data. The desktop app, the `regards` CLI and the MCP server for agents all talk to it ([ADR-0005](docs/adr/0005-process-model.md)).
- **The local API** is JSON-RPC over a per-user Windows named pipe (current user only) or a Unix socket (`0600`). There is no TCP port ([ADR-0006](docs/adr/0006-local-ipc-and-security.md)).
- **Regards 1.0 is fully local:** no Regards servers and no telemetry. Sending while the computer is off arrives in 1.1 ([ADR-0019](docs/adr/0019-send-while-off-in-1-1.md)).

All architecture decisions are in [`docs/adr/`](docs/adr/README.md). Research is in [`docs/research/`](docs/research/README.md).

## Build from source

Requirements:

- [Rust](https://rustup.rs) (stable; the toolchain is pinned in `rust-toolchain.toml`)
- [Node.js](https://nodejs.org) 22 or newer (for the desktop UI)
- **Windows:** Visual Studio Build Tools with "Desktop development with C++" (the WebView2 runtime ships with Windows 10/11)
- **Linux:** `build-essential` plus the [Tauri system libraries](https://tauri.app/start/prerequisites/), for example on Debian/Ubuntu: `libwebkit2gtk-4.1-dev libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev`

```sh
cargo build --workspace
cargo test --workspace
```

Desktop app (development mode with hot reload):

```sh
cd apps/desktop
npm install
npm run tauri dev
```

## Try the daemon

```sh
regards daemon run        # run the daemon in the foreground (Ctrl+C to stop)
regards daemon status     # in another terminal; add --json for machine output
regards daemon stop
regards version
```

During development, point Regards at a throwaway data directory so it never touches real data:

```sh
REGARDS_DATA_DIR=/tmp/regards-dev regards daemon run      # macOS / Linux
$env:REGARDS_DATA_DIR="$env:TEMP\regards-dev"; regards daemon run   # Windows PowerShell
```

Set `REGARDS_LOG=debug` for verbose daemon logs.

`daemon status` exit codes: `0` running, `3` not running. `daemon run` exits with `4` when another daemon already owns the data directory.

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) first. It covers the clean-room rules, which every contribution must follow, and the `Signed-off-by` requirement (DCO).

## Licence

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT licence ([LICENSE-MIT](LICENSE-MIT))

at your option. Unless you explicitly state otherwise, any contribution you intentionally submit for inclusion in Regards, as defined in the Apache-2.0 licence, is dual licensed as above, without additional terms or conditions.
