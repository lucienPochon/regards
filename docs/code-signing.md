# Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io), certificate by [SignPath Foundation](https://signpath.org).

**Status:** Regards has applied to the SignPath Foundation programme. Until it is accepted, the Windows installers are unsigned, and Windows SmartScreen warns when you start one.

## What is signed

On Windows:
- the installer, `Regards_<version>_x64-setup.exe`;
- the programs it installs: the app (`regards-desktop.exe`) and the `regards` program (the daemon, the CLI and the MCP server).

They are signed only when they come from this repository:
- **Built by the [Release workflow](../.github/workflows/release.yml)**, on GitHub-hosted runners, from a version tag (`v0.3.0` …). SignPath checks that the files it signs come from that workflow.
- **Every signing request is approved by hand** by an approver (below). Nothing built on a personal computer is signed.

The macOS and Linux builds are not signed (see [releasing](releasing.md)).

**Checking a download:** right-click the installer › *Properties* › *Digital Signatures*. The signer is *SignPath Foundation*.

## Team roles

| Role | Who |
| --- | --- |
| Committers and reviewers | The maintainer, [lucienPochon](https://github.com/lucienPochon). Changes from anyone else are reviewed by the maintainer before they are merged ([CONTRIBUTING](../CONTRIBUTING.md)) |
| Approvers | The maintainer, [lucienPochon](https://github.com/lucienPochon). Approves each signing request in SignPath |

Everyone with write access to this repository or to the SignPath project uses multi-factor authentication.

## Privacy

This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.

What it connects to, and when, is listed in the [privacy statement](privacy.md).

## What the installers contain

- **Regards' own code**, from this repository.
- **Open-source components**, built from source by the same workflow: Rust crates and npm packages. The app lists each one, with its licence, under *Support › Licences*.
- **Windows' own web view (WebView2)** is not included: the app uses the copy that Windows provides. On a computer without it, the installer downloads Microsoft's installer for it.
