#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

python3 - <<'PY'
from pathlib import Path

root = Path.cwd()
review = (root / "docs/security/sc43-security-review.md").read_text(encoding="utf-8")
module_map = (root / "docs/module-map.md").read_text(encoding="utf-8")
status = (root / "CURRENT_STATUS.md").read_text(encoding="utf-8")
roadmap = (root / "docs/roadmap.md").read_text(encoding="utf-8")
baseline = (root / "docs/roadmap/sc43-security-closeout-baseline.md").read_text(encoding="utf-8")

for marker in [
    "review date", "reviewer", "source snapshot", "feature_status", "proof_level",
    "partial", "source", "result_unknown", "reconcile", "durable", "live", "physical",
    "Open risks", "Explicit non-claims",
]:
    if marker not in review:
        raise SystemExit(f"SC-43 review record missing {marker}")
for marker in ["SC-43", "security", "CURRENT_STATUS"]:
    if marker not in module_map:
        raise SystemExit(f"module map missing SC-43 handoff marker {marker}")
if "### SC-43" not in status:
    raise SystemExit("CURRENT_STATUS is missing SC-43")
if '<a id="step-sc-43"></a>SC-43' not in roadmap:
    raise SystemExit("roadmap is missing SC-43")
for marker in ["review record", "module map", "CURRENT_STATUS", "partial", "source", "limitations"]:
    if marker not in baseline:
        raise SystemExit(f"SC-43 baseline missing {marker}")
for marker in [
    "SupplyChainReleaseEvidence",
    "ReleaseUatEvidence",
    "PersistenceUatEvidence",
    "PersistenceCapacityEvidence",
]:
    if marker not in review:
        raise SystemExit(f"SC-43 evidence index missing {marker}")

print("SC-43 security closeout review and status handoff are structurally complete")
PY
