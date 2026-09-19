#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

for script in \
  scripts/package-lifecycle-smoke.sh \
  scripts/oa28-live-handoff-preflight.sh \
  scripts/oa26-durable-observability-gate.sh; do
  test -f "$script"
  bash -n "$script"
done

python3 - <<'PY'
from pathlib import Path

root = Path.cwd()
baseline = (root / "docs/roadmap/sc42-security-rehearsal-baseline.md").read_text(encoding="utf-8")
status = (root / "CURRENT_STATUS.md").read_text(encoding="utf-8")
roadmap = (root / "docs/roadmap.md").read_text(encoding="utf-8")
for marker in ["restart", "quarantine", "replay", "result_unknown", "reconcile", "retention", "legal hold", "partial", "durable", "live"]:
    if marker not in baseline:
        raise SystemExit(f"SC-42 baseline missing {marker}")
if "### SC-42" not in status:
    raise SystemExit("CURRENT_STATUS is missing SC-42")
if '<a id="step-sc-42"></a>SC-42' not in roadmap:
    raise SystemExit("roadmap is missing SC-42")
print("SC-42 recovery and retention rehearsal boundaries are structurally complete")
PY
