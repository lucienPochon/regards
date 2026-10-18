# Contributing to Regards

Thank you for helping. Please read this page before opening a pull request.

## 1. Clean-room rules (mandatory)

Regards is an independent project. Its workflows are *inspired by* other email clients, but it contains none of their code ([ADR-0001](docs/adr/0001-clean-room-policy.md)).

- **Never copy** code, decompiled logic, assets, icons, sounds, CSS, UI text, translation files or identifier names from **Spark** (Spark Mail Limited / Readdle) or from **Alice** (`get-alice/alice`, AGPL-3.0), or from any other incompatibly licensed project. This includes close paraphrase and line-by-line translation.
- **Work from specifications:** our research documents, ADRs, open standards (RFCs) and provider documentation.
- **AI coding agents** working on this repository must not be given access to Spark's installation files or to the Alice source code.
- **Don't use other products' trademarks** in the product, UI or marketing.

Pull requests that break these rules are closed.

## 2. Sign your commits (DCO)

Every commit needs a `Signed-off-by:` line, certifying the [Developer Certificate of Origin](https://developercertificate.org/):

```sh
git commit -s -m "Describe the change"
```

By contributing, you agree that your contribution is licensed under **Apache-2.0 OR MIT**, the licence of this repository. Server components added later are AGPL-3.0-only and live in separate directories ([ADR-0004](docs/adr/0004-licensing-and-trademark.md)).

## 3. Code standards

- `cargo fmt --all` (120 columns, see `rustfmt.toml`)
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`; new behaviour comes with tests
- `cargo deny check` (licences, advisories, sources); new dependencies must be permissively licensed (`deny.toml`)
- `unsafe` is denied workspace-wide. Platform code that needs it (for example Windows security descriptors) allows it per module, with a `SAFETY:` comment on every block.
- Windows, macOS and Linux are all first-class platforms ([ADR-0013](docs/adr/0013-cross-platform-support.md)); CI must pass on all three.

## 4. Decisions

Significant design changes need an Architecture Decision Record in [`docs/adr/`](docs/adr/README.md). Copy the template in the index and open it as `Proposed`.

## 5. Privacy

Regards collects no data and ships no third-party analytics ([ADR-0015](docs/adr/0015-privacy-telemetry-and-ai.md)). Never log message bodies, addresses or secrets.
