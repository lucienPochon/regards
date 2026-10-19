# Regards

Regards is a local-first, open-source email client for **Windows, macOS and Linux**. It offers a calm, sectioned inbox, done / pin / set aside, snooze, reminders, send later, undo everywhere, and a safe interface for AI agents. It is built with a Rust core, Tauri 2 and React.

> **Status: pre-alpha (Phase 1, sync).** You can read and organise mail, but not write it yet. What works today:
> - **Accounts:** IMAP with a password (sign-in checked, password in the OS credential store). OAuth comes later.
> - **Sync:** folders and messages, newest first and then older mail in the background. New mail arrives by push (IMAP IDLE) in under a second against Dovecot, and QRESYNC is used where the server has it.
> - **Reading:** HTML mail in a sandboxed frame, with remote images blocked until you allow them; links open in your browser.
> - **Conversations** across folders.
> - **Actions:** read/unread, flag, archive, delete, move and delete for good. They show at once, work offline, and moves can be undone.
> - **Keyboard shortcuts:** press `?` in the app.
>
> Not yet: writing and sending, search, the smart inbox, snooze and send later. See the [plan](docs/research/03-independent-implementation-plan.md).

## How it is built

- **One background service per user (the daemon)** owns all mail data. The desktop app, the `regards` CLI and the MCP server for agents all talk to it ([ADR-0005](docs/adr/0005-process-model.md)).
- **The local API** is JSON-RPC over a per-user Windows named pipe (current user only) or a Unix socket (`0600`). There is no TCP port ([ADR-0006](docs/adr/0006-local-ipc-and-security.md)).
- **Storage:**
  - **Mail data:** SQLite (`mail.db`) in the data directory, migrated automatically, with a backup before every migration. The directory is private to the user.
  - **Passwords:** the OS credential store (Credential Manager, Keychain or Secret Service). They are never written to the database or the logs ([ADR-0008](docs/adr/0008-storage-and-search.md)).
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

Tests against a real IMAP server are opt-in. They run in CI against Dovecot in Docker; to run them locally, start the container from [`spikes/imap-trial`](spikes/imap-trial/README.md), then:

```sh
REGARDS_TEST_IMAP=localhost:31993 REGARDS_TEST_IMAP_CA=/path/to/ca.pem \
  cargo test -p regards-imap -p regards-daemon -- --ignored live_
```

Desktop app (development mode with hot reload):

```sh
cd apps/desktop
npm install
npm run tauri dev
```

`tauri dev` builds the `regards` CLI first. The app starts the daemon from the `regards` program next to its own executable, which is also where installers will put it. Set `REGARDS_DAEMON_BIN` to use a different program.

To work on the interface without a mail server, run `npm run dev` and open <http://localhost:1420/preview.html> in a browser. It shows the app with made-up sample mail, and the actions work on that sample. Add `?accounts=2` for two accounts, `?accounts=0` for the first-run screen, `?theme=dark`, `?layout=single` to open messages in place of the list, `?sidebar=icons` for the folded sidebar, or `?screen=off` or `?screen=hold` for how new senders are shown. The preview is not part of the app build.

Actions in the app change your real mailbox, and "Delete forever" is permanent. To try things safely, use the Dovecot container from [`spikes/imap-trial`](spikes/imap-trial/README.md): start the app with `REGARDS_DEV_EXTRA_CA=/path/to/ca.pem` (development builds only), then add an account on `localhost`, port `31993`, with any user name and the password `pass`.

## Try the daemon

```sh
regards daemon start      # start it in the background (no-op if it is running)
regards daemon run        # or run it in the foreground (Ctrl+C to stop)
regards daemon status     # add --json for machine output
regards daemon stop
regards version
```

A daemon started in the background logs to `logs/daemon.log` in the data directory.

### Add a mail account

Add accounts in the desktop app.
- **The check:** Regards first signs in to the IMAP server (TLS, usually port 993) to check the details. Only then does it save the account.
- **The password:** it goes to the OS credential store.

The CLI cannot add accounts: only the app may change accounts and credentials ([ADR-0006](docs/adr/0006-local-ipc-and-security.md)).

**Testing against a local server:** to try it against a local test server with its own CA, for example the Dovecot container from [`spikes/imap-trial`](spikes/imap-trial/README.md), point a **debug** build at the CA:

```sh
REGARDS_DEV_EXTRA_CA=/path/to/ca.pem REGARDS_DATA_DIR=/tmp/regards-dev npm run tauri dev
```

Release builds ignore `REGARDS_DEV_EXTRA_CA`.

**The desktop app owns the daemon it uses.** Only a daemon the app started itself can give it desktop access ([ADR-0006](docs/adr/0006-local-ipc-and-security.md)). When the app finds a daemon started some other way, for example by `regards daemon run`, it replaces it with its own. When the app quits, it stops its daemon.

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
