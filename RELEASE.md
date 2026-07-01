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
- `cargo build --release --locked --offline -p kiana-entrypoints --bin kiana`
- release binary version plus doctor text and `kiana.doctor.v1` JSON smoke
- help, auth, completion, plugin, MCP, and project MCP smoke checks
- temporary source install into an isolated `INSTALL_DIR`
- installed binary version, doctor text/JSON, help, auth, plugin, and MCP smoke checks

## Package Gate

```bash
DIST_DIR="$(mktemp -d)" bash scripts/package-release.sh
```

The package script creates:

- `kiana-<version>-<os>-<arch>.tar.gz`
- `kiana-<version>-<os>-<arch>.tar.gz.sha256`
- `kiana-<version>-<os>-<arch>.binary.sha256`
- `manifests/enterprise/offline-manifest.json`
- package-channel dry-run files or explicit blockers under `manifests/`
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
`deny.toml`. Install those tools into ignored local build paths with:

```bash
bash scripts/install-compliance-tools.sh
```

For one-shot local full compliance, use:

```bash
KIANA_COMPLIANCE_AUTO_INSTALL=1 bash scripts/compliance-audit.sh
```

## CI And Artifact Workflow

- `.github/workflows/release-smoke.yml` runs the local smoke gate on push, PR,
  and manual dispatch.
- `.github/workflows/release.yml` installs `cargo-audit` and `cargo-deny`
  through `scripts/install-compliance-tools.sh`, validates `v$(VERSION)` tag
  alignment, derives manifest URLs from the active repository/tag, runs full
  preflight, runs the smoke gate, packages artifacts on Linux/macOS/Windows
  with full compliance reports, runs `scripts/sign-release-artifacts.sh` using
  release signing/notarization commands, verifies checksums, preserves signing
  and notarization proof artifacts, regenerates combined distribution manifests,
  runs `scripts/verify-commercial-release-artifacts.sh`, uploads artifacts, and
  creates a draft GitHub Release for tag builds only after the commercial
  artifact proof gate passes.

These workflows still require a real remote repository, tag policy, release
credentials, and signing before they become authoritative release gates.

Before cutting a real release tag, run the full preflight:

```bash
bash scripts/release-preflight.sh
```

The full preflight no longer accepts a manual signing confirmation variable.
Commercial release tags must produce signed artifacts, macOS notarization proof,
publishable channel manifests, and no blocked channel state before the draft
GitHub Release step can run.

Artifact signing is explicit and credential-backed:

```bash
KIANA_SIGNING_COMMAND='gpg --batch --yes --armor --detach-sign --output "$KIANA_SIGN_OUTPUT" "$KIANA_SIGN_INPUT"' \
  bash scripts/sign-release-artifacts.sh
```

The signing command receives `KIANA_SIGN_INPUT` and `KIANA_SIGN_OUTPUT` for
each archive and binary checksum. macOS targets also require
`KIANA_MACOS_NOTARIZATION_COMMAND` to write `KIANA_NOTARIZATION_PROOF`, or a
pre-validated `KIANA_MACOS_NOTARIZATION_PROOF_FILE`; the proof must use
`kiana.macos-notarization.v1` with `status=accepted`.

Product acceptance is explicit as well. Local RCs can record headless product
shell coverage:

```bash
bash scripts/product-acceptance-report.sh --local-rc
```

Full commercial preflight requires `KIANA_PRODUCT_ACCEPTANCE_FILE` or
`docs/product-acceptance/$(cat VERSION).json` with
`schema = kiana.product-acceptance.v1`, `status = accepted`, and the required
permission, diff, history, onboarding, resume, and settings workflows.

## Optional Live Service Gate

Live provider verification requires at least one real provider target. The
script stores proof JSON under `target/live-smoke/provider/` by default:

```bash
ANTHROPIC_API_KEY=<key> \
  bash scripts/provider-live-smoke.sh --required
```

OpenAI-compatible and Ollama live targets can also be verified with
`KIANA_OPENAI_API_KEY`/`OPENAI_API_KEY` plus optional
`KIANA_OPENAI_BASE_URL`, or explicit `KIANA_OLLAMA_BASE_URL`/`OLLAMA_BASE_URL`
plus model env. The provider script runs `kiana model catalog --live --json`
and `kiana model smoke --live --tools --json`, then requires at least one real
text provider and one real tool-call provider to pass in `--required` mode.

Real remote session verification requires a remote bearer token and stores proof
JSON under `target/live-smoke/remote/` by default:

```bash
KIANA_REMOTE_ACCESS_TOKEN=<token> \
  bash scripts/remote-live-smoke.sh --required
```

This validates CCR v2 code-session creation, bridge credentials, and SDK URL
generation against a real service. `scripts/release-preflight.sh` runs both
live smoke scripts in full mode; local RC and ordinary smoke gates do not run
them without production-like credentials.

## Still Blocking Commercial GA

- Real git remote, release tags, and active CI.
- Stable install URLs and package-manager channels.
- Binary signing, macOS notarization, retention policy, full dependency audit,
  and third-party license review signoff.
- Live remote/CCR/Session Ingress/token refresh end-to-end verification.
- Live provider text/tool smoke and live dynamic model-catalog proof against
  production-like credentials or daemons.
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
