# Kiana Commercial Release Readiness

Date: 2026-07-01

## Verified Local Release Candidate Gates

The local source-build release candidate now has a passing Windows/Git Bash gate:

```bash
CARGO_REGISTRIES_CRATES_IO_PROTOCOL=git \
CARGO_NET_GIT_FETCH_WITH_CLI=true \
./scripts/release-smoke.sh
```

The gate covers:

- `cargo fmt --all --check`
- `cargo test --workspace --locked --offline --no-fail-fast`
- `cargo build --release --locked --offline -p kiana-entrypoints --bin kiana`
- Release binary version and doctor smoke.
- Help, auth, completion, plugin marketplace, MCP config, and project MCP smoke.
- Temporary source install into an isolated `INSTALL_DIR`.
- Installed binary version, doctor, help, auth, plugin, and MCP smoke.

The package script also produced a local tarball and checksums in a temporary `DIST_DIR`:

```bash
DIST_DIR="$(mktemp -d)" ./scripts/package-release.sh
```

## Completed This Pass

- Windows release-smoke now resolves `kiana.exe` and preserves a usable Git Bash tool PATH for child Windows processes.
- Install smoke uses the actual Cargo and Rustup homes instead of assuming `HOME/.cargo`.
- Doctor warnings in a clean environment are verified by content instead of treated as hard install failures.
- Temporary install smoke runs offline after the main build gate, avoiding accidental registry access.
- Added `scripts/package-release.sh` for tar.gz plus SHA256 artifacts.
- Added `.github/workflows/release.yml` for tag/manual cross-platform artifact packaging, checksum verification, artifact upload, and draft GitHub Release creation.
- Added `SECURITY.md`, `PRIVACY.md`, and `TELEMETRY.md`.
- Added `CHANGELOG.md`, `UPGRADE.md`, `docs/release-checklist.md`, and `docs/distribution-channels.md`.
- Added workspace package metadata inheritance for internal crates and marked them `publish = false`.
- Tightened `.gitignore` so local task state, secrets, logs, build output, release output, and raw reference checkouts do not enter the product repository by default.
- Added `scripts/release-preflight.sh` and `make release-preflight` to make local RC checks and full commercial release blockers executable.
- Added CycloneDX SBOM generation and `scripts/compliance-audit.sh`; release packages now include `SBOM.cdx.json` and `docs/compliance-report.json`.
- Added `deny.toml`; tag/manual release workflow installs `cargo-audit` and `cargo-deny` and runs full compliance mode.
- Added `scripts/install-compliance-tools.sh` so full compliance tooling can be installed under ignored `target/` paths instead of relying on global Cargo state.
- Full local compliance mode now passes with project-local `cargo-audit` and `cargo-deny`; current RustSec output has no vulnerability errors and retains six allowed warning advisories for upstream-only maintenance/unsoundness tracking.
- Direct-connect Unix socket support is implemented for Unix builds through `cc+unix:///path/to/socket` and `kiana server --unix <path>`; Windows builds report the platform boundary explicitly.
- Service-layer API key lookup now reuses the same effective config stack as CLI auth/config commands, including managed settings overrides and empty-value filtering.
- Release smoke now enforces locked/offline Cargo test and release-build gates, and successful smoke subcommands keep their real exit status instead of relying only on output matching.
- Release packaging now emits distribution manifest dry-runs from the same package checksums, including an enterprise offline manifest and explicit package-channel blockers when a target cannot be published yet.
- Release package lifecycle smoke now verifies staged tarball checksums, extraction, install, repeated install, rollback restore, and uninstall behavior in a temporary install root.
- Added a `commercial` permission profile and doctor-level `commercial_security` readiness report for strict sandbox plus approval posture validation.
- Added `kiana doctor --json` with a pinned `kiana.doctor.v1` schema and release smoke validation for schema-critical readiness fields.
- OAuth token files can now be used as redacted remote/bridge bearer-token sources and refreshed through their stored refresh token when command-based refresh is not configured.
- OAuth token files that carry `expires_at` are refreshed before remote-session and bridge live calls when the token is expired or within the proactive refresh window.
- OAuth token writes now use same-directory atomic replacement, flush before rename, and owner-only permissions on platforms with native permission bits.
- Remote code-session create, bridge credential fetch, smoke, and hydrate commands retry once after auth failure through the same remote refresh path used by websocket reconnects.
- Code-session auth retry now keys off typed 401/403 HTTP status from remote API errors, with text matching retained only as a compatibility fallback.
- Tag release workflow now enforces `v$(VERSION)` alignment, derives package manifest URLs from the active GitHub repository/tag, runs full release preflight before packaging, and recursively uploads manifest/compliance artifacts into the draft GitHub Release.
- `kiana auth status --json` now exposes redacted OAuth token-file status, expiry, expiring/expired state, refreshability, and storage source; service-level OAuth token checks reject expired token files instead of using a placeholder false result.
- `kiana auth status --json` now exposes provider-aware readiness for Anthropic, OpenAI-compatible, Ollama, and fake providers, including redacted API key previews plus endpoint/model settings without exposing secrets.
- Startup now records a first-start onboarding sentinel under `KIANA_HOME`, and TUI startup shows a one-time onboarding message when no API key or usable OAuth token is configured.
- TUI `/diff --json` and `/diff --last-assistant --json` now render structured diff previews with file lists, stats, and assistant patch snippets instead of raw JSON transcript output.
- TUI permission requests now carry structured approval panel state with tool, reason, blocked path, input preview, suggestions, and queue context while preserving `/allow` and `/deny` command handling.
- TUI headless render tests now verify the REPL approval panel keeps queued permission context, status shortcuts, and prompt input visible, and the resume picker renders filtered search results plus resume help.
- TUI `/history` now opens a searchable prompt-history picker backed by `KIANA_HOME/tui-history.jsonl`, dedupes newest-first prompt entries, restores the selected prompt as a draft, and has focused headless state/render coverage.
- `scripts/product-shell-smoke.sh` now names and runs the TUI approval, diff, onboarding, resume, and prompt-history headless gates, and `scripts/release-smoke.sh` invokes it in full release mode.
- OpenAI-compatible text provider support is now wired through the provider model profile, runner provider selection, and `kiana model list --json`; tool support is explicitly disabled and rejected before network requests.
- Local Ollama text provider support is now wired through the provider model profile, runner provider selection, and `kiana model list --json`; tool support is explicitly disabled and rejected before network requests.
- Text-only provider runs now default to an empty tool set when tools are not explicitly configured, so text prompts do not require a `--tools ""` workaround.
- `kiana model smoke --json` now emits a pinned `kiana.model-smoke.v1` provider smoke report; default release gates require fake provider text smoke to pass and report live provider skip reasons, while real Anthropic/OpenAI-compatible/Ollama smoke remains opt-in through `--live` or `KIANA_PROVIDER_SMOKE_LIVE=1`.
- `kiana license status --json` now emits a pinned `kiana.license-status.v1` readiness report for offline enterprise license, account, entitlement, support contact, and managed-policy inputs without exposing raw license keys.
- Created a local initial product commit so `HEAD` now resolves and the product tree is clean.
- Fixed user documentation drift for Rust version, config path, model ID, model listing, and README reference links.

