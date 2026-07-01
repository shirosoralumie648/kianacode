# Security Policy

## Supported State

Kiana is currently a source-build release-readiness snapshot. Treat it as pre-1.0 until the release pipeline, signed artifacts, dependency review, and live remote gates are complete.

## Reporting Vulnerabilities

Do not publish working exploits, tokens, API keys, or customer data in public issues. Use the maintainer's private security channel once the project repository is published. If no private channel is configured yet, block public release until one is available.

Commercial releases must attach an accepted `kiana.release-ops.v1` proof through
`scripts/release-ops-report.sh full`. That proof records the private reporting
route, security contact, release credential owner, support contact, artifact/log
retention policy, and credential review owner.

## Security Scope

Security-sensitive areas include:

- Tool execution and permission checks.
- Bash/PowerShell sandboxing and platform fallbacks.
- MCP server configuration, headers, and environment expansion.
- Remote sessions, bridge credentials, OAuth/session token refresh, and trusted-device tokens.
- Plugin, skill, and marketplace installation paths.
- Installer scripts, release artifacts, checksums, and upgrade/uninstall flows.

## Current Boundaries

The local release smoke gate verifies formatting, tests, release build, CLI help/auth/completion/plugin/MCP paths, and temporary source installation. It does not replace:

- A signed release-artifact pipeline.
- Cross-platform installer validation.
- `cargo audit` / `cargo deny` / SBOM review.
- Real remote-service end-to-end validation.
- A published security advisory process.
- An accepted `kiana.release-ops.v1` operations proof for public release.

## Secret Handling

Never commit API keys, remote access tokens, MCP headers, session tokens, or trusted-device tokens. Prefer environment variables or the configured Kiana home directory, and rotate any token that appears in logs, screenshots, crash reports, or issue text.
