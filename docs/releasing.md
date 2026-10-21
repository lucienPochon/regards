# Releasing Regards

For maintainers. A release is a set of installers on a GitHub release, built by the [Release workflow](../.github/workflows/release.yml) from a version tag.

## What a bundle holds

Each installer contains:
- **The app** (`regards-desktop`, the Tauri shell around the React UI);
- **The `regards` program:** the daemon, the CLI and the MCP server.

The app starts the daemon from the `regards` next to its own executable (`regards_ipc::launcher::sibling_program`). Tauri puts it there as a *sidecar*:
- `npm run release` builds with [`tauri.release.json`](../apps/desktop/src-tauri/tauri.release.json) over the usual configuration.
- [`scripts/sidecar.mjs`](../apps/desktop/scripts/sidecar.mjs) first builds `regards` for the target, and places it where Tauri expects it: `src-tauri/binaries/regards-<target triple>`, which is git-ignored.

- [`scripts/licences.mjs`](../apps/desktop/scripts/licences.mjs) writes the third-party licence list that Support › Licences shows. It runs [cargo-about](https://github.com/EmbarkStudios/cargo-about) with the root [`about.toml`](../about.toml), and puts the result in `public/licences/rust.json`, which is git-ignored. A release needs cargo-about (`cargo install cargo-about --version =0.9.2 --locked --features cli`), and the workflow installs it.
- [`scripts/licences-npm.mjs`](../apps/desktop/scripts/licences-npm.mjs) does the same for the web packages the window ships (the production npm dependencies, as installed), into `public/licences/npm.json`, also git-ignored. It needs nothing beyond npm.

Development builds don't need any of this: Cargo builds `regards` into the same folder as the app, and without cargo-about the licence list is empty.

## Cutting a release

1. **Set the version** in three places, all the same:
   - `version` in the root `Cargo.toml`;
   - `apps/desktop/package.json`;
   - `apps/desktop/src-tauri/tauri.conf.json`.

   Commit through a pull request, as usual.
2. **Tag the merged commit** `v<version>`, for example `git tag v0.1.0 && git push origin v0.1.0`. The workflow then:
   - checks that the tag matches the version;
   - creates a **draft** release;
   - builds:
     - **Windows x64:** the NSIS installer. Its [hooks](../apps/desktop/src-tauri/windows/hooks.nsh) run `regards daemon stop` before files are replaced or removed, because in background mode the daemon keeps running, and Windows won't replace a running program. An MSI would need the same, so none is built;
     - **macOS:** Apple silicon and Intel, as DMG;
     - **Linux x64:** deb, rpm and AppImage, built on Ubuntu 22.04 so they run on more systems;
   - uploads the bundles to the draft.

   It can also be started by hand (*Run workflow*) for an existing tag.
3. **Try the installers**, at least on one system, then publish the draft. Nothing is published before that.

To build one bundle locally: `cd apps/desktop && npm run release`, optionally with `-- --bundles nsis` (or `dmg`, `deb` …) for a single format.

## Not yet

- **Signing:**
  - Windows: SignPath, free for open source.
  - macOS: Developer ID and notarisation, when funded.

  Until then, Windows SmartScreen and macOS Gatekeeper warn on first start. Each needs its account and secrets (plan, Phase 0), then a signing step in the workflow.
- **Installing updates by itself:** this needs a signing key pair for the update files, kept by the owner, and Tauri's updater.

## Update checks

What exists today (`apps/desktop/src-tauri/src/updates.rs`):
- **Asked once:** the app asks the person whether to check for updates. It doesn't check until they say yes, because a check tells GitHub the computer's address ([privacy](privacy.md)).
- **When on:** it asks GitHub's API for the latest release once a day.
- **When there is a later version:** a later release (not a draft or pre-release, with a page in this repository) is told once, by a notification and the `update_available` event. The person opens its page and downloads the installer.

Publishing a release (above) is all it takes for installed copies to learn of it. A fork's build can point elsewhere with `REGARDS_RELEASES_API` and `REGARDS_RELEASE_PAGES` at build time.
