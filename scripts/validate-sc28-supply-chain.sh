#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

# SC-28 scanner structural guard: keep the read-only policy, the closed reason
# code set and the quarantine contract visible to CI reviewers even when scanner
# binaries are unavailable, then run the offline deny-first normalizer fixtures.
python3 - <<'PY'
from pathlib import Path

root = Path.cwd()
workflow = (root / ".github/workflows/sc28-supply-chain.yml").read_text(encoding="utf-8")
scanner = (root / "scripts/supply-chain-scan.sh").read_text(encoding="utf-8")
normalizer = (root / "scripts/supply-chain-scan.py").read_text(encoding="utf-8")
validator = (root / "scripts/validate-sc28-supply-chain.sh").read_text(encoding="utf-8")
baseline = (root / "docs/roadmap/sc28-supply-chain-baseline.md").read_text(encoding="utf-8")
deny = (root / "deny.toml").read_text(encoding="utf-8")
fixtures = (root / "scripts/tests/test_sc28_supply_chain.py").read_text(encoding="utf-8")

for marker in [
    "permissions:",
    "contents: read",
    "cargo-audit",
    "cargo-deny",
    "upload-artifact@v4",
    "if: always()",
    "test_sc28_supply_chain.py",
    'SC28_MAX_HIGH_ADVISORIES: "0"',
    'SC28_MAX_CRITICAL_ADVISORIES: "0"',
]:
    if marker not in workflow:
        raise SystemExit(f"SC-28 workflow marker missing: {marker}")
if "continue-on-error: true" in workflow:
    raise SystemExit("SC-28 workflow must not ignore scanner failures")
for marker in [
    "set -euo pipefail",
    "Cargo.lock",
    "cargo metadata",
    "cargo audit",
    "cargo deny",
    "SC28_MAX_HIGH_ADVISORIES",
    "SC28_MAX_CRITICAL_ADVISORIES",
    "--metadata-exit",
    "--lock-present",
    "--audit-version",
]:
    if marker not in scanner:
        raise SystemExit(f"SC-28 scanner marker missing: {marker}")
for marker in [
    "CycloneDX",
    "SPDX-2.3",
    "REASON_CODES",
    "lockfile_missing",
    "lockfile_resolution_failed",
    "dependency_graph_empty",
    "lockfile_drift",
    "license_unknown",
    "advisory_found",
    "advisory_severity_unclassified",
    "quarantined",
    "automatic_release_allowed",
]:
    if marker not in normalizer:
        raise SystemExit(f"SC-28 normalizer marker missing: {marker}")
# The normalizer's automatic_release_allowed field is intentionally checked
# above; it must never be omitted from the quarantine record. It must also stay
# a pure normalizer: process and network access belong to the shell wrapper.
for module in ("subprocess", "urllib", "socket", "requests", "shutil", "ctypes"):
    if f"import {module}" in normalizer.replace("from ", "import "):
        raise SystemExit(f"SC-28 normalizer must not import {module}")
for marker in ["[advisories]", "[licenses]", "[bans]", "[sources]", "unknown-registry", "unknown-git"]:
    if marker not in deny:
        raise SystemExit(f"deny.toml marker missing: {marker}")
for marker in [
    "SBOM",
    "SPDX",
    "CycloneDX",
    "Cargo.lock",
    "advisory",
    "license",
    "quarantine",
    "completion slice",
    "deny-first",
    "test_sc28_supply_chain.py",
    "lockfile_resolution_failed",
    "source",
    "limitations",
    "reviewer",
]:
    if marker not in baseline:
        raise SystemExit(f"SC-28 baseline marker missing: {marker}")
for marker in [
    "test_missing_lockfile_quarantines_before_any_resolution",
    "test_an_unlocked_dependency_graph_quarantines_without_a_silent_pass",
    "test_lockfile_drift_between_resolution_and_scan_quarantines",
    "test_unknown_license_metadata_quarantines_and_stays_noassertion",
    "test_missing_scanner_binaries_never_count_as_a_pass",
    "test_clean_locked_snapshot_passes_and_emits_deterministic_manifests",
]:
    if marker not in fixtures:
        raise SystemExit(f"SC-28 fixture missing: {marker}")
if "test_sc28_supply_chain.py" not in validator:
    raise SystemExit("SC-28 validator must run the offline normalizer fixtures")
print("SC-28 supply-chain workflow, manifests, thresholds and quarantine boundaries are structurally fail-closed")
PY

# The deny-first normalizer fixtures are pure Python and offline: no cargo, no
# scanner download, no network. Running them through this validator keeps the
# quarantine contract exercised by the unified `ci` gate as well as by the
# automatic `sc28-supply-chain.yml` lane.
python3 -B scripts/tests/test_sc28_supply_chain.py
