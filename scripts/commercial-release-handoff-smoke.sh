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
tmp_source_control="$(mktemp)"
tmp_proof_report="$(mktemp)"
tmp_proof_handoff="$(mktemp)"
tmp_dist="$(mktemp -d)"
trap 'rm -f "$tmp_report" "$tmp_handoff" "$tmp_source_control" "$tmp_proof_report" "$tmp_proof_handoff"; rm -rf "$tmp_dist"' EXIT

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

KIANA_SOURCE_CONTROL_PROOF_FILE="$tmp_source_control" \
  bash scripts/commercial-release-blockers-report.sh \
    --json \
    --handoff-md "$tmp_proof_handoff" > "$tmp_proof_report"

"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-commercial-release-blockers.v1.schema.json \
  "$tmp_proof_report" >/dev/null

"$python" - "$tmp_proof_report" "$tmp_proof_handoff" <<'PY'
import json
import sys
from pathlib import Path

report = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
handoff = Path(sys.argv[2]).read_text(encoding="utf-8")
by_id = {check["id"]: check for check in report.get("checks", [])}

for check_id in ["source.remote", "source.version-tag"]:
    check = by_id.get(check_id)
    if not check:
        raise SystemExit(f"{check_id} check is missing")
    if check.get("status") != "satisfied":
        raise SystemExit(f"{check_id} was not satisfied by accepted source-control proof")
    if check_id in handoff:
        raise SystemExit(f"{check_id} should not appear as a blocking handoff assignment")
    if "source-control proof accepted" not in check.get("evidence", ""):
        raise SystemExit(f"{check_id} evidence does not name accepted source-control proof")
PY

"$python" - "$tmp_dist" <<'PY'
import hashlib
import json
import sys
from pathlib import Path

dist = Path(sys.argv[1])
version = "0.1.0"
target = "linux-x86_64"
package = f"kiana-{version}-{target}"
archive_name = f"{package}.tar.gz"
archive = dist / archive_name
binary_sha = dist / f"{package}.binary.sha256"
archive_sig = dist / f"{archive_name}.sig"
binary_sig = dist / f"{package}.binary.sig"
proof = dist / f"{package}.signature.json"

archive.write_text("signed fixture archive\n", encoding="utf-8")
binary_sha.write_text("0" * 64 + f"  {package}/kiana\n", encoding="utf-8")
archive_sig.write_text("fixture archive signature\n", encoding="utf-8")
binary_sig.write_text("fixture binary signature\n", encoding="utf-8")

def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

proof.write_text(
    json.dumps(
        {
            "schema": "kiana.release-signature.v1",
            "target": target,
            "archive": archive_name,
            "archive_sha256": sha256(archive),
            "binary_sha256_file_sha256": sha256(binary_sha),
            "signed_at": "2026-01-01T00:00:00Z",
            "signer": "release engineering",
            "signature_files": {
                "archive": archive_sig.name,
                "binary": binary_sig.name,
            },
            "verification": {
                "method": "KIANA_SIGNATURE_VERIFY_COMMAND",
                "archive": "verified",
                "binary": "verified",
                "verified_at": "2026-01-01T00:00:01Z",
            },
        },
        indent=2,
        sort_keys=True,
    )
    + "\n",
    encoding="utf-8",
)
PY

DIST_DIR="$tmp_dist" \
  bash scripts/commercial-release-blockers-report.sh \
    --json \
    --handoff-md "$tmp_proof_handoff" > "$tmp_proof_report"

"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-commercial-release-blockers.v1.schema.json \
  "$tmp_proof_report" >/dev/null

"$python" - "$tmp_proof_report" "$tmp_proof_handoff" <<'PY'
import json
import sys
from pathlib import Path

report = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
handoff = Path(sys.argv[2]).read_text(encoding="utf-8")
check = {item["id"]: item for item in report.get("checks", [])}.get("signing.release-artifacts")
if not check:
    raise SystemExit("signing.release-artifacts check is missing")
if check.get("status") != "satisfied":
    raise SystemExit("signing.release-artifacts was not satisfied by accepted signature proof")
if "signing.release-artifacts" in handoff:
    raise SystemExit("signing.release-artifacts should not appear as a blocking handoff assignment")
if "release signature proofs accepted" not in check.get("evidence", ""):
    raise SystemExit("signing.release-artifacts evidence does not name accepted signature proofs")
PY

mkdir -p "$tmp_dist/proofs/product"
cat > "$tmp_dist/proofs/product/product-acceptance.json" <<'JSON'
{
  "schema": "kiana.product-acceptance.v1",
  "version": "0.1.0",
  "status": "accepted",
  "accepted": true,
  "accepted_by": "target customer acceptance lead",
  "accepted_at": "2026-01-01T00:00:00Z",
  "scope": "terminal product shell, local app-server, and context-search acceptance",
  "workflows": [
    "permission",
    "diff",
    "history",
    "onboarding",
    "resume",
    "settings",
    "app-server",
    "context-search",
    "context-cache-recovery"
  ]
}
JSON

DIST_DIR="$tmp_dist" \
  bash scripts/commercial-release-blockers-report.sh \
    --json \
    --handoff-md "$tmp_proof_handoff" > "$tmp_proof_report"

"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-commercial-release-blockers.v1.schema.json \
  "$tmp_proof_report" >/dev/null

"$python" - "$tmp_proof_report" "$tmp_proof_handoff" <<'PY'
import json
import sys
from pathlib import Path

report = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
handoff = Path(sys.argv[2]).read_text(encoding="utf-8")
check = {item["id"]: item for item in report.get("checks", [])}.get("acceptance.product")
if not check:
    raise SystemExit("acceptance.product check is missing")
if check.get("status") != "satisfied":
    raise SystemExit("acceptance.product was not satisfied by staged product acceptance proof")
if "acceptance.product" in handoff:
    raise SystemExit("acceptance.product should not appear as a blocking handoff assignment")
if "product acceptance proof accepted" not in check.get("evidence", ""):
    raise SystemExit("acceptance.product evidence does not name accepted staged proof")
PY

echo "commercial release handoff smoke passed"
