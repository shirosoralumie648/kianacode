# Changelog

All notable changes to Kiana Code are recorded here.

This project is still pre-1.0. Until a stable compatibility policy is published,
minor versions may include breaking CLI, config, or storage changes. Use
`UPGRADE.md` for migration steps before upgrading production installations.

## 0.1.0 - 2026-07-01

### Added

- Source-build release candidate gate through `scripts/release-smoke.sh`.
- Cross-platform artifact packaging through `scripts/package-release.sh`.
- Tag/manual GitHub Actions artifact workflow in `.github/workflows/release.yml`.
- SHA256 checksum generation and verification for release tarballs and binaries.
- Security, privacy, telemetry, upgrade, and commercial release readiness docs.
- Workspace package metadata for license, repository, Rust version, and authors.
- CycloneDX SBOM generation and local RC compliance audit artifacts.
- `deny.toml` and full release workflow steps for `cargo-audit` and
  `cargo-deny`.
- Local compliance tool bootstrap through `scripts/install-compliance-tools.sh`.
- Dependency hardening for the release candidate: removed `rmcp`, upgraded
  `reqwest`/`tokio-tungstenite`/TLS stack dependencies, and replaced direct
  `ansi_term` usage so `cargo audit` no longer reports vulnerability errors.
- Direct-connect reference parity: `cc+unix:///path/to/socket` open targets and
  `kiana server --unix <path>` now have Unix-domain-socket implementations on
  Unix platforms, with explicit unsupported-platform errors on Windows.
- Service-layer API key resolution now uses the same effective config stack as
  the CLI, including config files, overlays, managed policy, environment
  precedence, and empty-value filtering.
- Release smoke now enforces locked/offline Cargo test and release-build gates
  and preserves real CLI smoke exit status for expected-success commands.
- Distribution manifest dry-runs are generated from release package checksums,
  including an enterprise offline manifest and explicit package-channel blockers
  when an artifact is not yet publishable by a target channel.

### Verified

- `cargo fmt --all --check`.
- `cargo test --workspace --locked --offline --no-fail-fast`.
- `cargo build --release -p kiana-entrypoints --bin kiana`.
- Windows/Git Bash `scripts/release-smoke.sh`.
- Windows x86_64 tarball packaging and checksum verification.
- Tarball binary installer and SBOM/compliance artifact inclusion.

### Known Release Blockers

- The repository has a local initial commit, but still needs a real remote, tag
  policy, release credentials, and active CI before a public release can be cut.
- Signing, notarization, package-manager distribution, and retention policies are
  not complete.
- Live remote/CCR/Session Ingress/token refresh gates require production-like
  service credentials and are not part of default CI.
- Reference-level TUI, approval, sandbox, enterprise account, and provider
  ecosystem parity remain product work before broad commercial availability.
