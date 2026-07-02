#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

python_bin() {
  command -v python3 2>/dev/null || command -v python 2>/dev/null || {
    echo "schema contract smoke requires python3 or python" >&2
    exit 1
  }
}

python="$(python_bin)"

for schema in docs/schemas/*.json; do
  "$python" -m json.tool "$schema" >/dev/null
done

"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-product-acceptance.v1.schema.json \
  docs/proof-templates/product-acceptance.example.json >/dev/null
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-entitlement-proof.v1.schema.json \
  docs/proof-templates/entitlement-proof.example.json >/dev/null
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-release-ops.v1.schema.json \
  docs/proof-templates/release-ops.example.json >/dev/null
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-platform-security-proof.v1.schema.json \
  docs/proof-templates/platform-security.example.json >/dev/null

tmp_runtime_event="$(mktemp)"
tmp_app_events="$(mktemp)"
trap 'rm -f "$tmp_runtime_event" "$tmp_app_events"' EXIT
cat > "$tmp_runtime_event" <<'JSON'
{
  "event_id": "evt-tool-result",
  "session_id": "session-1",
  "turn_id": "turn-1",
  "sequence": 2,
  "timestamp": "2026-07-02T00:00:02Z",
  "type": "tool_result",
  "tool_call_id": "toolu_read",
  "name": "Read",
  "workbench": "filesystem",
  "is_error": false,
  "content": "ok"
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-runtime-event.v1.schema.json \
  "$tmp_runtime_event" >/dev/null

cat > "$tmp_app_events" <<'JSON'
{
  "schema": "kiana.app-server.events.v1",
  "session_id": "session-1",
  "active": true,
  "work_dir": "/workspace",
  "live_url": "/sessions/session-1/ws",
  "event_source": {
    "type": "sdk-session-tree",
    "available": true
  },
  "limit": 200,
  "total_events": 1,
  "count": 1,
  "truncated": false,
  "events": [
    {
      "event_id": "evt-permission",
      "session_id": "session-1",
      "turn_id": "turn-1",
      "sequence": 3,
      "timestamp": "2026-07-02T00:00:03Z",
      "type": "permission_request",
      "request_id": "perm-1",
      "tool_name": "Bash",
      "action": "run",
      "input": {
        "command": "git status"
      },
      "reason": "ask mode"
    }
  ]
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-app-server-events.v1.schema.json \
  "$tmp_app_events" >/dev/null

tmp_report="$(mktemp)"
trap 'rm -f "$tmp_runtime_event" "$tmp_app_events" "$tmp_report"' EXIT
bash scripts/commercial-release-blockers-report.sh --json > "$tmp_report"
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-commercial-release-blockers.v1.schema.json \
  "$tmp_report" >/dev/null

echo "schema contract smoke passed"
