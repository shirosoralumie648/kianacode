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
