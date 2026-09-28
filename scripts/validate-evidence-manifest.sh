#!/usr/bin/env bash
# SC-35: validate one evidence manifest against the shape the card requires.
#
# The Rust module decides whether a manifest is internally consistent. This script is the
# other half: it checks a manifest *file* in the repository, so that a manifest committed by
# hand is held to the same five fields (command, environment, source snapshot, fixture,
# limitations) plus the seal, before anybody reads it as evidence.
#
# It is deliberately dependency-light: bash plus python3 for JSON. It reads one file, writes
# nothing, and exits non-zero listing the rules it broke.
#
# Usage: bash scripts/validate-evidence-manifest.sh <manifest.json>
set -Eeuo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <manifest.json>" >&2
  exit 2
fi

manifest="$1"
if [[ ! -f "$manifest" ]]; then
  echo "evidence manifest not found: $manifest" >&2
  exit 2
fi

python3 - "$manifest" <<'PYEOF'
import json
import re
import sys

PATH = sys.argv[1]
with open(PATH, encoding="utf-8") as handle:
    raw = handle.read()

try:
    manifest = json.loads(raw)
except json.JSONDecodeError as error:
    sys.exit(f"evidence manifest is not valid JSON: {error}")

if not isinstance(manifest, dict):
    sys.exit("evidence manifest must be a JSON object")

failures = []


def require_text(key):
    value = manifest.get(key)
    if not isinstance(value, str) or not value.strip():
        failures.append(f"{key} is missing or empty")
        return ""
    return value


def require_list(key):
    value = manifest.get(key)
    if not isinstance(value, list) or not value:
        failures.append(f"{key} is missing or empty")
        return []
    return value


schema = require_text("schema")
if schema and schema != "kiana.evidence-manifest.v1":
    failures.append(f"unknown schema: {schema}")

require_text("manifest_id")

snapshot = require_text("source_snapshot")
if snapshot and not re.fullmatch(r"[0-9a-f]{7,64}", snapshot):
    failures.append("source_snapshot must be 7..=64 lowercase hex characters")

recorded_by = require_text("recorded_by")
reviewer = require_text("reviewer")
if recorded_by and reviewer and recorded_by == reviewer:
    failures.append("reviewer must not be the recorder")

command = manifest.get("command")
if not isinstance(command, dict):
    failures.append("command is missing")
else:
    argv = command.get("argv")
    if not isinstance(argv, list) or not argv:
        failures.append("command.argv is missing or empty")
    if not isinstance(command.get("cwd"), str) or not command["cwd"].strip():
        failures.append("command.cwd is missing or empty")
    if command.get("exit_code") is None:
        failures.append("command.exit_code is missing")

require_list("environment")

fixtures = manifest.get("fixtures")
absent_reason = manifest.get("fixture_absent_reason")
if isinstance(fixtures, list) and fixtures:
    if isinstance(absent_reason, str) and absent_reason.strip():
        failures.append("fixture_absent_reason must be empty when fixtures are cited")
    for entry in fixtures:
        if not isinstance(entry, dict):
            failures.append("each fixture must be an object")
            continue
        path = entry.get("path", "")
        if not isinstance(path, str) or path.startswith("/") or ".." in path or "://" in path:
            failures.append(f"fixture path must be repository-relative: {path!r}")
        digest = entry.get("sha256", "")
        if not isinstance(digest, str) or not re.fullmatch(r"sha256:[0-9a-f]{64}", digest):
            failures.append(f"fixture sha256 is malformed: {digest!r}")
elif not (isinstance(absent_reason, str) and absent_reason.strip()):
    failures.append("either fixtures or fixture_absent_reason is required")

require_text("status_change")
require_list("limitations")

rungs = {"source", "local_behavior", "durable", "live", "physical"}
proof = manifest.get("proof_level")
if proof not in rungs:
    failures.append(f"unknown proof_level: {proof!r}")

statuses = {"not_supported", "target", "deferred", "partial", "implemented"}
feature = manifest.get("feature_status")
if feature not in statuses:
    failures.append(f"unknown feature_status: {feature!r}")

if (
    feature in {"not_supported", "target", "deferred"}
    and proof in {"durable", "live", "physical"}
):
    failures.append(f"feature_status={feature} cannot carry proof_level={proof}")

if failures:
    for failure in failures:
        print(f"evidence manifest rule broken: {failure}", file=sys.stderr)
    sys.exit(1)

print(f"evidence manifest shape ok: {PATH}")
PYEOF
