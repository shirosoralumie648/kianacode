# Upgrade Guide

Kiana Code is currently pre-1.0. Treat upgrades as controlled rollouts, not
automatic background updates.

## Supported Upgrade Path

1. Read `CHANGELOG.md`.
2. Back up `~/.kiana/config.toml` and any local hooks, permissions, plugins, and
   task state under `~/.kiana`.
3. Install the new binary into a staging location first.
4. Run:

   ```bash
   kiana --version
   kiana doctor
   kiana model list --json
   ```

5. Run one non-destructive prompt with the intended provider configuration.
6. For release tarballs, run the package lifecycle smoke against the staged
   artifact:

   ```bash
   DIST_DIR="/path/to/dist" bash scripts/package-lifecycle-smoke.sh
   ```

7. Replace the production binary only after the staging checks pass.

## Rollback

Keep the previous release binary and its checksum until the new version has
passed at least one real workflow. To roll back:

```bash
cp ./previous/kiana ~/.local/bin/kiana
kiana --version
kiana doctor
```

On Windows, use `kiana.exe` and the installation directory selected during
install.

For tarball installs, the packaged install script also supports uninstalling
the selected `INSTALL_DIR` target:

```bash
INSTALL_DIR="$HOME/.local/bin" bash scripts/install-release-binary.sh --uninstall
```

## Config Compatibility

- Primary config path: `~/.kiana/config.toml`.
- Current default model profile: `claude-sonnet-4-6`.
- API keys may be supplied through environment variables or `kiana login`.
- Remote session credentials are separate from provider API keys.

## Pre-1.0 Breaking Change Policy

Before a public commercial release, any incompatible change must update:

- `CHANGELOG.md`
- `UPGRADE.md`
- `CONFIG.md`
- Relevant release smoke or package checks
