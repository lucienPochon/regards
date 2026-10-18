# ADR-0013: Windows, macOS and Linux are all tier-1 from day one

- **Status:** Accepted
- **Date:** 2026-10-18
- **Deciders:** Project owner

## Context

The product must run on Windows, macOS and Linux. Alice has only been cross-compiled for Windows and has never run there ([04 §2](../research/04-alice-compatibility-assessment.md)). Tauri uses a different webview on each OS (ADR-0002).

## Decision

1. **Tier-1 means:** every commit is built and tested on all three OSes in CI, and no release ships unless all three pass the release checklist.
2. **Initial support matrix** (revisited yearly):

   | OS | Versions | Architectures | Webview |
   | --- | --- | --- | --- |
   | Windows | 10 22H2, 11 | x64, ARM64 | WebView2 (Evergreen) |
   | macOS | 13 Ventura and newer | Apple silicon + Intel (universal) | WKWebView |
   | Linux | Ubuntu 22.04/24.04 LTS, Debian 12+, Fedora (current) | x64 (ARM64 later) | WebKitGTK 4.1 |

3. **CI:** GitHub Actions matrix on Windows, macOS and Linux. Standard hosted runners are free for public repositories on all three. Every run includes lint, unit/integration tests, mail-protocol fixtures (Linux containers), packaging, and the HTML-reader **visual regression tests on all three webviews**.
4. **OS integration:**

   | Concern | Windows | macOS | Linux |
   | --- | --- | --- | --- |
   | Secrets | Credential Manager (secrets split per credential to respect blob-size limits) | Keychain | Secret Service (GNOME Keyring/KWallet); if absent, a clear error and setup guidance in v1 |
   | IPC | Named pipe + DACL | Unix socket | Unix socket |
   | Background / login start | Task Scheduler or Run key | LaunchAgent (`SMAppService`) | systemd user unit, or XDG autostart |
   | Notifications | Toast with AppUserModelID | UserNotifications (signed bundle required) | freedesktop notifications |
   | Tray | Notification area | Menu bar extra | StatusNotifierItem/AppIndicator; **optional**, since GNOME needs an extension |
   | `mailto:` and deep links | Registry protocol handlers | `CFBundleURLTypes` | `.desktop` MimeType / `x-scheme-handler` |
   | Packaging | NSIS or MSI, **code-signed** | DMG, **Developer ID signed + notarized** | AppImage, `.deb`, `.rpm` (Flatpak later) |
   | Updates | Tauri updater with signed manifests | Tauri updater | Distro packages / AppImage update; the Tauri updater where applicable |

5. **Portability rules:**
   - No platform-specific code outside dedicated `platform/` modules.
   - Paths go through `dirs`-style per-user directories.
   - Handle Windows long paths and case-insensitive filesystems.
   - Never shell out to OS tools for core features.
6. **Accessibility:** keyboard-complete UI, and screen-reader checks with NVDA (Windows), VoiceOver (macOS) and Orca (Linux) before 1.0.

## Alternatives considered

- **Windows-first, others later.** Cheaper at first, but porting late is expensive, and Linux webview issues would surface too late. Rejected.
- **Linux via Flatpak only.** Sandboxing complicates the IPC socket, the credential store and agent CLI access. Deferred; native packages first.

## Consequences

- Signing costs and accounts (Apple Developer ID, a Windows signing option) are needed before public beta ([06](../research/06-costs-and-business-model.md)).
- Linux WebKitGTK quirks are a known risk; the HTML reader and composer must be tested on the oldest supported WebKitGTK.
- Each OS needs a real-device smoke test per release, not just CI.

## References

- [Tauri prerequisites](https://tauri.app/start/prerequisites/); [ADR-0002](0002-technology-stack.md), [ADR-0006](0006-local-ipc-and-security.md)
