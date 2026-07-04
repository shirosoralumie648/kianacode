#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

python_bin() {
  command -v python3 2>/dev/null || command -v python 2>/dev/null || {
    echo "commercial release handoff smoke requires python3 or python" >&2
    exit 1
  }
}

python="$(python_bin)"
tmp_report="$(mktemp)"
tmp_handoff="$(mktemp)"
trap 'rm -f "$tmp_report" "$tmp_handoff"' EXIT

KIANA_BLOCKER_OWNER_SOURCE_REMOTE="release-manager-test" \
  bash scripts/commercial-release-blockers-report.sh \
    --json \
    --handoff-md "$tmp_handoff" > "$tmp_report"

"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-commercial-release-blockers.v1.schema.json \
  "$tmp_report" >/dev/null

"$python" - "$tmp_report" "$tmp_handoff" <<'PY'
import json
import sys
from pathlib import Path

report = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
handoff = Path(sys.argv[2]).read_text(encoding="utf-8")

if report.get("schema") != "kiana.commercial-release-blockers.v1":
    raise SystemExit("commercial handoff report schema mismatch")
if not handoff.startswith("# Kiana Commercial Release Handoff "):
    raise SystemExit("commercial handoff markdown has an unexpected title")
if "## Blocking Assignments" not in handoff:
    raise SystemExit("commercial handoff markdown is missing the assignment section")

checks = report.get("checks", [])
if not checks:
    raise SystemExit("commercial blockers report has no checks")

required_keys = {
    "owner",
    "owner_status",
    "acceptance_artifacts",
    "verification_commands",
    "handoff_notes",
}
for check in checks:
    missing = sorted(required_keys.difference(check))
    if missing:
        raise SystemExit(f"{check.get('id', '<unknown>')} missing handoff keys: {missing}")
    if not check["owner"]:
        raise SystemExit(f"{check['id']} has an empty owner")
    if check["owner_status"] not in {
        "local-owner",
        "role-owner-required",
        "specific-owner-assigned",
    }:
        raise SystemExit(f"{check['id']} has invalid owner_status {check['owner_status']!r}")
    for key in ["acceptance_artifacts", "verification_commands", "handoff_notes"]:
        if not isinstance(check[key], list):
            raise SystemExit(f"{check['id']} {key} is not a list")

by_id = {check["id"]: check for check in checks}
source_remote = by_id.get("source.remote")
if not source_remote:
    raise SystemExit("source.remote check is missing")
if source_remote["owner"] != "release-manager-test":
    raise SystemExit("source.remote owner override was not applied")
if source_remote["owner_status"] != "specific-owner-assigned":
    raise SystemExit("source.remote owner override did not mark a specific owner")
if "source.remote" not in handoff:
    raise SystemExit("source.remote is missing from the handoff markdown")
if "release-manager-test" not in handoff:
    raise SystemExit("owner override is missing from the handoff markdown")

blocking = [check for check in checks if check.get("status") == "blocking"]
if blocking:
    for check in blocking:
        if check["id"] not in handoff:
            raise SystemExit(f"{check['id']} blocking assignment missing from markdown")
        if check["owner"] not in handoff:
            raise SystemExit(f"{check['id']} owner missing from markdown")
else:
    if "No blocking checks were detected." not in handoff:
        raise SystemExit("ready handoff does not state that no blockers were detected")
PY

echo "commercial release handoff smoke passed"
