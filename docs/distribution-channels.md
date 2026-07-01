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
  uninstall behavior for a staged tarball.
- Release tarballs include `SBOM.cdx.json` and `docs/compliance-report.json`.
- Release tarballs include pinned JSON schemas under `docs/schemas/`, including
  doctor and model provider smoke reports.
- Distribution manifest dry-runs are generated through
  `scripts/generate-distribution-manifests.sh` and included beside package
  artifacts under `dist/manifests/`.
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

## Planned Package Managers

- Homebrew tap for macOS and Linux.
- winget package for Windows.
- apt/yum repository or signed `.deb`/`.rpm` packages for Linux.
- Enterprise offline bundle for managed environments.

Package-manager manifests must be generated from the same version and checksum
set as the GitHub Release artifacts.

Until Windows packaging produces a winget-supported ZIP/MSI/EXE, the winget
manifest generator records an explicit blocker instead of pretending the tarball
is publishable.
