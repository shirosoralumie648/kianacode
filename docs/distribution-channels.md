# Distribution Channels

Kiana Code currently supports source-build installation and release tarballs.
The channels below define the commercial distribution target state.

## Available Now

- Source checkout install through `install.sh` or `make install`.
- Local release tarball through `scripts/package-release.sh`.
- Binary tarball install through `scripts/install-release-binary.sh` after
  extracting the release archive.
- Release tarballs include `SBOM.cdx.json` and `docs/compliance-report.json`.
- GitHub Actions artifact workflow for Linux, macOS, and Windows once the real
  remote is active.

## Required For Public Commercial Release

- GitHub Releases with tarballs and SHA256 files.
- Signed Windows and Linux binaries.
- Signed and notarized macOS binaries.
- A stable install URL that does not depend on a moving branch.
- Upgrade, rollback, and uninstall instructions linked from release notes.

## Planned Package Managers

- Homebrew tap for macOS and Linux.
- winget package for Windows.
- apt/yum repository or signed `.deb`/`.rpm` packages for Linux.
- Enterprise offline bundle for managed environments.

Package-manager manifests must be generated from the same version and checksum
set as the GitHub Release artifacts.
