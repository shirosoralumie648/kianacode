# Kiana Code Release Readiness

Current status: local source-build release candidate. The project can build,
test, package, and verify a local binary, but it is not a full commercial
general-availability release until the external release infrastructure and
remaining product gates are complete.

## Required Local Gate

```bash
bash scripts/release-preflight.sh --local-rc
```

```bash
bash scripts/release-smoke.sh
```

`make release-smoke` calls the same script when `make` is available.

The gate runs:

- `cargo fmt --all --check`
- `cargo test --workspace --locked --offline --no-fail-fast`
- `cargo build --release -p kiana-entrypoints --bin kiana`
- release binary version and doctor smoke
- help, auth, completion, plugin, MCP, and project MCP smoke checks
- temporary source install into an isolated `INSTALL_DIR`
- installed binary version, doctor, help, auth, plugin, and MCP smoke checks

## Package Gate

```bash
DIST_DIR="$(mktemp -d)" bash scripts/package-release.sh
```

The package script creates:

- `kiana-<version>-<os>-<arch>.tar.gz`
- `kiana-<version>-<os>-<arch>.tar.gz.sha256`
- `kiana-<version>-<os>-<arch>.binary.sha256`
- `SBOM.cdx.json` inside the tarball
- `docs/compliance-report.json` inside the tarball

The tarball includes the binary, licenses, user docs, security/privacy/telemetry
statements, upgrade guidance, changelog, release checklist, commercial readiness
status, and `scripts/install-release-binary.sh` for installing the packaged
binary without rebuilding from source.

## Compliance Gate

```bash
bash scripts/compliance-audit.sh --local-rc
```

Local RC mode verifies workspace package metadata, generates a CycloneDX SBOM,
and records dependency/license advisory tool status. Full commercial mode:

```bash
bash scripts/compliance-audit.sh
```

requires complete third-party license metadata plus `cargo-audit` and
`cargo-deny` to be installed and passing. The `cargo-deny` policy lives in
`deny.toml`.

## CI And Artifact Workflow

- `.github/workflows/release-smoke.yml` runs the local smoke gate on push, PR,
  and manual dispatch.
- `.github/workflows/release.yml` installs `cargo-audit` and `cargo-deny`, runs
  the smoke gate, packages artifacts on Linux/macOS/Windows with full compliance
  reports, verifies checksums, uploads artifacts, and creates a draft GitHub
  Release for tag builds.

These workflows still require a real remote repository, tag policy, release
credentials, and signing before they become authoritative release gates.

Before cutting a real release tag, run the full preflight:

```bash
KIANA_RELEASE_SIGNING_CONFIRMED=1 bash scripts/release-preflight.sh
```

The signing confirmation must only be set after signing, notarization where
applicable, and release-channel credential checks are active.

## Optional Live Service Gate

Real remote session verification requires a remote bearer token:

```bash
KIANA_REMOTE_ACCESS_TOKEN=<token> make live-smoke
```

or:

```bash
KIANA_REMOTE_ACCESS_TOKEN=<token> \
  ./target/debug/kiana remote-session code-session smoke --json
```

This validates CCR v2 code-session creation, bridge credentials, and SDK URL
generation against a real service. It is intentionally not part of default CI
without production-like credentials.

## Still Blocking Commercial GA

- Real git remote, release tags, and active CI.
- Stable install URLs and package-manager channels.
- Binary signing, macOS notarization, retention policy, full dependency audit,
  and third-party license review signoff.
- Live remote/CCR/Session Ingress/token refresh end-to-end verification.
- Reference-level TUI permission, diff, history, onboarding, and resume flows.
- Hardened default sandbox and approval policy across Windows, macOS, and Linux.
- Enterprise account, license, managed policy, and support workflows.

## Related Documents

- `CHANGELOG.md`
- `UPGRADE.md`
- `SECURITY.md`
- `PRIVACY.md`
- `TELEMETRY.md`
- `docs/commercial-release-readiness.md`
- `docs/release-checklist.md`
- `docs/distribution-channels.md`
