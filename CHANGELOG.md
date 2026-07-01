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
- `kiana doctor --json` commercial security output now includes `platform`,
  `isolation`, and `controls`, with Linux retaining strict `bwrap` sandbox
  readiness and Windows/macOS using the approval plus shell exec-policy model.
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
- Commercial release workflows now reject manual signing confirmation shortcuts:
  `scripts/verify-commercial-release-artifacts.sh` checks combined release
  artifacts for signatures, macOS notarization proof, publishable Windows
  packaging, and non-blocked channel manifests before a draft GitHub Release is
  created.
- `stream-json` init events now expose visible skill and plugin capability
  summaries instead of empty placeholder arrays; plugin skills and plugin slash
  commands disappear from the visible capability set when the plugin is
  disabled.
- Added commercial live smoke wrappers for provider and remote evidence:
  `scripts/provider-live-smoke.sh --required` stores live catalog/text/tool
  proof JSON, `scripts/remote-live-smoke.sh --required` stores CCR v2 code
  session proof JSON, and full release preflight now runs both gates.
- Exec policy now rejects destructive sync-to-root shapes such as
  `rsync --delete ... /` and `robocopy ... C:\ /MIR`, including nested shell
  wrapper invocations, while preserving safe literal examples.
- Added `scripts/product-shell-smoke.sh` and wired it into release smoke and
  release packaging so TUI approval, diff, onboarding, resume, and prompt
  history headless coverage is a named product-shell gate.
- Added a TUI `/settings` readiness hub that surfaces account/auth, provider
  model capability, permissions, MCP, remote, and diagnostics status from the
  existing command registry.
- `kiana model smoke` now supports opt-in tool-call smoke through `--tools` or
  `KIANA_PROVIDER_SMOKE_TOOLS=1`; the default no-network gate remains text-only,
  while `--live --tools` can verify live provider tool-call behavior.
- Added `kiana model catalog [--json] [--live]` with a pinned
  `kiana.model-catalog.v1` report; default release gates stay offline, while
  `--live` refreshes OpenAI-compatible `/models` and Ollama `/api/tags` against
  configured endpoints.
- Added a pinned `kiana.enterprise.offline-manifest.v1` schema and package
  lifecycle checks for enterprise offline manifest artifact/checksum coverage.
- `kiana auth status --json` now reports OAuth token-file status, expiry,
  expiring/expired state, refreshability, and storage source without exposing
  token material; the service OAuth token check now rejects expired token files.
- `kiana auth logout` now clears the configured API key and OAuth token file by
  default, with `--api-key-only` and `--oauth-only` selectors for managed
  environments.
- `kiana auth status --json` now includes provider-aware readiness for
  Anthropic, OpenAI-compatible, Ollama, and fake providers, including redacted
  API key previews and local endpoint/model settings without exposing secrets.
- Built-in provider registry metadata now centralizes provider display name,
  protocol, auth method, option aliases, env vars, default model/base URL,
  model source category, streaming mode, and live-smoke requirement for model
  listing, auth readiness, and runner provider construction.
- `kiana model list --json` now reports `streaming_mode` and
  `native_streaming`, distinguishing Anthropic native streaming from the
  synthetic stream-event fallback used by OpenAI-compatible, Ollama, and fake
  providers.
- OpenAI-compatible providers now advertise tool support and map Chat
  Completions `tools`/`tool_calls` to Kiana `tool_use`/`tool_result` loops.
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
- Local Ollama `/api/chat` provider now advertises tool support and maps
  Ollama `tools`/`tool_calls` responses into Kiana `tool_use`/`tool_result`
  loops.
- Added headless TUI render coverage for the REPL approval panel and resume
  picker search view.
- Added `kiana model smoke [--json] [--live]` for provider smoke reports, with
  fake provider covered by default release gates and live providers opt-in.
- Added a TUI `/history` picker backed by `KIANA_HOME/tui-history.jsonl`, with
  searchable prompt history, deduped newest-first persistence, draft restore,
  and headless render/state coverage.
- Added `kiana license status [--json|--text]` with a pinned
  `kiana.license-status.v1` readiness report for offline enterprise license,
  account, entitlement, support, and managed-policy inputs without exposing raw
  license keys.

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