## Still Blocking Full Commercial Release

These are not solved by the local release gate and must be completed before claiming broad commercial availability:

- Publish the real Git repository and remote URL, then activate CI on real push/PR/release events.
- Push the local product commit to the real remote, then create an immutable release tag from a reviewed clean tree.
- Activate the release workflow on the real remote, then add signing, notarization where applicable, release credential review, and retention policy review.
- Provide real install URLs and distribution channels such as GitHub Releases, Homebrew, winget, npm, apt, or an enterprise installer.
- Implement package-manager publication; upgrade, rollback, uninstall, and changelog documentation plus local tarball lifecycle smoke now exists but needs release-channel execution.
- Run real remote/CCR/Session Ingress/token refresh end-to-end gates against production-like services.
- Bring live terminal walkthroughs for TUI permission, diff, onboarding, resume, and history to reference-level usability; the named product-shell headless smoke is now wired into release gates, but target-customer interactive acceptance is still pending.
- Finish provider ecosystem parity beyond Anthropic/fake/OpenAI-compatible/Ollama paths, including opt-in live provider smoke reports against production-like credentials/daemons; OpenAI-compatible and Ollama tool calls are covered by local mock gates, and provider credential readiness is visible through `auth status`, but live provider operation is not proven by default gates.
- Continue hardening platform-specific execution isolation and approval policy across Windows, macOS, and Linux; shell policy now blocks additional destructive root-sync shapes such as `rsync --delete ... /` and `robocopy ... C:\ /MIR`, but OS-level sandbox depth still needs platform runner proof.
- Repeat dependency/compliance execution on the real release runners and complete third-party license/advisory signoff for the release record.
- Complete real enterprise account/license backend validation, policy support, and a private vulnerability reporting channel; the local license readiness contract exists, but production entitlement verification is not wired.

## Current Conclusion

This repository is now locally releasable as a source-build release candidate with package artifacts. It is not yet a complete commercial release until the external release infrastructure, live service gates, signing/compliance, and remaining reference-parity product gaps are closed.
