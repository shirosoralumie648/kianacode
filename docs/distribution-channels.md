# Distribution Channels

Kiana Code currently supports source-build installation and release tarballs.
The channels below define the commercial distribution target state.

## Available Now

- Source checkout install through `install.sh` or `make install`.
- Local release tarball through `scripts/package-release.sh`.
- Binary tarball install through `scripts/install-release-binary.sh` after
  extracting the release archive.
- Binary tarball uninstall through `scripts/install-release-binary.sh --uninstall`.
- Package lifecycle smoke through `scripts/package-lifecycle-smoke.sh` verifies
  checksum, extraction, install, repeated install, rollback restore, and
  uninstall behavior for a staged tarball. It also verifies that commercial
  release proof scripts and pinned proof schemas are packaged.
- Release tarballs include `SBOM.cdx.json` and `docs/compliance-report.json`.
- Release tarballs include pinned JSON schemas under `docs/schemas/`, including
  doctor, model provider smoke, license readiness, release signature, and macOS
  notarization reports.
- Distribution manifest dry-runs are generated through
  `scripts/generate-distribution-manifests.sh` and included beside package
  artifacts under `dist/manifests/`.
- Windows package runs now produce a portable ZIP beside the tarball, with a
  checksum file for winget submission.
- When combined release artifacts include the Windows portable ZIP,
  `scripts/generate-distribution-manifests.sh` emits winget version, locale,
  and installer YAML under `dist/manifests/winget/`.
- Commercial artifact verification runs through
  `scripts/verify-commercial-release-artifacts.sh` after all runner artifacts
  are combined; it fails on missing or unverifiable signatures, missing macOS notarization
  proof, missing Windows ZIP/MSI/EXE publishable artifacts, blocked channel
  manifests, missing live smoke proof, missing product acceptance proof, or
  missing Linux/macOS/Windows platform security proof.
- `scripts/release-signature-verification-smoke.sh` runs an offline fixture
  against the commercial verifier so invalid signatures, blocked channels, and
  missing proof artifacts cannot regress silently.
- Enterprise offline bundles include
  `manifests/enterprise/offline-manifest.json`, generated from the same archive
  checksum files and validated by package lifecycle smoke against the pinned
  `kiana.enterprise.offline-manifest.v1` schema.
- GitHub Actions artifact workflow for Linux, macOS, and Windows once the real
  remote is active.
- Release CI installs compliance tools through `scripts/install-compliance-tools.sh`
  before running full audit mode.

## Required For Public Commercial Release

- GitHub Releases with tarballs and SHA256 files.
- Signed Windows and Linux binaries.
- Signed and notarized macOS binaries.
- A stable install URL that does not depend on a moving branch.
- Real public release notes that link the existing install, upgrade, rollback,
  and uninstall instructions for the shipped artifact.
- Signing proof artifacts using `kiana.release-signature.v1` beside every
  archive and binary checksum, validated against the target archive and
  signature file names plus `KIANA_SIGNATURE_VERIFY_COMMAND` verification.
- macOS notarization proof artifacts using `kiana.macos-notarization.v1` with
  `status=accepted`.

## Planned Package Managers

- Homebrew tap for macOS and Linux.
- winget package for Windows.
- apt/yum repository or signed `.deb`/`.rpm` packages for Linux.
- Enterprise offline bundle for managed environments.

Package-manager manifests must be generated from the same version and checksum
set as the GitHub Release artifacts.

Until Windows packaging produces a winget-supported ZIP/MSI/EXE, the winget
manifest generator records an explicit blocker instead of pretending the tarball
is publishable. Windows release packaging now emits a portable ZIP; full
commercial verification still requires that ZIP to be present in the combined
release artifact set before the winget blocker is removed.

Full commercial release verification treats those blockers as hard failures.
Use local RC mode for source-build release candidates that intentionally ship
before signing, notarization, or package-manager publication is complete.
