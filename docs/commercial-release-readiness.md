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
- `cargo test --workspace --no-fail-fast`
- `cargo build --release -p kiana-entrypoints --bin kiana`
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
- Created a local initial product commit so `HEAD` now resolves and the product tree is clean.
- Fixed user documentation drift for Rust version, config path, model ID, model listing, and README reference links.

## Still Blocking Full Commercial Release

These are not solved by the local release gate and must be completed before claiming broad commercial availability:

- Publish the real Git repository and remote URL, then activate CI on real push/PR/release events.
- Push the local product commit to the real remote, then create an immutable release tag from a reviewed clean tree.
- Activate the release workflow on the real remote, then add signing, notarization where applicable, release credential review, and retention policy review.
- Provide real install URLs and distribution channels such as GitHub Releases, Homebrew, winget, npm, apt, or an enterprise installer.
- Implement package-manager publication; upgrade, rollback, uninstall, and changelog documentation now exists but needs release-channel execution.
- Run real remote/CCR/Session Ingress/token refresh end-to-end gates against production-like services.
- Bring TUI permission, diff, history, onboarding, and resume flows to reference-level usability.
- Harden default execution isolation and approval policy across Windows, macOS, and Linux.
- Add dependency/compliance gates such as `cargo audit`, `cargo deny`, SBOM generation, and third-party license review.
- Complete enterprise account/license/policy support and a private vulnerability reporting channel.

## Current Conclusion

This repository is now locally releasable as a source-build release candidate with package artifacts. It is not yet a complete commercial release until the external release infrastructure, live service gates, signing/compliance, and remaining reference-parity product gaps are closed.
