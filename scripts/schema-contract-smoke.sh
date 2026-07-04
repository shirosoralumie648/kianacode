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
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-source-control-proof.v1.schema.json \
  docs/proof-templates/source-control.example.json >/dev/null

tmp_runtime_event="$(mktemp)"
tmp_app_events="$(mktemp)"
tmp_auth_status="$(mktemp)"
tmp_context_index="$(mktemp)"
tmp_repo_map="$(mktemp)"
tmp_diff="$(mktemp)"
tmp_checkpoint="$(mktemp)"
tmp_checks_dry_run="$(mktemp)"
tmp_checks_run="$(mktemp)"
tmp_review_dry_run="$(mktemp)"
tmp_review_run="$(mktemp)"
tmp_proof_manifest="$(mktemp)"
tmp_source_control="$(mktemp)"
tmp_enterprise_offline_manifest="$(mktemp)"
tmp_doctor="$(mktemp)"
tmp_local_rc_evidence="$(mktemp)"
trap 'rm -f "$tmp_runtime_event" "$tmp_app_events" "$tmp_auth_status" "$tmp_context_index" "$tmp_repo_map" "$tmp_diff" "$tmp_checkpoint" "$tmp_checks_dry_run" "$tmp_checks_run" "$tmp_review_dry_run" "$tmp_review_run" "$tmp_proof_manifest" "$tmp_source_control" "$tmp_enterprise_offline_manifest" "$tmp_doctor" "$tmp_local_rc_evidence"' EXIT
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

