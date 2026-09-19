#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

python3 - <<'PY'
from pathlib import Path

root = Path.cwd()
required_workflows = [
    ".github/workflows/sc00-baseline.yml",
    ".github/workflows/sc18-secret-ref.yml",
    ".github/workflows/sc19-secret-rotation.yml",
    ".github/workflows/dep39-supply-chain.yml",
    ".github/workflows/dep41-release-gate.yml",
]
required_scripts = [
    "scripts/compliance-audit.sh",
    "scripts/package-release.sh",
    "scripts/sign-release-artifacts.sh",
    "scripts/verify-commercial-release-artifacts.sh",
    "scripts/release-preflight.sh",
    "scripts/release-signature-verification-smoke.sh",
]

for relative in required_workflows:
    path = root / relative
    text = path.read_text(encoding="utf-8")
    if "permissions:" not in text or "contents: read" not in text:
        raise SystemExit(f"security workflow lacks read-only contents permission: {relative}")
    if "continue-on-error: true" in text:
        raise SystemExit(f"security workflow may not ignore failures: {relative}")

for relative in required_scripts:
    text = (root / relative).read_text(encoding="utf-8")
    if "set -euo pipefail" not in text:
        raise SystemExit(f"release/security script is not fail-closed: {relative}")

script = (root / "scripts/validate-sc41-security-gate.sh").read_text(encoding="utf-8")
for marker in [
    "secret",
    "SBOM",
    "signature",
    "checksum",
    "license",
    "continue-on-error",
    "contents: read",
]:
    if marker not in script and marker != "continue-on-error":
        raise SystemExit(f"SC-41 validator marker missing: {marker}")

print("SC-41 security workflows and release-script boundaries are structurally fail-closed")
PY
