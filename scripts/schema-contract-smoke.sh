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
tmp_runtime_result="$(mktemp)"
tmp_app_events="$(mktemp)"
tmp_app_command_run="$(mktemp)"
tmp_app_permissions_status="$(mktemp)"
tmp_app_trust_status="$(mktemp)"
tmp_app_trust_invalid_dir="$(mktemp -d)"
tmp_app_team_status="$(mktemp)"
tmp_team_plan="$(mktemp)"
tmp_tasks="$(mktemp)"
tmp_auth_status="$(mktemp)"
tmp_context_index="$(mktemp)"
tmp_context_vector_search="$(mktemp)"
tmp_context_artifacts="$(mktemp)"
tmp_context_artifact_ingest="$(mktemp)"
tmp_context_artifact_graph="$(mktemp)"
tmp_context_artifact_store="$(mktemp)"
tmp_context_artifact_readiness="$(mktemp)"
tmp_repo_map="$(mktemp)"
tmp_diff="$(mktemp)"
tmp_checkpoint="$(mktemp)"
tmp_checks_dry_run="$(mktemp)"
tmp_checks_run="$(mktemp)"
tmp_review_dry_run="$(mktemp)"
tmp_review_run="$(mktemp)"
tmp_proof_manifest="$(mktemp)"
tmp_source_control="$(mktemp)"
tmp_release_workflow_proof="$(mktemp)"
tmp_release_signature="$(mktemp)"
tmp_enterprise_offline_manifest="$(mktemp)"
tmp_distribution_review="$(mktemp)"
tmp_platform_security="$(mktemp)"
tmp_doctor="$(mktemp)"
tmp_local_rc_evidence="$(mktemp)"
tmp_managed_plugin_policy="$(mktemp)"
tmp_plugin_app_manifest="$(mktemp)"
tmp_memory_record="$(mktemp)"
tmp_memory_status="$(mktemp)"
tmp_memory_search="$(mktemp)"
tmp_eda_review="$(mktemp)"
tmp_eda_netlist_review="$(mktemp)"
tmp_eval_baseline="$(mktemp)"
tmp_eval_report="$(mktemp)"
tmp_swarm_process_identity_backend="$(mktemp)"
tmp_swarm_worker_telemetry="$(mktemp)"
tmp_swarm_worker_health="$(mktemp)"
tmp_swarm_worker_state="$(mktemp)"
tmp_eda_invalid_dir="$(mktemp -d)"
trap 'rm -f "$tmp_runtime_event" "$tmp_runtime_result" "$tmp_app_events" "$tmp_app_command_run" "$tmp_app_permissions_status" "$tmp_app_trust_status" "$tmp_app_team_status" "$tmp_team_plan" "$tmp_tasks" "$tmp_auth_status" "$tmp_context_index" "$tmp_context_vector_search" "$tmp_context_artifacts" "$tmp_context_artifact_ingest" "$tmp_context_artifact_graph" "$tmp_context_artifact_store" "$tmp_context_artifact_readiness" "$tmp_repo_map" "$tmp_diff" "$tmp_checkpoint" "$tmp_checks_dry_run" "$tmp_checks_run" "$tmp_review_dry_run" "$tmp_review_run" "$tmp_proof_manifest" "$tmp_source_control" "$tmp_release_workflow_proof" "$tmp_release_signature" "$tmp_enterprise_offline_manifest" "$tmp_distribution_review" "$tmp_platform_security" "$tmp_doctor" "$tmp_local_rc_evidence" "$tmp_managed_plugin_policy" "$tmp_plugin_app_manifest" "$tmp_memory_record" "$tmp_memory_status" "$tmp_memory_search" "$tmp_eda_review" "$tmp_eda_netlist_review" "$tmp_eval_baseline" "$tmp_eval_report" "$tmp_swarm_process_identity_backend" "$tmp_swarm_worker_telemetry" "$tmp_swarm_worker_health" "$tmp_swarm_worker_state"; rm -rf "$tmp_app_trust_invalid_dir" "$tmp_eda_invalid_dir"' EXIT
cat > "$tmp_memory_record" <<'JSON'
{
  "schema": "kiana.memory-record.v1",
  "id": "mem-1783720000000-0123456789abcdef",
  "kind": "decision",
  "source": "workflow",
  "text": "Use bounded swarm retry policy for isolated workers.",
  "redaction_count": 0,
  "created_at_ms": 1783720000000
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-memory-record.v1.schema.json \
  "$tmp_memory_record" >/dev/null
cat > "$tmp_memory_status" <<'JSON'
{
  "schema": "kiana.memory-status.v1",
  "legacy_file": "/tmp/kiana/memory.md",
  "legacy_bytes": 42,
  "store_file": "/tmp/kiana/memory/events.jsonl",
  "record_count": 1,
  "kind_counts": {
    "decision": 1
  },
  "redaction_count": 0,
  "latest_created_at_ms": 1783720000000
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-memory-status.v1.schema.json \
  "$tmp_memory_status" >/dev/null
cat > "$tmp_memory_search" <<'JSON'
{
  "schema": "kiana.memory-search.v1",
  "store_file": "/tmp/kiana/memory/events.jsonl",
  "query": "swarm retry",
  "terms": ["swarm", "retry"],
  "limit": 10,
  "record_count": 1,
  "hits": [
    {
      "id": "mem-1783720000000-0123456789abcdef",
      "kind": "decision",
      "source": "workflow",
      "score": 2,
      "matched_terms": ["swarm", "retry"],
      "text": "Use bounded swarm retry policy for isolated workers.",
      "redaction_count": 0,
      "created_at_ms": 1783720000000
    }
  ]
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-memory-search.v1.schema.json \
  "$tmp_memory_search" >/dev/null

cat > "$tmp_release_workflow_proof" <<'JSON'
{
  "schema": "kiana.release-workflow-proof.v1",
  "run_id": "run_release_001",
  "workflow_id": "wf_release_001",
  "proof_path": "dist/proofs/workflow/recovery-integrity.json",
  "proof_schema": "kiana.workflow-integrity-report.v1",
  "proof_status": "verified",
  "selection": {
    "mode": "explicit"
  },
  "release_binding_schema": "kiana.workflow-release-binding.v1",
  "env": {
    "KIANA_RELEASE_WORKFLOW_RUN_ID": "run_release_001",
    "KIANA_WORKFLOW_RECOVERY_INTEGRITY_PROOF_FILE": "dist/proofs/workflow/recovery-integrity.json"
  }
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-release-workflow-proof.v1.schema.json \
  "$tmp_release_workflow_proof" >/dev/null

tmp_commercial_blockers="$(mktemp)"
bash scripts/commercial-release-blockers-report.sh --json > "$tmp_commercial_blockers"
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-commercial-release-blockers.v1.schema.json \
  "$tmp_commercial_blockers" >/dev/null
"$python" - "$tmp_commercial_blockers" <<'PY'
import json
import sys
from pathlib import Path

report = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
action_plan = report.get("action_plan", {})
if action_plan.get("schema") != "kiana.commercial-release-action-plan.v1":
    raise SystemExit("commercial blocker action_plan schema mismatch")
if action_plan.get("total_actions") != report.get("summary", {}).get("blocking"):
    raise SystemExit("commercial blocker action_plan total does not match blocking summary")
PY

cat > "$tmp_eval_baseline" <<'JSON'
{
  "schema": "kiana.eval-baseline.v1",
  "suite_id": "basic-runtime",
  "description": "Schema smoke baseline",
  "cases": {
    "tool-success": {
      "max_event_count": 5,
      "max_tool_call_count": 1,
      "max_tool_result_count": 1,
      "max_tool_error_count": 0,
      "max_input_tokens": 32,
      "max_output_tokens": 16,
      "required_status": "completed",
      "required_stop_reason": "end_turn"
    }
  }
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-eval-baseline.v1.schema.json \
  "$tmp_eval_baseline" >/dev/null

cat > "$tmp_eval_report" <<'JSON'
{
  "schema": "kiana.eval-report.v1",
  "suite_id": "basic-runtime",
  "suite_sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
  "status": "passed",
  "summary": {
    "total": 1,
    "passed": 1,
    "failed": 0,
    "events": 5,
    "tool_calls": 1,
    "tool_errors": 0
  },
  "baseline": {
    "schema": "kiana.eval-baseline.v1",
    "provided": true,
    "path": "basic-runtime-baseline.json",
    "sha256": "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
    "suite_id": "basic-runtime",
    "status": "passed",
    "findings": []
  },
  "cases": [
    {
      "id": "tool-success",
      "kind": "runtime_event_replay",
      "fixture": "basic-tool-success.jsonl",
      "fixture_sha256": "1111111111111111111111111111111111111111111111111111111111111111",
      "status": "passed",
      "metrics": {
        "event_count": 5,
        "event_type_counts": {
          "assistant": 1,
          "tool_call": 1,
          "tool_result": 1,
          "usage": 1,
          "result": 1
        },
        "assistant_text_count": 2,
        "tool_call_count": 1,
        "tool_result_count": 1,
        "tool_error_count": 0,
        "tool_names": ["Read"],
        "input_tokens": 12,
        "output_tokens": 4,
        "final_status": "completed",
        "stop_reason": "end_turn",
        "final_text": "done"
      },
      "findings": []
    }
  ]
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-eval-report.v1.schema.json \
  "$tmp_eval_report" >/dev/null
cat > "$tmp_swarm_worker_state" <<'JSON'
{
  "schema": "kiana.swarm-worker-state.v2",
  "worker_id": "worker-api-1",
  "dispatch_id": "swarm-dispatch-0123456789abcdef",
  "task_id": "api",
  "status": "running",
  "pid": 4242,
  "process_identity": {
    "schema": "kiana.swarm-process-identity.v1",
    "platform": "linux_procfs",
    "pid": 4242,
    "process_group_id": 4242,
    "start_time_ticks": 987654321,
    "command_sha256": "sha256:0000000000000000000000000000000000000000000000000000000000000000"
  },
  "process_identity_status": "verified",
  "process_identity_reason": null,
  "attempt": 1,
  "started_at_ms": 1783720000000,
  "finished_at_ms": null,
  "exit_code": null,
  "termination_reason": null,
  "output_bytes": 0,
  "result_path": null,
  "updated_at_ms": 1783720001000
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-swarm-worker-state.v2.schema.json \
  "$tmp_swarm_worker_state" >/dev/null
cat > "$tmp_swarm_process_identity_backend" <<'JSON'
{
  "schema": "kiana.swarm-process-identity-backend.v1",
  "platform": "linux",
  "backend": "linux_procfs",
  "supported": true,
  "safe_to_start_workers": true,
  "safe_to_monitor_workers": true,
  "safe_to_cancel_workers": true,
  "identity_fields": ["pid", "process_group_id", "start_time_ticks", "command_sha256"],
  "continuity_fields": ["pid", "process_group_id", "start_time_ticks"],
  "provenance_fields": ["command_sha256"],
  "unsupported_reason": null,
  "notes": [
    "command_sha256 is provenance only and is not used as process continuity evidence",
    "workers must be their own process group leaders before cancellation is allowed"
  ]
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-swarm-process-identity-backend.v1.schema.json \
  "$tmp_swarm_process_identity_backend" >/dev/null
cat > "$tmp_swarm_worker_telemetry" <<'JSON'
{
  "schema": "kiana.swarm-worker-telemetry.v1",
  "provided": true,
  "command_count": 2,
  "commands_run": ["cargo test", "cargo fmt"],
  "invalid_command_entries": 0,
  "raw_sha256": "sha256:3333333333333333333333333333333333333333333333333333333333333333",
  "notes": []
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-swarm-worker-telemetry.v1.schema.json \
  "$tmp_swarm_worker_telemetry" >/dev/null
cat > "$tmp_swarm_worker_health" <<'JSON'
{
  "schema": "kiana.swarm-worker-health.v1",
  "state": "retrying",
  "reason": "retrying_after_worker_failed",
  "next_action": "run_swarm_monitor",
  "process": {
    "pid": 4243,
    "identity_status": "verified",
    "identity_reason": null
  },
  "attempt": {
    "current": 2,
    "max": 3,
    "remaining": 1,
    "retrying": true
  },
  "budget": {
    "output_bytes": 0,
    "max_output_bytes": 10485760,
    "output_bytes_remaining": 10485760,
    "commands_run": 0,
    "max_commands": 20,
    "commands_remaining": 20,
    "elapsed_ms": 0,
    "timeout_ms": 1800000,
    "timeout_ms_remaining": 1800000
  },
  "scope": {
    "changed_files": 0,
    "scope_deviations": 0,
    "project_violation_reason": null
  }
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-swarm-worker-health.v1.schema.json \
  "$tmp_swarm_worker_health" >/dev/null
cat > "$tmp_eda_review" <<'JSON'
{
  "schema": "kiana.eda-review.v1",
  "rule_version": "eda-review-rules.v1",
  "workflow_id": "wf-abc123",
  "run_id": "run-1783700000000-abc123",
  "review_id": "eda-0123456789abcdefabcd",
  "created_at_ms": 1783700000000,
  "status": "pass",
  "sources": [
    {
      "kind": "schematic",
      "path": "hardware/main.kicad_sch",
      "size": 64,
      "sha256": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
      "media_type": "application/x-kicad-schematic"
    },
    {
      "kind": "bom",
      "path": "hardware/bom.csv",
      "size": 128,
      "sha256": "sha256:1111111111111111111111111111111111111111111111111111111111111111",
      "media_type": "text/csv"
    },
    {
      "kind": "gerber",
      "path": "hardware/gerber",
      "size": 256,
      "sha256": "sha256:2222222222222222222222222222222222222222222222222222222222222222",
      "media_type": "application/vnd.gerber"
    }
  ],
  "checks": [
    {
      "check_id": "schematic_presence",
      "status": "pass",
      "summary": "Schematic artifact presence and digest",
      "evidence": ["hardware/main.kicad_sch"]
    }
  ],
  "findings": [],
  "summary": {
    "blocked": 0,
    "errors": 0,
    "warnings": 0,
    "info": 0,
    "bom_rows": 2,
    "cpl_rows": 2,
    "gerber_files": 3
  },
  "limitations": ["No electrical sign-off is performed."],
  "approval_requirements": ["hardware_order"],
  "next_action": "engineer_review_then_bringup",
  "artifacts": [
    "eda/reviews/eda-0123456789abcdefabcd/eda_review.json",
    "eda/reviews/eda-0123456789abcdefabcd/bom_risk.md",
    "eda/reviews/eda-0123456789abcdefabcd/bringup-plan.md"
  ]
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-eda-review.v1.schema.json \
  "$tmp_eda_review" >/dev/null
"$python" - "$tmp_eda_review" "$tmp_eda_netlist_review" <<'PY'
import json
import sys
from pathlib import Path

source = Path(sys.argv[1])
output = Path(sys.argv[2])
document = json.loads(source.read_text(encoding="utf-8"))
document["rule_version"] = "eda-review-rules.v2"
document["sources"].append({
    "kind": "netlist",
    "path": "hardware/main.xml",
    "size": 512,
    "sha256": "sha256:3333333333333333333333333333333333333333333333333333333333333333",
    "media_type": "application/xml",
})
document["checks"].append({
    "check_id": "netlist_structure",
    "status": "pass",
    "summary": "KiCad XML netlist structure",
    "evidence": ["hardware/main.xml"],
})
document["summary"].update({
    "netlist_components": 2,
    "net_count": 2,
    "power_net_count": 1,
    "interface_net_count": 1,
    "dangling_net_count": 0,
})
output.write_text(json.dumps(document, indent=2) + "\n", encoding="utf-8")
PY
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-eda-review.v1.schema.json \
  "$tmp_eda_netlist_review" >/dev/null
"$python" - "$tmp_eda_review" "$tmp_eda_netlist_review" "$tmp_eda_invalid_dir" <<'PY'
import copy
import json
import sys
from pathlib import Path

source = Path(sys.argv[1])
netlist_source = Path(sys.argv[2])
output_dir = Path(sys.argv[3])
base = json.loads(source.read_text(encoding="utf-8"))
netlist_base = json.loads(netlist_source.read_text(encoding="utf-8"))

variants = {}

missing_gerber = copy.deepcopy(base)
missing_gerber["sources"] = [item for item in missing_gerber["sources"] if item["kind"] != "gerber"]
variants["pass-missing-gerber.json"] = missing_gerber

pass_blocked = copy.deepcopy(base)
pass_blocked["summary"]["blocked"] = 1
variants["pass-with-blocked.json"] = pass_blocked

pass_errors = copy.deepcopy(base)
pass_errors["summary"]["errors"] = 1
variants["pass-with-errors.json"] = pass_errors

blocked_without_blocker = copy.deepcopy(base)
blocked_without_blocker["status"] = "blocked"
blocked_without_blocker["next_action"] = "supply_required_artifacts"
variants["blocked-with-zero-blocked.json"] = blocked_without_blocker

review_without_error = copy.deepcopy(base)
review_without_error["status"] = "review_required"
review_without_error["next_action"] = "resolve_findings_and_rerun"
variants["review-required-with-zero-errors.json"] = review_without_error

netlist_without_check = copy.deepcopy(netlist_base)
netlist_without_check["checks"] = [
    item for item in netlist_without_check["checks"]
    if item["check_id"] != "netlist_structure"
]
variants["netlist-without-check.json"] = netlist_without_check

netlist_bad_media = copy.deepcopy(netlist_base)
next(item for item in netlist_bad_media["sources"] if item["kind"] == "netlist")["media_type"] = "text/plain"
variants["netlist-with-bad-media-type.json"] = netlist_bad_media

pass_with_netlist_error = copy.deepcopy(netlist_base)
pass_with_netlist_error["findings"].append({
    "code": "netlist_component_missing_from_bom",
    "severity": "error",
    "message": "Netlist component J1 is missing from the BOM.",
    "evidence": ["J1"],
    "recommendation": "Regenerate BOM and netlist from one revision.",
    "designator": "J1",
})
variants["pass-with-netlist-error.json"] = pass_with_netlist_error

v2_missing_summary = copy.deepcopy(netlist_base)
del v2_missing_summary["summary"]["net_count"]
variants["v2-missing-netlist-summary.json"] = v2_missing_summary

for name, document in variants.items():
    (output_dir / name).write_text(json.dumps(document, indent=2) + "\n", encoding="utf-8")
PY
"$python" - \
  docs/schemas/kiana-eda-review.v1.schema.json \
  "$tmp_eda_review" \
  "$tmp_eda_netlist_review" \
  "$tmp_eda_invalid_dir" <<'PY'
import json
import sys
from pathlib import Path

schema = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
valid = json.loads(Path(sys.argv[2]).read_text(encoding="utf-8"))
valid_netlist = json.loads(Path(sys.argv[3]).read_text(encoding="utf-8"))
invalid_dir = Path(sys.argv[4])


def status_rule(status):
    for rule in schema.get("allOf", []):
        if rule.get("if", {}).get("properties", {}).get("status", {}).get("const") == status:
            return rule.get("then", {})
    raise SystemExit(f"EDA schema is missing the {status!r} conditional rule")


pass_rule = status_rule("pass")
pass_summary = pass_rule.get("properties", {}).get("summary", {}).get("properties", {})
if pass_summary.get("blocked", {}).get("const") != 0:
    raise SystemExit("EDA pass schema must require summary.blocked=0")
if pass_summary.get("errors", {}).get("const") != 0:
    raise SystemExit("EDA pass schema must require summary.errors=0")

source_rules = pass_rule.get("properties", {}).get("sources", {}).get("allOf", [])
required_source_kinds = {
    rule.get("contains", {}).get("properties", {}).get("kind", {}).get("const")
    for rule in source_rules
    if rule.get("minContains") == 1
}
if required_source_kinds != {"schematic", "bom", "gerber"}:
    raise SystemExit(f"EDA pass schema has wrong required sources: {sorted(required_source_kinds)}")

blocked_rule = status_rule("blocked")
blocked_minimum = blocked_rule.get("properties", {}).get("summary", {}).get("properties", {}).get("blocked", {}).get("minimum")
if blocked_minimum != 1:
    raise SystemExit("EDA blocked schema must require summary.blocked>=1")

review_rule = status_rule("review_required")
error_minimum = review_rule.get("properties", {}).get("summary", {}).get("properties", {}).get("errors", {}).get("minimum")
if error_minimum != 1:
    raise SystemExit("EDA review_required schema must require summary.errors>=1")


def semantic_errors(document):
    status = document.get("status")
    summary = document.get("summary", {})
    errors = []
    sources = document.get("sources", [])
    source_kinds = {item.get("kind") for item in sources}
    if "netlist" in source_kinds:
        netlist_sources = [item for item in sources if item.get("kind") == "netlist"]
        if any(item.get("media_type") != "application/xml" for item in netlist_sources):
            errors.append("netlist requires application/xml")
        check_ids = {item.get("check_id") for item in document.get("checks", [])}
        if "netlist_structure" not in check_ids:
            errors.append("netlist requires netlist_structure check")
    if document.get("rule_version") == "eda-review-rules.v2":
        for key in (
            "netlist_components",
            "net_count",
            "power_net_count",
            "interface_net_count",
            "dangling_net_count",
        ):
            if not isinstance(summary.get(key), int) or summary[key] < 0:
                errors.append(f"rules.v2 requires non-negative summary.{key}")
    if status == "pass":
        if summary.get("blocked") != 0:
            errors.append("pass requires blocked=0")
        if summary.get("errors") != 0:
            errors.append("pass requires errors=0")
        missing = {"schematic", "bom", "gerber"} - source_kinds
        if missing:
            errors.append(f"pass is missing sources: {sorted(missing)}")
        if any(item.get("severity") in {"blocked", "error"} for item in document.get("findings", [])):
            errors.append("pass cannot contain blocking or error findings")
    elif status == "blocked" and not isinstance(summary.get("blocked"), int):
        errors.append("blocked requires integer summary.blocked")
    elif status == "blocked" and summary["blocked"] < 1:
        errors.append("blocked requires blocked>=1")
    elif status == "review_required" and not isinstance(summary.get("errors"), int):
        errors.append("review_required requires integer summary.errors")
    elif status == "review_required" and summary["errors"] < 1:
        errors.append("review_required requires errors>=1")
    return errors


if semantic_errors(valid):
    raise SystemExit(f"valid EDA fixture failed semantic contract: {semantic_errors(valid)}")
if semantic_errors(valid_netlist):
    raise SystemExit(
        f"valid EDA netlist fixture failed semantic contract: {semantic_errors(valid_netlist)}"
    )
for path in sorted(invalid_dir.glob("*.json")):
    document = json.loads(path.read_text(encoding="utf-8"))
    if not semantic_errors(document):
        raise SystemExit(f"EDA negative fixture is not invalid: {path.name}")
PY
for invalid_eda_review in \
  "$tmp_eda_invalid_dir/pass-missing-gerber.json" \
  "$tmp_eda_invalid_dir/pass-with-blocked.json" \
  "$tmp_eda_invalid_dir/pass-with-errors.json" \
  "$tmp_eda_invalid_dir/blocked-with-zero-blocked.json" \
  "$tmp_eda_invalid_dir/review-required-with-zero-errors.json" \
  "$tmp_eda_invalid_dir/netlist-without-check.json" \
  "$tmp_eda_invalid_dir/netlist-with-bad-media-type.json" \
  "$tmp_eda_invalid_dir/pass-with-netlist-error.json" \
  "$tmp_eda_invalid_dir/v2-missing-netlist-summary.json"
do
  if "$python" scripts/validate-json-schema.py \
    docs/schemas/kiana-eda-review.v1.schema.json \
    "$invalid_eda_review" >/dev/null 2>&1; then
    echo "EDA schema unexpectedly accepted invalid fixture: $(basename "$invalid_eda_review")" >&2
    exit 1
  fi
done
cat > "$tmp_plugin_app_manifest" <<'JSON'
{
  "schema": "kiana.plugin-app-manifest.v1",
  "id": "review-workbench",
  "title": "Review Workbench",
  "entry": "apps/review/index.html",
  "routes": [
    {
      "path": "/review",
      "title": "Review"
    }
  ]
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-plugin-app-manifest.v1.schema.json \
  "$tmp_plugin_app_manifest" >/dev/null
cat > "$tmp_managed_plugin_policy" <<'JSON'
{
  "schema": "kiana.managed-plugin-policy.v1",
  "plugins": {
    "allow": ["review-tools@tools-marketplace"],
    "allowMarketplaces": ["tools-marketplace"],
    "requireSignature": true,
    "requireSignatureVerification": true,
    "signatureVerificationCommand": "test \"$KIANA_PLUGIN_SIGNATURE_VALUE\" = sig-ed25519-test"
  }
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-managed-plugin-policy.v1.schema.json \
  "$tmp_managed_plugin_policy" >/dev/null
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

cat > "$tmp_runtime_result" <<'JSON'
{
  "event_id": "evt-result",
  "session_id": "session-1",
  "turn_id": "turn-1",
  "sequence": 9,
  "timestamp": "2026-07-04T00:00:09Z",
  "type": "result",
  "status": "completed",
  "stop_reason": "model_stop",
  "assistant_text": "done",
  "metadata": {
    "iterations": 1
  }
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-runtime-event.v1.schema.json \
  "$tmp_runtime_result" >/dev/null

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
  "summary": {
    "turns": 1,
    "event_types": {
      "permission_request": 1
    },
    "tool_results": {
      "total": 0,
      "errors": 0
    },
    "file_changes": {
      "count": 0,
      "paths": []
    },
    "terminal": {
      "present": false,
      "status": null,
      "stop_reason": null
    }
  },
  "view": {
    "schema": "kiana.app-server.events-view.v1",
    "message_count": 1,
    "messages": [
      {
        "event_id": "evt-permission",
        "turn_id": "turn-1",
        "sequence": 3,
        "timestamp": "2026-07-02T00:00:03Z",
        "source_type": "permission_request",
        "role": "system",
        "content": "Permission requested for Bash.",
        "metadata": {
          "kind": "permission_request",
          "request_id": "perm-1",
          "tool_name": "Bash",
          "action": "run",
          "reason": "ask mode",
          "input": {
            "command": "git status"
          }
        }
      }
    ]
  },
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

cat > "$tmp_app_command_run" <<'JSON'
{
  "schema": "kiana.app-server.command-run.v1",
  "workspace": "/workspace",
  "command": {
    "name": "version",
    "slash": "/version",
    "description": "Show version",
    "command_type": "local",
    "enabled": true,
    "supports_non_interactive": true,
    "routes_to": "local_command",
    "source": {
      "kind": "core",
      "plugin": null
    }
  },
  "request": {
    "args": [],
    "arg_count": 0
  },
  "status": "ok",
  "executed": true,
  "output": {
    "output_type": "text",
    "value": "0.1.0",
    "metadata": null
  }
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-app-server-command-run.v1.schema.json \
  "$tmp_app_command_run" >/dev/null

cat > "$tmp_app_permissions_status" <<'JSON'
{
  "schema": "kiana.app-server.permissions-status.v1",
  "workspace": "/workspace",
  "profile": "commercial",
  "mode": "ask",
  "sources": ["file_profile", "managed_policy", "managed_profile"],
  "file": {
    "path": "/workspace/permissions.json",
    "status": "loaded",
    "loaded": true,
    "error": null
  },
  "managed_policy": {
    "path": "/workspace/managed-permissions.json",
    "status": "loaded",
    "loaded": true,
    "error": null
  },
  "rules": {
    "allowed_tools": ["Read"],
    "disallowed_tools": ["Bash"],
    "ask_tools": ["Edit"],
    "managed_allowed_tools": ["Glob"],
    "managed_disallowed_tools": ["Write"],
    "managed_ask_tools": ["Bash"]
  },
  "interactive_prompts": {
    "supported": true,
    "non_interactive_requires_allow_rule": true
  }
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-app-server-permissions-status.v1.schema.json \
  "$tmp_app_permissions_status" >/dev/null

cat > "$tmp_app_trust_status" <<'JSON'
{
  "schema": "kiana.app-server.trust-status.v1",
  "workspace": "/workspace",
  "project_trust": "unknown",
  "project_trusted": false,
  "allows_project_resources": false,
  "source": "default",
  "project_id": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
  "project_root": "/workspace",
  "file": {
    "path": "/home/test/.kiana/trust/projects/0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef.json",
    "status": "missing",
    "exists": false,
    "error": null
  },
  "legacy_project_file": {
    "path": "/workspace/.kiana/trust.json",
    "exists": true,
    "ignored": true,
    "reason": "project_local_trust_is_not_authoritative"
  }
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-app-server-trust-status.v1.schema.json \
  "$tmp_app_trust_status" >/dev/null
"$python" - "$tmp_app_trust_status" "$tmp_app_trust_invalid_dir" <<'PY'
import copy
import json
import sys
from pathlib import Path

source = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
output_dir = Path(sys.argv[2])

variants = {}

default_found = copy.deepcopy(source)
default_found["file"].update({"status": "found", "exists": True})
variants["default-with-found-record.json"] = default_found

user_store_without_path = copy.deepcopy(source)
user_store_without_path.update({
    "project_trust": "trusted",
    "project_trusted": True,
    "allows_project_resources": True,
    "source": "user_store",
})
user_store_without_path["file"].update({
    "path": None,
    "status": "found",
    "exists": True,
})
variants["user-store-without-path.json"] = user_store_without_path

missing_without_path = copy.deepcopy(source)
missing_without_path["file"]["path"] = None
variants["missing-without-path.json"] = missing_without_path

path_error_with_existing_file = copy.deepcopy(source)
path_error_with_existing_file["file"].update({
    "path": None,
    "status": "error",
    "exists": True,
    "error": "trust store path unavailable",
})
variants["path-error-with-existing-file.json"] = path_error_with_existing_file

for name, document in variants.items():
    (output_dir / name).write_text(json.dumps(document, indent=2) + "\n", encoding="utf-8")
PY
for invalid_trust_status in "$tmp_app_trust_invalid_dir"/*.json
do
  if "$python" scripts/validate-json-schema.py \
    docs/schemas/kiana-app-server-trust-status.v1.schema.json \
    "$invalid_trust_status" >/dev/null 2>&1; then
    echo "trust status schema unexpectedly accepted invalid fixture: $(basename "$invalid_trust_status")" >&2
    exit 1
  fi
done

cat > "$tmp_tasks" <<'JSON'
{
  "schema": "kiana.tasks.v1",
  "task_list_id": "default",
  "tasks_dir": "/workspace/.kiana/tasks/default",
  "count": 2,
  "status_counts": {
    "pending": 1,
    "completed": 1
  },
  "tasks": [
    {
      "id": "1",
      "title": "Review task status",
      "subject": "Review task status",
      "status": "pending",
      "owner": "planner",
      "blockedBy": []
    },
    {
      "id": "2",
      "title": "Ship app status",
      "subject": "Ship app status",
      "status": "completed",
      "owner": "builder",
      "blockedBy": []
    }
  ]
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-tasks.v1.schema.json \
  "$tmp_tasks" >/dev/null

cat > "$tmp_app_team_status" <<'JSON'
{
  "schema": "kiana.app-server.team-status.v1",
  "workspace": "/workspace",
  "team": {
    "name": "default",
    "source": "default_task_list"
  },
  "task_list": {
    "id": "default",
    "tasks_dir": "/workspace/.kiana/tasks/default",
    "count": 2,
    "status_counts": {
      "pending": 1,
      "completed": 1
    }
  },
  "tasks": {
    "schema": "kiana.tasks.v1",
    "task_list_id": "default",
    "tasks_dir": "/workspace/.kiana/tasks/default",
    "count": 2,
    "status_counts": {
      "pending": 1,
      "completed": 1
    },
    "tasks": [
      {
        "id": "1",
        "title": "Review task status",
        "subject": "Review task status",
        "status": "pending",
        "owner": "planner",
        "blockedBy": []
      },
      {
        "id": "2",
        "title": "Ship app status",
        "subject": "Ship app status",
        "status": "completed",
        "owner": "builder",
        "blockedBy": []
      }
    ]
  }
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-app-server-team-status.v1.schema.json \
  "$tmp_app_team_status" >/dev/null

cat > "$tmp_team_plan" <<'JSON'
{
  "schema": "kiana.team-plan.v1",
  "workspace": "/workspace",
  "team": {
    "name": "default",
    "source": "default_task_list"
  },
  "task_list": {
    "id": "default",
    "tasks_dir": "/workspace/.kiana/tasks/default",
    "count": 5,
    "status_counts": {
      "pending": 4,
      "completed": 1
    }
  },
  "role_runtime": {
    "status": "ready",
    "ready_for_fake_runtime": true,
    "required_roles": ["pm", "architect", "engineer", "qa", "data-analyst"],
    "missing_roles": [],
    "role_task_count": 5,
    "blocked_task_count": 1,
    "dependency_edge_count": 1
  },
  "roles": [
    {
      "id": "pm",
      "name": "PM",
      "aliases": ["pm", "product-manager", "product-owner"],
      "present": true,
      "task_count": 1,
      "open_task_count": 0,
      "completed_task_count": 1,
      "task_ids": ["1"]
    },
    {
      "id": "architect",
      "name": "Architect",
      "aliases": ["architect", "architecture", "tech-lead"],
      "present": true,
      "task_count": 1,
      "open_task_count": 1,
      "completed_task_count": 0,
      "task_ids": ["2"]
    },
    {
      "id": "engineer",
      "name": "Engineer",
      "aliases": ["engineer", "developer", "coder"],
      "present": true,
      "task_count": 1,
      "open_task_count": 1,
      "completed_task_count": 0,
      "task_ids": ["3"]
    },
    {
      "id": "qa",
      "name": "QA",
      "aliases": ["qa", "tester", "quality-assurance"],
      "present": true,
      "task_count": 1,
      "open_task_count": 1,
      "completed_task_count": 0,
      "task_ids": ["4"]
    },
    {
      "id": "data-analyst",
      "name": "Data Analyst",
      "aliases": ["data-analyst", "analyst", "data-scientist"],
      "present": true,
      "task_count": 1,
      "open_task_count": 1,
      "completed_task_count": 0,
      "task_ids": ["5"]
    }
  ],
  "artifact_readiness": {
    "status": "ready",
    "required_roles": [
      { "role": "prd", "present": true, "count": 1, "task_ids": ["1"] },
      { "role": "design", "present": true, "count": 1, "task_ids": ["2"] },
      { "role": "tasks", "present": true, "count": 1, "task_ids": ["3"] },
      { "role": "source", "present": true, "count": 1, "task_ids": ["4"] },
      { "role": "test", "present": true, "count": 1, "task_ids": ["5"] }
    ],
    "missing_roles": []
  },
  "tasks": {
    "schema": "kiana.tasks.v1",
    "task_list_id": "default",
    "tasks_dir": "/workspace/.kiana/tasks/default",
    "count": 0,
    "status_counts": {},
    "tasks": []
  }
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-team-plan.v1.schema.json \
  "$tmp_team_plan" >/dev/null

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

cat > "$tmp_context_vector_search" <<'JSON'
{
  "schema": "kiana.context-vector-search.v1",
  "root": "/workspace",
  "query": "checkout flow",
  "terms": ["checkout", "flow"],
  "embedding_model": "kiana.deterministic-hash-embedding.v1",
  "dimensions": 64,
  "limit": 1,
  "files_indexed": 1,
  "skipped_files": 0,
  "hits": [
    {
      "path": "src/lib.rs",
      "language": "rust",
      "content_hash": "0123456789abcdef",
      "score": 0.75,
      "token_overlap": 1,
      "line_number": 1,
      "line": "pub fn checkout_flow() {}"
    }
  ]
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-context-vector-search.v1.schema.json \
  "$tmp_context_vector_search" >/dev/null

cat > "$tmp_context_artifacts" <<'JSON'
{
  "schema": "kiana.context-artifacts.v1",
  "root": "/workspace",
  "files_indexed": 1,
  "skipped_files": 0,
  "artifacts": [
    {
      "id": "file:src/lib.rs:0123456789abcdef",
      "kind": "file",
      "path": "src/lib.rs",
      "language": "rust",
      "bytes": 21,
      "line_count": 1,
      "content_hash": "0123456789abcdef"
    }
  ],
  "cache": {
    "path": "/workspace/.kiana/context-artifacts.json",
    "status": "created",
    "reused_artifacts": 0,
    "added_artifacts": 1,
    "changed_artifacts": 0,
    "removed_artifacts": 0
  }
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-context-artifacts.v1.schema.json \
  "$tmp_context_artifacts" >/dev/null

cat > "$tmp_context_artifact_ingest" <<'JSON'
{
  "schema": "kiana.context-artifact-ingest.v1",
  "root": "/workspace",
  "source_root": "/workspace/reference",
  "store_dir": ".kiana/context-ingest",
  "manifest_path": ".kiana/context-ingest/manifest.json",
  "artifacts_schema": "kiana.context-artifacts.v1",
  "ingested_files": 1,
  "skipped_files": 0,
  "total_bytes": 21,
  "sync": {
    "path": ".kiana/context-ingest/manifest.json",
    "status": "created",
    "reused_files": 0,
    "added_files": 1,
    "changed_files": 0,
    "removed_files": 0
  },
  "artifacts": [
    {
      "id": "ingest:docs/prd.md:0123456789abcdef",
      "kind": "prd",
      "source_path": "docs/prd.md",
      "stored_path": ".kiana/context-ingest/files/0123456789abcdef/docs/prd.md",
      "language": "markdown",
      "bytes": 21,
      "line_count": 1,
      "content_hash": "0123456789abcdef"
    }
  ]
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-context-artifact-ingest.v1.schema.json \
  "$tmp_context_artifact_ingest" >/dev/null

cat > "$tmp_context_artifact_graph" <<'JSON'
{
  "schema": "kiana.context-artifact-dependency-graph.v1",
  "root": "/workspace",
  "nodes": [
    {
      "id": "file:src/lib.rs:0123456789abcdef",
      "kind": "file",
      "path": "src/lib.rs",
      "language": "rust",
      "content_hash": "0123456789abcdef"
    },
    {
      "id": "file:tests/lib_test.rs:fedcba9876543210",
      "kind": "file",
      "path": "tests/lib_test.rs",
      "language": "rust",
      "content_hash": "fedcba9876543210"
    },
    {
      "id": "file:docs/design.md:abcdef0123456789",
      "kind": "file",
      "path": "docs/design.md",
      "language": "markdown",
      "content_hash": "abcdef0123456789"
    }
  ],
  "edges": [
    {
      "source": "file:tests/lib_test.rs:fedcba9876543210",
      "target": "file:src/lib.rs:0123456789abcdef",
      "relation": "test_of",
      "evidence": "tests/lib_test.rs matches src/lib.rs"
    },
    {
      "source": "file:docs/design.md:abcdef0123456789",
      "target": "file:src/lib.rs:0123456789abcdef",
      "relation": "path_reference",
      "evidence": "docs/design.md references src/lib.rs"
    }
  ]
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-context-artifact-dependency-graph.v1.schema.json \
  "$tmp_context_artifact_graph" >/dev/null

cat > "$tmp_context_artifact_store" <<'JSON'
{
  "schema": "kiana.context-artifact-store.v1",
  "root": "/workspace",
  "artifacts_schema": "kiana.context-artifacts.v1",
  "dependency_graph_schema": "kiana.context-artifact-dependency-graph.v1",
  "artifact_count": 1,
  "dependency_count": 0,
  "artifact_roles": [
    { "role": "source", "count": 1 }
  ],
  "artifacts": {
    "schema": "kiana.context-artifacts.v1",
    "root": "/workspace",
    "files_indexed": 1,
    "skipped_files": 0,
    "artifacts": [
      {
        "id": "file:src/lib.rs:0123456789abcdef",
        "kind": "file",
        "path": "src/lib.rs",
        "language": "rust",
        "bytes": 21,
        "line_count": 1,
        "content_hash": "0123456789abcdef"
      }
    ]
  },
  "dependency_graph": {
    "schema": "kiana.context-artifact-dependency-graph.v1",
    "root": "/workspace",
    "nodes": [
      {
        "id": "file:src/lib.rs:0123456789abcdef",
        "kind": "file",
        "path": "src/lib.rs",
        "language": "rust",
        "content_hash": "0123456789abcdef"
      }
    ],
    "edges": []
  },
  "cache": {
    "path": "/workspace/.kiana/context-artifact-store.json",
    "status": "created",
    "reused_artifacts": 0,
    "added_artifacts": 1,
    "changed_artifacts": 0,
    "removed_artifacts": 0,
    "reused_dependencies": 0,
    "added_dependencies": 0,
    "removed_dependencies": 0
  }
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-context-artifact-store.v1.schema.json \
  "$tmp_context_artifact_store" >/dev/null

cat > "$tmp_context_artifact_readiness" <<'JSON'
{
  "schema": "kiana.context-artifact-readiness.v1",
  "root": "/workspace",
  "artifact_store_schema": "kiana.context-artifact-store.v1",
  "status": "incomplete",
  "artifact_count": 3,
  "dependency_count": 2,
  "required_roles": [
    { "role": "prd", "required": true, "present": false, "count": 0 },
    { "role": "design", "required": true, "present": true, "count": 1 },
    { "role": "tasks", "required": true, "present": false, "count": 0 },
    { "role": "source", "required": true, "present": true, "count": 1 },
    { "role": "test", "required": true, "present": true, "count": 1 }
  ],
  "missing_roles": ["prd", "tasks"]
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-context-artifact-readiness.v1.schema.json \
  "$tmp_context_artifact_readiness" >/dev/null

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

cat > "$tmp_release_signature" <<'JSON'
{
  "schema": "kiana.release-signature.v1",
  "target": "linux-x86_64",
  "archive": "kiana-0.1.0-linux-x86_64.tar.gz",
  "archive_sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
  "binary_sha256_file_sha256": "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
  "signed_at": "2026-01-01T00:00:00Z",
  "signer": "release engineering",
  "signature_files": {
    "archive": "kiana-0.1.0-linux-x86_64.tar.gz.sig",
    "binary": "kiana-0.1.0-linux-x86_64.binary.sig"
  },
  "verification": {
    "method": "KIANA_SIGNATURE_VERIFY_COMMAND",
    "archive": "verified",
    "binary": "verified",
    "verified_at": "2026-01-01T00:00:01Z"
  }
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-release-signature.v1.schema.json \
  "$tmp_release_signature" >/dev/null

cat > "$tmp_platform_security" <<'JSON'
{
  "schema": "kiana.platform-security-proof.v1",
  "version": "0.1.0",
  "status": "accepted",
  "accepted": true,
  "accepted_by": "release security",
  "accepted_at": "2026-01-01T00:00:00Z",
  "platform": "linux",
  "runner": "release-runner-linux-1",
  "runner_id": "runner-linux-1",
  "generated_at": "2026-01-01T00:00:01Z",
  "isolation": "linux_bwrap",
  "controls": [
    "permission_profile:commercial",
    "permission_mode:ask",
    "explicit_project_trust:required",
    "managed_allow_required_for_mutations",
    "isolation:linux_bwrap",
    "network_policy:explicit-provider-credentials"
  ],
  "doctor_status": "ready",
  "doctor_command": "kiana doctor --json",
  "doctor_report_sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
  "evidence": [
    {
      "label": "doctor_command",
      "value": "kiana doctor --json"
    },
    {
      "label": "doctor_report_sha256",
      "value": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    },
    {
      "label": "runner_id",
      "value": "runner-linux-1"
    }
  ]
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-platform-security-proof.v1.schema.json \
  "$tmp_platform_security" >/dev/null

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

cat > "$tmp_distribution_review" <<'JSON'
{
  "schema": "kiana.app-server.distribution-review.v1",
  "version": "0.1.0",
  "dist_dir": "dist",
  "summary": {
    "artifacts": 2,
    "platforms": {
      "linux": true,
      "macos": false,
      "windows": true
    },
    "channels_ready": 2,
    "blocking": 2
  },
  "artifacts": [
    {
      "target": "linux-x86_64",
      "archive": "kiana-0.1.0-linux-x86_64.tar.gz",
      "path": "kiana-0.1.0-linux-x86_64.tar.gz",
      "sha256_file": "kiana-0.1.0-linux-x86_64.tar.gz.sha256",
      "binary_sha256_file": "kiana-0.1.0-linux-x86_64.binary.sha256"
    },
    {
      "target": "windows-x86_64",
      "archive": "kiana-0.1.0-windows-x86_64.zip",
      "path": "kiana-0.1.0-windows-x86_64.zip",
      "sha256_file": "kiana-0.1.0-windows-x86_64.zip.sha256",
      "binary_sha256_file": "kiana-0.1.0-windows-x86_64.binary.sha256"
    }
  ],
  "channels": {
    "github_releases": {
      "status": "ready",
      "artifact_count": 2
    },
    "homebrew": {
      "status": "ready",
      "formulae": ["manifests/homebrew/kiana-linux-x86_64.rb"]
    },
    "winget": {
      "status": "blocked",
      "manifests": ["manifests/winget/Kiana.Kiana/0.1.0/Kiana.Kiana.installer.yaml"],
      "blocked_path": "manifests/winget/BLOCKED.md"
    }
  },
  "enterprise_offline_manifest": {
    "present": true,
    "path": "manifests/enterprise/offline-manifest.json",
    "valid": true,
    "schema": "kiana.enterprise.offline-manifest.v1",
    "artifact_count": 2,
    "channels": {
      "github_releases": "generated_from_release_base_url",
      "homebrew": "generated",
      "winget": "blocked_no_windows_publishable_artifact"
    }
  },
  "blockers": [
    {
      "id": "distribution.platform-artifacts",
      "blocking": true,
      "message": "missing platform artifacts: macos"
    },
    {
      "id": "distribution.winget",
      "blocking": true,
      "message": "winget channel is explicitly blocked"
    }
  ]
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-app-server-distribution-review.v1.schema.json \
  "$tmp_distribution_review" >/dev/null

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
    "proofs": 5,
    "blockers_total": 13,
    "local_blockers": 0,
    "external_blockers": 13
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
  "readiness": {
    "ready": true,
    "release_artifacts_present": true,
    "lifecycle_smoke_passed": true,
    "required_proofs_present": true,
    "local_blockers_clear": true,
    "required_proof_schemas": [
      {
        "schema": "kiana.source-control-proof.v1",
        "present": true,
        "paths": ["dist/proofs/local-rc/source-control/source-control.json"]
      },
      {
        "schema": "kiana.product-acceptance.v1",
        "present": true,
        "paths": ["dist/proofs/local-rc/product/product-acceptance.json"]
      },
      {
        "schema": "kiana.entitlement-proof.v1",
        "present": true,
        "paths": ["dist/proofs/local-rc/entitlement/entitlement-proof.json"]
      },
      {
        "schema": "kiana.release-ops.v1",
        "present": true,
        "paths": ["dist/proofs/local-rc/release-ops/release-ops.json"]
      },
      {
        "schema": "kiana.platform-security-proof.v1",
        "present": true,
        "paths": ["dist/proofs/local-rc/platform-security/platform-security-linux.json"]
      }
    ],
    "issues": []
  },
  "proofs": [
    {
      "path": "dist/proofs/local-rc/source-control/source-control.json",
      "schema": "kiana.source-control-proof.v1",
      "status": "local_rc_only",
      "accepted": false
    },
    {
      "path": "dist/proofs/local-rc/product/product-acceptance.json",
      "schema": "kiana.product-acceptance.v1",
      "status": "headless_smoke_only",
      "accepted": false
    },
    {
      "path": "dist/proofs/local-rc/entitlement/entitlement-proof.json",
      "schema": "kiana.entitlement-proof.v1",
      "status": "local_rc_only",
      "accepted": false
    },
    {
      "path": "dist/proofs/local-rc/release-ops/release-ops.json",
      "schema": "kiana.release-ops.v1",
      "status": "local_rc_only",
      "accepted": false
    },
    {
      "path": "dist/proofs/local-rc/platform-security/platform-security-linux.json",
      "schema": "kiana.platform-security-proof.v1",
      "status": "local_rc_only",
      "accepted": false
    }
  ],
  "blockers": {
    "status": "blocked",
    "blocking": 13,
    "local_blocking": 0,
    "external_blocking": 13,
    "blocking_ids": ["source.remote", "source.version-tag"],
    "external_blocking_ids": ["source.remote", "source.version-tag"],
    "blocking_by_resolution_scope": {
      "local-automation": 0,
      "release-owner": 2,
      "release-security": 0,
      "release-environment": 0,
      "final-artifact-derived": 0,
      "live-service": 0,
      "acceptance-owner": 0
    },
    "blocking_ids_by_resolution_scope": {
      "local-automation": [],
      "release-owner": ["source.remote", "source.version-tag"],
      "release-security": [],
      "release-environment": [],
      "final-artifact-derived": [],
      "live-service": [],
      "acceptance-owner": []
    },
    "handoff_status": "external_action_required",
    "handoff_artifacts": [
      {
        "kind": "blockers_json",
        "path": "dist/proofs/local-rc/blockers/commercial-release-blockers.json",
        "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
      },
      {
        "kind": "handoff_markdown",
        "path": "dist/proofs/local-rc/blockers/commercial-release-handoff.md",
        "sha256": "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
      }
    ]
  }
}
JSON
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-local-rc-evidence.v1.schema.json \
  "$tmp_local_rc_evidence" >/dev/null

tmp_report="$(mktemp)"
tmp_handoff="$(mktemp)"
trap 'rm -f "$tmp_runtime_event" "$tmp_runtime_result" "$tmp_app_events" "$tmp_app_command_run" "$tmp_app_permissions_status" "$tmp_app_trust_status" "$tmp_app_team_status" "$tmp_team_plan" "$tmp_tasks" "$tmp_auth_status" "$tmp_context_index" "$tmp_context_vector_search" "$tmp_context_artifacts" "$tmp_context_artifact_graph" "$tmp_context_artifact_store" "$tmp_context_artifact_readiness" "$tmp_repo_map" "$tmp_diff" "$tmp_checkpoint" "$tmp_checks_dry_run" "$tmp_checks_run" "$tmp_review_dry_run" "$tmp_review_run" "$tmp_proof_manifest" "$tmp_source_control" "$tmp_release_signature" "$tmp_enterprise_offline_manifest" "$tmp_distribution_review" "$tmp_platform_security" "$tmp_doctor" "$tmp_local_rc_evidence" "$tmp_managed_plugin_policy" "$tmp_plugin_app_manifest" "$tmp_report" "$tmp_handoff"; rm -rf "$tmp_app_trust_invalid_dir" "$tmp_eda_invalid_dir"' EXIT
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
    for key in ["owner", "owner_status", "resolution_scope", "acceptance_artifacts", "verification_commands", "handoff_notes"]:
        if key not in check:
            raise SystemExit(f"commercial blockers report missing {key}")
summary_scopes = report.get("summary", {}).get("blocking_by_resolution_scope", {})
for scope in [
    "local-automation",
    "release-owner",
    "release-security",
    "release-environment",
    "final-artifact-derived",
    "live-service",
    "acceptance-owner",
]:
    if scope not in summary_scopes:
        raise SystemExit(f"commercial blockers report missing scope count {scope}")
if "## Blocking Assignments" not in handoff:
    raise SystemExit("commercial blockers handoff is missing assignment section")
if "source.remote" not in handoff:
    raise SystemExit("commercial blockers handoff is missing source.remote")
platform_security = next(
    (check for check in checks if check.get("id") == "acceptance.platform-security"),
    None,
)
if not platform_security:
    raise SystemExit("commercial blockers report is missing acceptance.platform-security")
platform_artifacts = set(platform_security.get("acceptance_artifacts") or [])
for platform in ["linux", "macos", "windows"]:
    expected = f"docs/platform-security/0.1.0-{platform}.json"
    if expected not in platform_artifacts:
        raise SystemExit(f"platform security blocker is missing {expected}")
PY
bash scripts/commercial-release-handoff-smoke.sh >/dev/null
bash scripts/capability-governance-smoke.sh

echo "schema contract smoke passed"