cat > "$tmp_auth_status" <<'JSON'
{
  "schema": "kiana.auth-status.v1",
  "api_key": "missing",
  "source": "none",
  "config_file": "/workspace/config.toml",
  "oauth": {
    "status": "missing",
    "access_token": "missing",
    "refresh_token": "missing",
    "file": "/workspace/oauth.json",
    "store": "file",
    "expires_at": null,
    "expired": false,
    "expiring": false,
    "refreshable": false,
    "error": null
  },
  "providers": [
    {
      "provider_id": "anthropic",
      "display_name": "Anthropic",
      "auth": "api_key",
      "status": "missing",
      "auth_source": "none",
      "key_preview": "missing",
      "base_url": null,
      "model_id": "claude-sonnet-4-20250514",
      "default_model_id": "claude-sonnet-4-20250514",
      "protocol": "anthropic_messages",
      "models_source": "static_table",
      "requires_live_smoke": true
    },
    {
      "provider_id": "ollama",
      "display_name": "Ollama",
      "auth": "not_required",
      "status": "configured",
      "auth_source": "not_required",
      "key_preview": "not_required",
      "base_url": "http://127.0.0.1:11434",
      "model_id": "llama3.1",
      "default_model_id": "llama3.1",
      "protocol": "ollama_chat",
      "models_source": "local_service",
      "requires_live_smoke": true
    }
  ]
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-auth-status.v1.schema.json \
  "$tmp_auth_status" >/dev/null

cat > "$tmp_context_index" <<'JSON'
{
  "schema": "kiana.context-index.v1",
  "root": "/workspace",
  "files_indexed": 1,
  "skipped_files": 0,
  "total_bytes": 21,
  "files": [
    {
      "path": "src/lib.rs",
      "language": "rust",
      "bytes": 21,
      "line_count": 1,
      "content_hash": "0123456789abcdef"
    }
  ],
  "cache": {
    "path": "/workspace/.kiana/context-index.json",
    "status": "recovered",
    "reused_files": 0,
    "added_files": 1,
    "changed_files": 0,
    "removed_files": 0
  }
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-context-index.v1.schema.json \
  "$tmp_context_index" >/dev/null

cat > "$tmp_repo_map" <<'JSON'
{
  "schema": "kiana.repo-map.v1",
  "root": "/workspace",
  "token_budget": 1000,
  "estimated_tokens": 24,
  "truncated": false,
  "omitted_files": 0,
  "files": [
    {
      "path": "src/lib.rs",
      "language": "rust",
      "bytes": 42,
      "estimated_tokens": 16,
      "symbols": ["struct Widget", "fn render"]
    }
  ]
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-repo-map.v1.schema.json \
  "$tmp_repo_map" >/dev/null

cat > "$tmp_diff" <<'JSON'
{
  "schema": "kiana.diff.v1",
  "root": "/workspace",
  "inside_git_repo": true,
  "dirty": true,
  "files": [
    {
      "path": "src/lib.rs",
      "index": "M",
      "worktree": " "
    },
    {
      "path": "review-notes.txt",
      "index": "?",
      "worktree": "?"
    }
  ],
  "staged": {
    "changed": true,
    "stat": " src/lib.rs | 1 +"
  },
  "unstaged": {
    "changed": false,
    "stat": ""
  }
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-diff.v1.schema.json \
  "$tmp_diff" >/dev/null

cat > "$tmp_checkpoint" <<'JSON'
{
  "schema": "kiana.checkpoint.v1",
  "id": "checkpoint-1",
  "root": "/workspace",
  "git_root": "/workspace",
  "checkpoint_dir": "/home/user/.kiana/checkpoints/workspace/checkpoint-1",
  "manifest_path": "/home/user/.kiana/checkpoints/workspace/checkpoint-1/manifest.json",
  "inside_git_repo": true,
  "dirty": true,
  "head": "0123456789abcdef0123456789abcdef01234567",
  "branch": "main",
  "kind": "manual",
  "created_at_unix_ms": 1783123200000,
  "files": [
    {
      "path": "review-notes.txt",
      "index": "?",
      "worktree": "?"
    }
  ],
  "staged_patch": {
    "changed": false,
    "path": "/home/user/.kiana/checkpoints/workspace/checkpoint-1/staged.diff",
    "bytes": 0
  },
  "unstaged_patch": {
    "changed": true,
    "path": "/home/user/.kiana/checkpoints/workspace/checkpoint-1/unstaged.diff",
    "bytes": 42
  },
  "untracked_files": ["review-notes.txt"]
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-checkpoint.v1.schema.json \
  "$tmp_checkpoint" >/dev/null

cat > "$tmp_checks_dry_run" <<'JSON'
{
  "schema": "kiana.checks.dry_run.v1",
  "root": "/workspace",
  "inside_git_repo": true,
  "dry_run": true,
  "checks": [
    {
      "id": "rustfmt",
      "description": "Rust formatting",
      "command": "cargo fmt --all --check"
    },
    {
      "id": "cargo_check",
      "description": "Rust workspace compilation",
      "command": "cargo check --workspace"
    }
  ]
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-checks-dry-run.v1.schema.json \
  "$tmp_checks_dry_run" >/dev/null

cat > "$tmp_checks_run" <<'JSON'
{
  "schema": "kiana.checks.run.v1",
  "root": "/workspace",
  "git_root": "/workspace",
  "inside_git_repo": true,
  "dry_run": false,
  "execution": {
    "isolation": "git_worktree",
    "applied_current_changes": true
  },
  "summary": {
    "total": 1,
    "passed": 1,
    "failed": 0,
    "skipped": 0
  },
  "results": [
    {
      "id": "release_smoke",
      "description": "Release smoke gate",
      "command": "bash scripts/release-smoke.sh",
      "status": "passed",
      "exit_code": 0,
      "stdout": "smoke-ok\n",
      "stderr": "",
      "error": null
    }
  ]
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-checks-run.v1.schema.json \
  "$tmp_checks_run" >/dev/null

cat > "$tmp_review_dry_run" <<'JSON'
{
  "schema": "kiana.review.dry_run.v1",
  "root": "/workspace",
  "git_root": "/workspace",
  "inside_git_repo": true,
  "dry_run": true,
  "dirty": true,
  "head": "0123456789abcdef0123456789abcdef01234567",
  "branch": "main",
  "planned_steps": [
    "create_isolated_worktree",
    "apply_current_patch",
    "run_configured_checks",
    "produce_review_findings",
    "discard_isolated_worktree"
  ],
  "files": [
    {
      "path": "src/lib.rs",
      "index": "M",
      "worktree": " "
    }
  ],
  "patches": {
    "staged": {
      "changed": true,
      "bytes": 42,
      "text": "diff --git a/src/lib.rs b/src/lib.rs\n"
    },
    "unstaged": {
      "changed": false,
      "bytes": 0,
      "text": ""
    }
  }
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-review-dry-run.v1.schema.json \
  "$tmp_review_dry_run" >/dev/null

cat > "$tmp_review_run" <<'JSON'
{
  "schema": "kiana.review.run.v1",
  "root": "/workspace",
  "git_root": "/workspace",
  "inside_git_repo": true,
  "dry_run": false,
  "dirty": true,
  "head": "0123456789abcdef0123456789abcdef01234567",
  "branch": "main",
  "files": [
    {
      "path": "src/lib.rs",
      "index": " ",
      "worktree": "M"
    }
  ],
  "patches": {
    "staged": {
      "changed": false,
      "bytes": 0,
      "text": ""
    },
    "unstaged": {
      "changed": true,
      "bytes": 42,
      "text": "diff --git a/src/lib.rs b/src/lib.rs\n"
    }
  },
  "checks": {
    "schema": "kiana.checks.run.v1",
    "root": "/workspace",
    "git_root": "/workspace",
    "inside_git_repo": true,
    "dry_run": false,
    "execution": {
      "isolation": "git_worktree",
      "applied_current_changes": true
    },
    "summary": {
      "total": 1,
      "passed": 1,
      "failed": 0,
      "skipped": 0
    },
    "results": [
      {
        "id": "release_smoke",
        "description": "Release smoke gate",
        "command": "bash scripts/release-smoke.sh",
        "status": "passed",
        "exit_code": 0,
        "stdout": "smoke-ok\n",
        "stderr": "",
        "error": null
      }
    ]
  },
  "findings": []
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-review-run.v1.schema.json \
  "$tmp_review_run" >/dev/null

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

cat > "$tmp_source_control" <<'JSON'
{
  "schema": "kiana.source-control-proof.v1",
  "version": "0.1.0",
  "status": "accepted",
  "accepted": true,
  "accepted_by": "release manager",
  "accepted_at": "2026-01-01T00:00:00Z",
  "remote_url": "https://github.com/acme/kiana.git",
  "commit": "0123456789abcdef0123456789abcdef01234567",
  "release_tag": "v0.1.0",
  "tagged_commit": "0123456789abcdef0123456789abcdef01234567",
  "pushed": true,
  "reviewed": true
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-source-control-proof.v1.schema.json \
  "$tmp_source_control" >/dev/null

cat > "$tmp_enterprise_offline_manifest" <<'JSON'
{
  "schema": "kiana.enterprise.offline-manifest.v1",
  "version": "0.1.0",
  "release_base_url": "https://github.com/acme/kiana/releases/download/v0.1.0",
  "artifacts": [
    {
      "target": "linux-x86_64",
      "archive": "kiana-0.1.0-linux-x86_64.tar.gz",
      "url": "https://github.com/acme/kiana/releases/download/v0.1.0/kiana-0.1.0-linux-x86_64.tar.gz",
      "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
      "binary_sha256": "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
      "local_path": "kiana-0.1.0-linux-x86_64.tar.gz",
      "checksum_path": "kiana-0.1.0-linux-x86_64.tar.gz.sha256",
      "binary_checksum_path": "kiana-0.1.0-linux-x86_64.binary.sha256"
    }
  ],
  "channels": {
    "github_releases": "generated_from_release_base_url",
    "homebrew": "generated",
    "winget": "generated"
  },
  "generated_by": "scripts/generate-distribution-manifests.sh"
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-enterprise-offline-manifest.v1.schema.json \
  "$tmp_enterprise_offline_manifest" >/dev/null

cat > "$tmp_local_rc_evidence" <<'JSON'
{
  "schema": "kiana.local-rc-evidence.v1",
  "version": "0.1.0",
  "generated_at": "2026-07-04T00:00:00Z",
  "status": "local_rc_ready",
  "dist_dir": "dist",
  "summary": {
    "release_artifacts": 1,
    "manifests": 2,
    "proofs": 3,
    "blockers_total": 12,
    "local_blockers": 0,
    "external_blockers": 12
  },
  "release_artifacts": [
    {
      "target": "linux-x86_64",
      "archive": "dist/kiana-0.1.0-linux-x86_64.tar.gz",
      "archive_sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
      "binary_sha256": "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
      "lifecycle_smoke": "passed"
    }
  ],
  "distribution_manifests": {
    "enterprise_offline_manifest": "dist/manifests/enterprise/offline-manifest.json",
    "homebrew_formulae": ["dist/manifests/homebrew/kiana-linux-x86_64.rb"],
    "winget_manifests": [],
    "blocked_channels": ["dist/manifests/winget/BLOCKED.md"]
  },
  "proofs": [
    {
      "path": "dist/proofs/product/product-acceptance-local-rc.json",
      "schema": "kiana.product-acceptance.v1",
      "status": "local_rc",
      "accepted": false
    }
  ],
  "blockers": {
    "status": "blocked",
    "blocking": 12,
    "local_blocking": 0,
    "external_blocking": 12,
    "blocking_ids": ["source.remote", "source.version-tag"]
  }
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-local-rc-evidence.v1.schema.json \
  "$tmp_local_rc_evidence" >/dev/null

tmp_report="$(mktemp)"
tmp_handoff="$(mktemp)"
trap 'rm -f "$tmp_runtime_event" "$tmp_app_events" "$tmp_auth_status" "$tmp_context_index" "$tmp_repo_map" "$tmp_diff" "$tmp_checkpoint" "$tmp_checks_dry_run" "$tmp_checks_run" "$tmp_review_dry_run" "$tmp_review_run" "$tmp_proof_manifest" "$tmp_source_control" "$tmp_enterprise_offline_manifest" "$tmp_doctor" "$tmp_local_rc_evidence" "$tmp_report" "$tmp_handoff"' EXIT
bash scripts/commercial-release-blockers-report.sh --json --handoff-md "$tmp_handoff" > "$tmp_report"
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-commercial-release-blockers.v1.schema.json \
  "$tmp_report" >/dev/null
"$python" - "$tmp_report" "$tmp_handoff" <<'PY'
import json
import sys
from pathlib import Path

report = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
handoff = Path(sys.argv[2]).read_text(encoding="utf-8")
checks = report.get("checks", [])
if not checks:
    raise SystemExit("commercial blockers report has no checks")
for check in checks:
    for key in ["owner", "owner_status", "acceptance_artifacts", "verification_commands", "handoff_notes"]:
        if key not in check:
            raise SystemExit(f"commercial blockers report missing {key}")
if "## Blocking Assignments" not in handoff:
    raise SystemExit("commercial blockers handoff is missing assignment section")
if "source.remote" not in handoff:
    raise SystemExit("commercial blockers handoff is missing source.remote")
PY
bash scripts/commercial-release-handoff-smoke.sh >/dev/null

echo "schema contract smoke passed"
