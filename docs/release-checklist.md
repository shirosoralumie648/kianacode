# Commercial Release Checklist

This checklist is the release manager's gate for turning a local RC into a
commercial release.

## Source Control

- A real git remote is configured.
- `HEAD` resolves to a signed or reviewed commit.
- The release tag is immutable, follows `vMAJOR.MINOR.PATCH`, and matches
  `v$(cat VERSION)`.
- `reference/`, local task state, logs, secrets, and build output are not part of
  the product repository.
- `bash scripts/release-preflight.sh` passes in full mode.

## Build And Test

- `cargo fmt --all --check` passes.
- `bash scripts/release-preflight.sh --local-rc` passes.
- `cargo test --workspace --locked --offline --no-fail-fast` passes.
- `kiana doctor --json` conforms to `docs/schemas/kiana-doctor.v1.schema.json`
  through the release smoke gate.
- `scripts/release-smoke.sh` passes on Linux, macOS, and Windows runners.
- `scripts/product-shell-smoke.sh` passes and remains wired into
  `scripts/release-smoke.sh`.
- `scripts/package-release.sh` produces tarballs and checksums for every target.
- Windows `scripts/package-release.sh` runs produce a portable ZIP plus
  checksum for winget submission.
- Checksum verification passes before upload.
- `scripts/verify-commercial-release-artifacts.sh` passes on the combined
  release artifact set before a draft GitHub Release is created.
- `scripts/compliance-audit.sh --local-rc` produces `SBOM.cdx.json` and
  `compliance-report.json`.
- `scripts/compliance-audit.sh` passes with `cargo-audit`, `cargo-deny`, and
  the checked-in `deny.toml` policy.
- `scripts/install-compliance-tools.sh` installs full audit tooling under
  ignored `target/` paths when the tools are not already available.

## Security And Compliance

- `SECURITY.md`, `PRIVACY.md`, and `TELEMETRY.md` are current.
- Dependency advisory and license review results are attached to the release.
- Any allowed advisory warnings are listed in `deny.toml` or release notes with an owner and follow-up path.
- SBOM output is attached or linked.
- Release binaries are signed; macOS artifacts are notarized when applicable.
- `scripts/sign-release-artifacts.sh` runs with a real `KIANA_SIGNING_COMMAND`
  and, on macOS, real notarization proof input.
- `KIANA_RELEASE_SIGNER` is set to a reviewed signer identity and is not the
  default `external-release-signer` placeholder.
- Signing proof files use `kiana.release-signature.v1`; macOS notarization
  proof files use `kiana.macos-notarization.v1` with `status=accepted`.
- `scripts/release-ops-report.sh full` passes with `kiana.release-ops.v1`
  proof covering private vulnerability reporting, release credential ownership,
  support contact, artifact/log retention, and accepted credential review.
- Private vulnerability reporting route is active before the public release tag
  is created.

## Distribution

- GitHub Release draft contains all artifacts and checksum files.
- Install URLs point to the real repository and release assets.
- Distribution manifests use the active repository/tag release URL and are
  attached with compliance artifacts.
- Enterprise offline manifest validates against the pinned
  `kiana.enterprise.offline-manifest.v1` contract and includes every release
  artifact checksum.
- Upgrade and rollback instructions are linked.
- Package-manager channels are either published or explicitly marked as pending.
- Commercial GA requires published channels; pending/dry-run/blocked channel
  state is allowed only for source-build RCs, not for full commercial release.
- Combined release artifacts include generated Homebrew formulae and winget
  YAML; no channel `BLOCKED.md` file remains.

## Product Readiness

- `scripts/provider-live-smoke.sh --required` passes against at least one real
  provider for text and tool-call smoke; configured OpenAI-compatible or Ollama
  dynamic catalog lookups also pass.
- `scripts/remote-live-smoke.sh --required` passes against production-like
  remote/CCR services and emits `kiana.remote-code-session-smoke.v1` proof.
- Remote and bridge authentication sources are verified by `kiana auth status`
  and `kiana doctor`, including redacted OAuth token-file state when env bearer
  tokens are not used.
- TUI approval, diff, history, onboarding, and resume flows are accepted for the
  target customer segment; the headless product-shell smoke is passing before
  any manual terminal acceptance.
- Product acceptance is recorded in `kiana.product-acceptance.v1` format and
  passes `scripts/product-acceptance-report.sh full`.
- Release artifacts include `dist/proofs/live-smoke/**`,
  `dist/proofs/product/product-acceptance.json`, and
  `dist/proofs/release-ops/release-ops.json`; the commercial artifact verifier
  checks those proof files.
- Sandbox and permission defaults match the documented commercial security
  posture on Windows, macOS, and Linux.
- `kiana doctor` reports `commercial_security: ready` on target release
  environments, or release notes explicitly scope unsupported platforms.
- Enterprise account, license, policy, and support expectations are documented,
  and `kiana license status --json` reports the intended release posture without
  exposing raw license keys.
