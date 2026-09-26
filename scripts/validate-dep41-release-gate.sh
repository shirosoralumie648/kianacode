#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

python3 - <<'PY'
from pathlib import Path

root = Path.cwd()
runbook = (root / "docs/roadmap/dep41-operator-runbook.md").read_text(encoding="utf-8")
matrix = (root / "docs/roadmap/dep41-capability-proof-matrix.md").read_text(encoding="utf-8")
status = (root / "CURRENT_STATUS.md").read_text(encoding="utf-8")
roadmap = (root / "docs/roadmap.md").read_text(encoding="utf-8")

for marker in [
    "DaemonHost",
    "ControlPlane",
    "result_unknown",
    "reconcile",
    "live",
    "physical",
    "operator approval",
    "source_snapshot",
    "worktree_status",
    "command_argv",
    "cwd·environment",
    "fixture·cassette",
    "exit_code",
    "status change",
    "proof-level change",
    "limitations",
    "reviewer",
    "ProviderLiveConnectionEvidence",
    "ContextMemoryGoldenPathEvidence",
    "UiEvidenceBundle",
    "CompanyLiveCloseoutEvidence",
    "ContainerLifecycleEvidence",
    "OrchestratedRolloutEvidence",
    "RolloutLifecycleEvidence",
    "SupplyChainReleaseEvidence",
    "ReleaseUatEvidence",
]:
    if marker not in runbook:
        raise SystemExit(f"DEP-41 runbook missing {marker}")

for marker in [
    "feature_status",
    "proof_level",
    "implemented",
    "partial",
    "target",
    "deferred",
    "not_supported",
    "source",
    "local_behavior",
    "durable",
    "live",
    "physical",
    "CAP-33",
    "DEP-40",
    "H36",
    "CM-39",
    "UI-41",
    "CO-48",
    "limitations",
    "reviewer",
]:
    if marker not in matrix:
        raise SystemExit(f"DEP-41 matrix missing {marker}")

header = "| Slice | feature_status | proof_level | Current evidence | Hard limit / next gate |"
start = matrix.find(header)
if start < 0:
    raise SystemExit("DEP-41 matrix table header missing")

allowed_feature_status = {"implemented", "partial", "target", "deferred", "not_supported"}
allowed_proof_level = {"source", "local_behavior", "durable", "live", "physical"}
matrix_rows = []
for line in matrix[start:].splitlines()[1:]:
    if not line.strip():
        break
    if not line.startswith("|"):
        raise SystemExit("DEP-41 matrix contains a non-table line")
    cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
    if cells and all(set(cell) <= {"-", ":", " "} for cell in cells):
        continue
    if len(cells) != 5 or any(not cell for cell in cells):
        raise SystemExit("DEP-41 matrix rows must have five non-empty columns")
    if cells[1] not in allowed_feature_status:
        raise SystemExit(f"DEP-41 invalid feature_status: {cells[1]}")
    if cells[2] not in allowed_proof_level:
        raise SystemExit(f"DEP-41 invalid proof_level: {cells[2]}")
    matrix_rows.append(cells)

if len(matrix_rows) < 10:
    raise SystemExit("DEP-41 matrix lost required handoff rows")
if any("overall project complete" in " ".join(row).lower() for row in matrix_rows):
    raise SystemExit("DEP-41 matrix contains blanket completion language")

if "| DEP-41 handoff gate | partial | source |" not in matrix:
    raise SystemExit("DEP-41 matrix must keep its own gate partial/source")
if "### DEP-41" not in status:
    raise SystemExit("CURRENT_STATUS is missing the DEP-41 evidence block")
if "<a id=\"step-dep-41\"></a>`DEP-41`" not in roadmap:
    raise SystemExit("roadmap is missing the DEP-41 detailed card")
if "feature_status=implemented" in matrix and "proof_level=source" not in matrix:
    raise SystemExit("implemented rows must expose an explicit proof ceiling")

print("DEP-41 release gate documentation and evidence contracts are structurally complete")
PY
