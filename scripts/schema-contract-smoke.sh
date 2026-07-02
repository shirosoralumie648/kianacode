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
tmp_proof_manifest="$(mktemp)"
tmp_doctor="$(mktemp)"
trap 'rm -f "$tmp_runtime_event" "$tmp_app_events" "$tmp_proof_manifest" "$tmp_doctor"' EXIT
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

cat > "$tmp_doctor" <<'JSON'
{
  "schema": "kiana.doctor.v1",
  "status": "warning",
  "cwd": "/workspace",
  "cargo": {
    "available": true,
    "value": "cargo 1.89.0"
  },
  "git_root": {
    "available": true,
    "value": "/workspace"
  },
  "config_file": {
    "path": "/home/user/.kiana/config.toml",
    "found": false
  },
  "sdk_sessions_dir": {
    "path": "/home/user/.kiana/sessions",
    "found": false
  },
  "api_key_set": false,
  "model": "claude-sonnet-4",
  "remote_settings": {
    "status": "missing",
    "file": "missing"
  },
  "tui_permission_request": {
    "active": false,
    "queued": 0
  },
  "mcp_transport": {
    "wired": true,
    "transports": ["stdio", "http", "sse", "ws"],
    "surfaces": ["tools", "resources", "resource_templates", "prompts"]
  },
  "modifiers": {
    "platform": "linux",
    "backend": "none",
    "available": false,
    "current": []
  },
  "remote_bridge": {
    "start_command_wired": true,
    "token_configured": false
  },
  "remote_code_session": {
    "live_smoke_token": "no",
    "configured": false,
    "source": null
  },
  "oauth_token_file": {
    "status": "missing",
    "valid": true,
    "refreshable": false,
    "error": null
  },
  "bash_sandbox": {
    "enabled": false,
    "status": "disabled",
    "runtime": "disabled",
    "fail_if_unavailable": false,
    "allow_unsandboxed_commands": true,
    "bwrap": "missing"
  },
  "commercial_security": {
    "ready": false,
    "status": "not_ready",
    "platform": "linux",
    "isolation": "linux_bwrap",
    "controls": ["permission_profile:commercial"],
    "issues": ["set `kiana permissions profile commercial`"]
  },
  "reference_capabilities": [
    {
      "id": "provider-registry",
      "domain": "provider/model/auth",
      "status": "local_ready_external_required",
      "references": ["cline", "pi", "langchain"],
      "surfaces": ["model-list", "model-catalog", "model-smoke"],
      "evidence": ["fake-provider-standard-tests"],
      "risks": ["production-like provider live smoke proof is external"]
    }
  ],
  "warnings": ["set ANTHROPIC_API_KEY or ~/.kiana/config.toml before sending model prompts"]
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-doctor.v1.schema.json \
  "$tmp_doctor" >/dev/null

cat > "$tmp_proof_manifest" <<'JSON'
{
  "schema": "kiana.commercial-proof-manifest.v1",
  "version": "0.1.0",
  "generated_at": "2026-07-02T00:00:00Z",
  "proof_root": "dist/proofs",
  "summary": {
    "proofs": 1,
    "accepted": 1,
    "live": 0,
    "platforms": ["linux"]
  },
  "proofs": [
    {
      "id": "acceptance.platform-security.linux",
      "category": "acceptance",
      "schema": "kiana.platform-security-proof.v1",
      "status": "accepted",
      "accepted": true,
      "live": null,
      "source": "docs/platform-security/0.1.0-linux.json",
      "path": "dist/proofs/platform-security/platform-security-linux.json",
      "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
      "details": {
        "platform": "linux"
      }
    }
  ]
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-commercial-proof-manifest.v1.schema.json \
  "$tmp_proof_manifest" >/dev/null

tmp_report="$(mktemp)"
trap 'rm -f "$tmp_runtime_event" "$tmp_app_events" "$tmp_proof_manifest" "$tmp_doctor" "$tmp_report"' EXIT
bash scripts/commercial-release-blockers-report.sh --json > "$tmp_report"
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-commercial-release-blockers.v1.schema.json \
  "$tmp_report" >/dev/null

echo "schema contract smoke passed"
