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
- Added a `commercial` permission profile and `kiana doctor`
  `commercial_security` readiness report for strict sandbox plus approval
  posture validation.
- `kiana doctor --json` now emits the stable `kiana.doctor.v1` report shape,
  and release gates validate the schema-critical fields.
- OAuth token files can now supply remote/bridge bearer tokens, refresh through
  the stored refresh token, and surface redacted status in `kiana auth status`
  and `kiana doctor`.
- OAuth token files with `expires_at` are refreshed before remote-session and
  bridge live calls when they are already expired or near expiry.
- OAuth token persistence now uses same-directory atomic replacement and
  owner-only permissions where the platform exposes them.
- Remote code-session create/bridge/smoke/hydrate commands retry once after an
  authentication failure by using the configured remote token refresh path.
- Code-session auth retry now uses typed HTTP status from remote API errors,
  with string matching retained only as a compatibility fallback.
- Tag release workflows now enforce `v$(VERSION)` tag alignment, derive
  manifest URLs from the active GitHub repository/tag, run full release
  preflight before packaging, and recursively attach manifest/compliance
  artifacts to draft GitHub Releases.
- `kiana auth status --json` now reports OAuth token-file status, expiry,
  expiring/expired state, refreshability, and storage source without exposing
  token material; the service OAuth token check now rejects expired token files.
- Startup now records a first-start onboarding sentinel under `KIANA_HOME`, and
  the TUI surfaces a one-time onboarding message when no API key or usable OAuth
  token is configured.
- TUI `/diff --json` and `/diff --last-assistant --json` now render structured
  diff previews with file lists, stats, and assistant patch snippets instead of
  raw JSON transcript output.
- TUI permission requests now maintain structured approval panel state with
  tool, reason, blocked path, input preview, suggestions, and queue context
  while preserving `/allow` and `/deny` command handling.
- Added an OpenAI-compatible text provider for chat/completions endpoints with
  explicit model capabilities and pre-request rejection when tools are enabled.
- Text-only providers now default to an empty tool set when the user has not
  explicitly configured tools, while explicit tool requests still fail fast.
- Release tarballs now include a package lifecycle smoke script, and the
  packaged binary installer supports uninstalling from the selected
  `INSTALL_DIR`.
- Added a local Ollama text provider for `/api/chat` endpoints with explicit
  model capabilities and pre-request rejection when tools are enabled.
- Added headless TUI render coverage for the REPL approval panel and resume
  picker search view.
- Added `kiana model smoke [--json] [--live]` for provider smoke reports, with
  fake provider covered by default release gates and live providers opt-in.

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
- Full OAuth browser/device login, secret-store hardening, and proactive refresh
  scheduling remain future work beyond the local token-file refresh base.
- Reference-level TUI, approval, sandbox, enterprise account, and provider
  ecosystem parity remain product work before broad commercial availability.
