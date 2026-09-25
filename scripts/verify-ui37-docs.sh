#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

for required in \
  docs/ui-entrypoints-runbook.md \
  docs/module-map.md \
  docs/roadmap/ui-entrypoints.md \
  CURRENT_STATUS.md \
  USER.md; do
  test -f "$required" || { echo "ui37_missing:$required" >&2; exit 1; }
done

rg -q 'DaemonHost' docs/ui-entrypoints-runbook.md
rg -q 'ControlPlane' docs/ui-entrypoints-runbook.md
rg -q 'proof_level=source' docs/ui-entrypoints-runbook.md
rg -q 'Unknown' docs/ui-entrypoints-runbook.md
rg -q 'UI-37' docs/roadmap/ui-entrypoints.md
rg -q 'UI-37' CURRENT_STATUS.md
if rg -n 'feature_status=implemented.*proof_level=(durable|live|physical)' docs/ui-entrypoints-runbook.md; then
  echo "ui37_proof_level_overclaim" >&2
  exit 1
fi

echo "ui37_docs_links_and_status_ok"
