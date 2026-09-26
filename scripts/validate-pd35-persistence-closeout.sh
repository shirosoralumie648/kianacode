#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

python3 - <<'PY'
from pathlib import Path

root = Path.cwd()
closeout = (root / "docs/roadmap/pd35-persistence-closeout.md").read_text(encoding="utf-8")
design = (root / "docs/roadmap/persistence-data-layer.md").read_text(encoding="utf-8")
status = (root / "CURRENT_STATUS.md").read_text(encoding="utf-8")
roadmap = (root / "docs/roadmap.md").read_text(encoding="utf-8")
runbook = (root / "docs/roadmap/dep41-operator-runbook.md").read_text(encoding="utf-8")
matrix = (root / "docs/roadmap/dep41-capability-proof-matrix.md").read_text(encoding="utf-8")

for number in range(35):
    marker = f"PD-{number:02d}"
    if marker not in closeout or marker not in design:
        raise SystemExit(f"PD-35 closeout missing {marker}")
for marker in [
    "feature_status",
    "proof_level",
    "partial",
    "target/partial",
    "source",
    "durable",
    "live",
    "physical",
    "StorageRoot",
    "preflight",
    "MigrationRegistry",
    "result_unknown",
    "retention",
    "legal-hold",
    "deletion receipt",
    "limitations",
    "reviewer",
]:
    if marker not in closeout:
        raise SystemExit(f"PD-35 closeout missing {marker}")

header = "| Range | feature_status | proof_level | Handoff |"
start = closeout.find(header)
if start < 0:
    raise SystemExit("PD-35 closeout table header missing")
allowed_feature_status = {"implemented", "partial", "target/partial", "target", "deferred", "not_supported"}
allowed_proof_level = {"source", "local_behavior", "durable", "live", "physical"}
pd_rows = []
for line in closeout[start:].splitlines()[1:]:
    if not line.strip():
        break
    if not line.startswith("|"):
        raise SystemExit("PD-35 closeout contains a non-table line")
    cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
    if cells and all(set(cell) <= {"-", ":", " "} for cell in cells):
        continue
    if len(cells) != 4 or any(not cell for cell in cells):
        raise SystemExit("PD-35 closeout rows must have four non-empty columns")
    if cells[1] not in allowed_feature_status or cells[2] not in allowed_proof_level:
        raise SystemExit("PD-35 closeout has invalid status/proof value")
    pd_rows.append(cells)
if len(pd_rows) < 9:
    raise SystemExit("PD-35 closeout table lost required ranges")
if any(row[1] == "implemented" and row[2] in {"durable", "live", "physical"} for row in pd_rows):
    raise SystemExit("PD-35 closeout overclaims implemented durable/live/physical")

if "### PD-35" not in status:
    raise SystemExit("CURRENT_STATUS is missing the PD-35 evidence block")
if '<a id="step-pd-35"></a>`PD-35`' not in roadmap:
    raise SystemExit("roadmap is missing the PD-35 detailed card")
if "result_unknown" not in runbook or "proof_level" not in matrix:
    raise SystemExit("DEP-41 handoff references are incomplete")
if "PD-35 closeout | implemented | durable" in closeout:
    raise SystemExit("PD-35 documentation must not claim durable completion")
for marker in ["PersistenceUatEvidence", "PersistenceCapacityEvidence"]:
    if marker not in closeout:
        raise SystemExit(f"PD-35 evidence index missing {marker}")

print("PD-35 persistence closeout and migration handoff are structurally complete")
PY
