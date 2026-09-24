#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

# SC-28 scanner structural guard: keep the read-only policy and quarantine
# contract visible to CI reviewers even when scanner binaries are unavailable.
python3 - <<'PY'
from pathlib import Path

root = Path.cwd()
workflow = (root / ".github/workflows/sc28-supply-chain.yml").read_text(encoding="utf-8")
scanner = (root / "scripts/supply-chain-scan.sh").read_text(encoding="utf-8")
normalizer = (root / "scripts/supply-chain-scan.py").read_text(encoding="utf-8")
baseline = (root / "docs/roadmap/sc28-supply-chain-baseline.md").read_text(encoding="utf-8")
deny = (root / "deny.toml").read_text(encoding="utf-8")

for marker in ["permissions:", "contents: read", "cargo-audit", "cargo-deny", "upload-artifact@v4", "if: always()"]:
    if marker not in workflow:
        raise SystemExit(f"SC-28 workflow marker missing: {marker}")
if "continue-on-error: true" in workflow:
    raise SystemExit("SC-28 workflow must not ignore scanner failures")
for marker in ["set -euo pipefail", "Cargo.lock", "cargo metadata", "cargo audit", "cargo deny", "SC28_MAX_HIGH_ADVISORIES", "SC28_MAX_CRITICAL_ADVISORIES"]:
    if marker not in scanner:
        raise SystemExit(f"SC-28 scanner marker missing: {marker}")
for marker in ["CycloneDX", "SPDX-2.3", "lockfile_drift", "license_unknown", "advisory_found", "quarantined", "automatic_release_allowed"]:
    if marker not in normalizer:
        raise SystemExit(f"SC-28 normalizer marker missing: {marker}")
# The normalizer's automatic_release_allowed field is intentionally checked
# above; it must never be omitted from the quarantine record.
for marker in ["[advisories]", "[licenses]", "[bans]", "[sources]", "unknown-registry", "unknown-git"]:
    if marker not in deny:
        raise SystemExit(f"deny.toml marker missing: {marker}")
for marker in ["SBOM", "SPDX", "CycloneDX", "Cargo.lock", "advisory", "license", "quarantine", "partial", "source", "limitations", "reviewer"]:
    if marker not in baseline:
        raise SystemExit(f"SC-28 baseline marker missing: {marker}")
print("SC-28 supply-chain workflow, manifests, thresholds and quarantine boundaries are structurally fail-closed")
PY
