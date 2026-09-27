#!/usr/bin/env bash
set -Eeuo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
cd -- "$ROOT"

roadmap="docs/roadmap.md"
detail="docs/roadmap/integrations-connectors.md"
status="CURRENT_STATUS.md"
baseline="docs/roadmap/int33-connector-closeout.md"
failures=0
fail() { echo "int33-connector-closeout: $*" >&2; failures=$((failures + 1)); }

for file in "$roadmap" "$detail" "$status" "$baseline"; do
  [[ -s "$file" ]] || fail "missing $file"
done

for number in $(seq -w 0 32); do
  id="INT-$number"
  rg -q "\`$id\`" "$detail" || fail "missing $id detail card"
  rg -q "\`$id\`" "$roadmap" || fail "missing $id roadmap row/history"
done

for marker in feature_status proof_level source_snapshot worktree_status command_argv limitations reviewer; do
  rg -q "$marker" "$baseline" || fail "closeout baseline missing $marker"
done

rg -q 'feature_status.*partial' "$baseline" || fail 'closeout baseline must remain partial'
rg -q 'proof_level.*source' "$baseline" || fail 'closeout baseline must remain source proof'
if rg -Eiq 'proof_level[^\n]*(durable|live|physical)|feature_status[^\n]*implemented[^\n]*(live|physical)' "$baseline"; then
  fail 'closeout baseline overclaims durable/live/physical proof'
fi

if rg -Eiq 'auto[_ -]?approve|auto[_ -]?resume|second execution loop|default_network_enabled|raw_secret|API_KEY|Bearer |payment|refund' "$baseline"; then
  fail 'closeout baseline contains forbidden/default effect wording'
fi

for marker in 'INT-24' 'INT-25' 'INT-26' 'INT-27' 'INT-28' 'INT-29' 'INT-30' 'INT-31' 'INT-32'; do
  rg -q "$marker" "$status" || fail "CURRENT_STATUS missing $marker evidence"
done

if ((failures > 0)); then
  echo "int33-connector-closeout: $failures failure(s)" >&2
  exit 1
fi
echo "int33-connector-closeout: static guard passed"
