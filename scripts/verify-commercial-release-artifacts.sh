#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

mode="${1:-full}"
if [[ "$mode" == "--local-rc" ]]; then
  echo "commercial artifact verification skipped in local RC mode"
  exit 0
fi
if [[ "$mode" != "full" ]]; then
  echo "usage: $0 [full|--local-rc]" >&2
  exit 2
fi

version="${VERSION:-$(tr -d '\r\n' < VERSION)}"
dist_dir="${DIST_DIR:-dist}"
manifest_dir="${MANIFEST_DIR:-${dist_dir}/manifests}"
signature_verify_command="${KIANA_SIGNATURE_VERIFY_COMMAND:-}"
failures=0
seen_linux=0
seen_macos=0
seen_windows=0

fail() {
  echo "FAIL: $*" >&2
  failures=$((failures + 1))
}

pass() {
  echo "OK: $*"
}

require_file() {
  if [[ -f "$1" ]]; then
    pass "file exists: $1"
  else
    fail "missing file: $1"
  fi
}

require_json_pattern() {
  local file="$1"
  local pattern="$2"
  local label="$3"
  if [[ ! -f "$file" ]]; then
    return
  fi
  if grep -Eq "$pattern" "$file"; then
    pass "$label"
  else
    fail "$file missing $label"
  fi
}

reject_json_pattern() {
  local file="$1"
  local pattern="$2"
  local label="$3"
  if [[ ! -f "$file" ]]; then
    return
  fi
  if grep -Eq "$pattern" "$file"; then
    fail "$file contains $label"
  else
    pass "$label not present"
  fi
}

require_proof_file() {
  local file="$1"
  local schema="$2"
  local label="$3"
  require_file "$file"
  require_json_pattern "$file" "\"schema\"[[:space:]]*:[[:space:]]*\"${schema}\"" "${label} schema"
}

verify_signature_file() {
  local target="$1"
  local signature="$2"
  local label="$3"
  if [[ ! -f "$target" || ! -f "$signature" ]]; then
    return
  fi
  if [[ -z "$signature_verify_command" ]]; then
    fail "KIANA_SIGNATURE_VERIFY_COMMAND is required to verify ${label}"
    return
  fi
  if KIANA_SIGNATURE_VERIFY_TARGET="$target" \
    KIANA_SIGNATURE_VERIFY_SIGNATURE="$signature" \
    bash -c "$signature_verify_command"
  then
    pass "${label} signature verifies"
  else
    fail "${label} signature failed verification"
  fi
}

python_bin() {
  command -v python3 2>/dev/null || command -v python 2>/dev/null || true
}

require_release_ops_contract() {
  local file="$1"
  local expected_version="$2"
  if [[ ! -f "$file" ]]; then
    return
  fi
  local python
  python="$(python_bin)"
  if [[ -z "$python" ]]; then
    fail "python3 or python is required to validate release ops proof"
    return
  fi
  if "$python" - "$file" "$expected_version" <<'PY'
import json
import sys

path, expected_version = sys.argv[1:3]
with open(path, "r", encoding="utf-8") as handle:
    report = json.load(handle)

credential_review = report.get("credential_review")
if not isinstance(credential_review, dict):
    credential_review = {}

required_strings = [
    "accepted_by",
    "accepted_at",
    "security_contact",
    "vulnerability_report_channel",
    "release_credentials_owner",
    "support_contact",
]
placeholder_markers = (
    "todo",
    "tbd",
    "pending",
    "placeholder",
    "replace-me",
    "example.com",
    "example.test",
    "public issue",
)

def filled(key):
    value = report.get(key)
    return isinstance(value, str) and bool(value.strip())

def not_placeholder(key):
    value = report.get(key)
    return isinstance(value, str) and not any(
        marker in value.lower() for marker in placeholder_markers
    )

def review_not_placeholder(key):
    value = credential_review.get(key)
    return isinstance(value, str) and not any(
        marker in value.lower() for marker in placeholder_markers
    )

checks = [
    report.get("schema") == "kiana.release-ops.v1",
    report.get("version") == expected_version,
    report.get("status") == "accepted",
    report.get("accepted") is True,
    all(filled(key) for key in required_strings),
    all(not_placeholder(key) for key in required_strings),
    isinstance(report.get("artifact_retention_days"), int)
    and report["artifact_retention_days"] >= 90,
    isinstance(report.get("log_retention_days"), int)
    and report["log_retention_days"] >= 30,
    credential_review.get("status") == "accepted",
    isinstance(credential_review.get("reviewed_by"), str)
    and bool(credential_review["reviewed_by"].strip()),
    review_not_placeholder("reviewed_by"),
    isinstance(credential_review.get("reviewed_at"), str)
    and bool(credential_review["reviewed_at"].strip()),
    isinstance(credential_review.get("scope"), str)
    and bool(credential_review["scope"].strip()),
    review_not_placeholder("scope"),
]
sys.exit(0 if all(checks) else 1)
PY
  then
    pass "release ops proof commercial contract"
  else
    fail "release ops proof failed commercial contract"
  fi
}

require_entitlement_contract() {
  local file="$1"
  local expected_version="$2"
  if [[ ! -f "$file" ]]; then
    return
  fi
  local python
  python="$(python_bin)"
  if [[ -z "$python" ]]; then
    fail "python3 or python is required to validate entitlement proof"
    return
  fi
  if "$python" - "$file" "$expected_version" "${KIANA_REQUIRED_ENTITLEMENTS:-commercial-use,enterprise-support,managed-policy}" <<'PY'
import json
import sys

path, expected_version, required_raw = sys.argv[1:4]
required = {item.strip() for item in required_raw.split(",") if item.strip()}
with open(path, "r", encoding="utf-8") as handle:
    report = json.load(handle)

backend = report.get("backend")
if not isinstance(backend, dict):
    backend = {}

required_strings = [
    "accepted_by",
    "accepted_at",
    "account_id",
    "organization",
    "plan",
    "license_key_fingerprint",
    "support_contact",
]
backend_strings = ["name", "environment", "checked_at", "request_id"]
placeholder_markers = (
    "todo",
    "tbd",
    "pending",
    "placeholder",
    "replace-me",
    "example.com",
    "example.test",
)

def filled(mapping, key):
    value = mapping.get(key)
    return isinstance(value, str) and bool(value.strip())

def not_placeholder(mapping, key):
    value = mapping.get(key)
    return isinstance(value, str) and not any(
        marker in value.lower() for marker in placeholder_markers
    )

entitlements = set(report.get("entitlements") or [])
checks = [
    report.get("schema") == "kiana.entitlement-proof.v1",
    report.get("version") == expected_version,
    report.get("status") == "accepted",
    report.get("accepted") is True,
    report.get("license_status") == "active",
    all(filled(report, key) for key in required_strings),
    all(not_placeholder(report, key) for key in required_strings),
    required.issubset(entitlements),
    all(filled(backend, key) for key in backend_strings),
    all(not_placeholder(backend, key) for key in backend_strings),
]
sys.exit(0 if all(checks) else 1)
PY
  then
    pass "entitlement proof commercial contract"
  else
    fail "entitlement proof failed commercial contract"
  fi
}

require_product_acceptance_contract() {
  local file="$1"
  local expected_version="$2"
  if [[ ! -f "$file" ]]; then
    return
  fi
  local python
  python="$(python_bin)"
  if [[ -z "$python" ]]; then
    fail "python3 or python is required to validate product acceptance proof"
    return
  fi
  if "$python" - "$file" "$expected_version" <<'PY'
import json
import sys

path, expected_version = sys.argv[1:3]
with open(path, "r", encoding="utf-8") as handle:
    report = json.load(handle)

required_workflows = {
    "permission",
    "diff",
    "history",
    "onboarding",
    "resume",
    "settings",
    "app-server",
    "context-search",
    "context-cache-recovery",
}
placeholder_markers = (
    "todo",
    "tbd",
    "pending",
    "placeholder",
    "replace-me",
    "example.com",
    "example.test",
)

def filled(key):
    value = report.get(key)
    return isinstance(value, str) and bool(value.strip())

def not_placeholder(key):
    value = report.get(key)
    return isinstance(value, str) and not any(
        marker in value.lower() for marker in placeholder_markers
    )

workflows = set(report.get("workflows") or [])
checks = [
    report.get("schema") == "kiana.product-acceptance.v1",
    report.get("version") == expected_version,
    report.get("status") == "accepted",
    report.get("accepted") is True,
    all(filled(key) and not_placeholder(key) for key in ["accepted_by", "accepted_at", "scope"]),
    required_workflows.issubset(workflows),
]
sys.exit(0 if all(checks) else 1)
PY
  then
    pass "product acceptance proof commercial contract"
  else
    fail "product acceptance proof failed commercial contract"
  fi
}

require_remote_smoke_contract() {
  local file="$1"
  if [[ ! -f "$file" ]]; then
    return
  fi
  local python
  python="$(python_bin)"
  if [[ -z "$python" ]]; then
    fail "python3 or python is required to validate remote live smoke proof"
    return
  fi
  if "$python" - "$file" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, "r", encoding="utf-8") as handle:
    report = json.load(handle)

allowed_keys = {
    "schema",
    "status",
    "checked_at",
    "session_id",
    "api_base_url",
    "sdk_url",
    "expires_in",
    "worker_epoch",
}
checks = [
    set(report).issubset(allowed_keys),
    report.get("schema") == "kiana.remote-code-session-smoke.v1",
    report.get("status") == "ok",
    isinstance(report.get("checked_at"), str) and bool(report["checked_at"].strip()),
    isinstance(report.get("session_id"), str) and report["session_id"].startswith("cse_"),
    isinstance(report.get("api_base_url"), str) and report["api_base_url"].startswith(("http://", "https://")),
    isinstance(report.get("sdk_url"), str) and report["sdk_url"].startswith(("http://", "https://")),
    isinstance(report.get("expires_in"), int) and report["expires_in"] > 0,
    isinstance(report.get("worker_epoch"), int) and report["worker_epoch"] >= 0,
]
sys.exit(0 if all(checks) else 1)
PY
  then
    pass "remote live smoke proof commercial contract"
  else
    fail "remote live smoke proof failed commercial contract"
  fi
}

require_provider_live_contract() {
  local catalog_file="$1"
  local smoke_file="$2"
  if [[ ! -f "$catalog_file" || ! -f "$smoke_file" ]]; then
    return
  fi
  local python
  python="$(python_bin)"
  if [[ -z "$python" ]]; then
    fail "python3 or python is required to validate provider live proofs"
    return
  fi
  if "$python" - "$catalog_file" "$smoke_file" <<'PY'
import json
import sys

catalog_path, smoke_path = sys.argv[1:3]
with open(catalog_path, "r", encoding="utf-8") as handle:
    catalog = json.load(handle)
with open(smoke_path, "r", encoding="utf-8") as handle:
    smoke = json.load(handle)

catalog_summary = catalog.get("summary")
if not isinstance(catalog_summary, dict):
    catalog_summary = {}
providers = catalog.get("providers")
if not isinstance(providers, list):
    providers = []
smoke_summary = smoke.get("summary")
if not isinstance(smoke_summary, dict):
    smoke_summary = {}
results = smoke.get("results")
if not isinstance(results, list):
    results = []

live_text_passed = any(
    isinstance(item, dict)
    and item.get("provider_id") != "fake"
    and item.get("live") is True
    and item.get("capability") == "text"
    and item.get("status") == "passed"
    for item in results
)
live_tools_passed = any(
    isinstance(item, dict)
    and item.get("provider_id") != "fake"
    and item.get("live") is True
    and item.get("capability") == "tools"
    and item.get("status") == "passed"
    for item in results
)
provider_rows_valid = all(
    isinstance(item, dict)
    and isinstance(item.get("provider_id"), str)
    and bool(item["provider_id"].strip())
    and isinstance(item.get("status"), str)
    and isinstance(item.get("live"), bool)
    and isinstance(item.get("model_ids"), list)
    and isinstance(item.get("discovered_model_ids"), list)
    for item in providers
)
result_rows_valid = all(
    isinstance(item, dict)
    and isinstance(item.get("provider_id"), str)
    and bool(item["provider_id"].strip())
    and item.get("status") in {"passed", "skipped", "failed"}
    and isinstance(item.get("live"), bool)
    and item.get("capability") in {"text", "tools"}
    for item in results
)
checks = [
    catalog.get("schema") == "kiana.model-catalog.v1",
    catalog.get("live") is True,
    isinstance(catalog.get("providers"), list) and len(providers) > 0,
    isinstance(catalog_summary.get("providers"), int) and catalog_summary["providers"] > 0,
    isinstance(catalog_summary.get("failed"), int) and catalog_summary["failed"] == 0,
    provider_rows_valid,
    smoke.get("schema") == "kiana.model-smoke.v1",
    smoke.get("live") is True,
    smoke.get("tools") is True,
    isinstance(smoke.get("results"), list) and len(results) > 0,
    isinstance(smoke_summary.get("failed"), int) and smoke_summary["failed"] == 0,
    isinstance(smoke_summary.get("passed"), int) and smoke_summary["passed"] >= 2,
    result_rows_valid,
    live_text_passed,
    live_tools_passed,
]
sys.exit(0 if all(checks) else 1)
PY
  then
    pass "provider live proof commercial contract"
  else
    fail "provider live proof failed commercial contract"
  fi
}

require_platform_security_contract() {
  local file="$1"
  local expected_version="$2"
  if [[ ! -f "$file" ]]; then
    return
  fi
  local python
  python="$(python_bin)"
  if [[ -z "$python" ]]; then
    fail "python3 or python is required to validate platform security proof"
    return
  fi
  if "$python" - "$file" "$expected_version" <<'PY'
import json
import sys

path, expected_version = sys.argv[1:3]
with open(path, "r", encoding="utf-8") as handle:
    report = json.load(handle)

platform = report.get("platform")
expected_isolation = {
    "linux": "linux_bwrap",
    "macos": "macos_exec_policy",
    "windows": "windows_exec_policy",
}.get(platform)
controls = report.get("controls")
evidence = report.get("evidence")
placeholder_markers = (
    "todo",
    "tbd",
    "pending",
    "placeholder",
    "replace-me",
    "example.com",
    "example.test",
)

def filled(key):
    value = report.get(key)
    return isinstance(value, str) and bool(value.strip())

def not_placeholder(key):
    value = report.get(key)
    return isinstance(value, str) and not any(
        marker in value.lower() for marker in placeholder_markers
    )

checks = [
    report.get("schema") == "kiana.platform-security-proof.v1",
    report.get("version") == expected_version,
    report.get("status") == "accepted",
    report.get("accepted") is True,
    filled("accepted_by"),
    filled("accepted_at"),
    filled("runner"),
    not_placeholder("accepted_by"),
    not_placeholder("runner"),
    platform in {"linux", "macos", "windows"},
    report.get("isolation") == expected_isolation,
    report.get("doctor_status") == "ready",
    isinstance(controls, list) and len(controls) > 0,
    isinstance(evidence, list) and len(evidence) > 0,
]
sys.exit(0 if all(checks) else 1)
PY
  then
    pass "platform security proof commercial contract"
  else
    fail "platform security proof failed commercial contract"
  fi
}

require_release_signature_contract() {
  local file="$1"
  local expected_target="$2"
  local expected_archive="$3"
  local expected_archive_sig="$4"
  local expected_binary_sig="$5"
  local archive_file="$6"
  local binary_sha_file="$7"
  if [[ ! -f "$file" ]]; then
    return
  fi
  local python
  python="$(python_bin)"
  if [[ -z "$python" ]]; then
    fail "python3 or python is required to validate release signature proof"
    return
  fi
  if "$python" - "$file" "$expected_target" "$expected_archive" "$expected_archive_sig" "$expected_binary_sig" "$archive_file" "$binary_sha_file" <<'PY'
import hashlib
import json
import sys

(
    path,
    expected_target,
    expected_archive,
    expected_archive_sig,
    expected_binary_sig,
    archive_file,
    binary_sha_file,
) = sys.argv[1:8]
with open(path, "r", encoding="utf-8") as handle:
    report = json.load(handle)

signature_files = report.get("signature_files")
if not isinstance(signature_files, dict):
    signature_files = {}
verification = report.get("verification")
if not isinstance(verification, dict):
    verification = {}

placeholder_markers = (
    "todo",
    "tbd",
    "pending",
    "placeholder",
    "replace-me",
    "example.com",
    "example.test",
    "external-release-signer",
)

def filled(mapping, key):
    value = mapping.get(key)
    return isinstance(value, str) and bool(value.strip())

def not_placeholder(value):
    return isinstance(value, str) and not any(
        marker in value.lower() for marker in placeholder_markers
    )

def sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()

checks = [
    report.get("schema") == "kiana.release-signature.v1",
    report.get("target") == expected_target,
    report.get("archive") == expected_archive,
    report.get("archive_sha256") == sha256(archive_file),
    report.get("binary_sha256_file_sha256") == sha256(binary_sha_file),
    filled(report, "signed_at"),
    filled(report, "signer"),
    not_placeholder(report.get("signer")),
    signature_files.get("archive") == expected_archive_sig,
    signature_files.get("binary") == expected_binary_sig,
    verification.get("method") == "KIANA_SIGNATURE_VERIFY_COMMAND",
    verification.get("archive") == "verified",
    verification.get("binary") == "verified",
    filled(verification, "verified_at"),
]
sys.exit(0 if all(checks) else 1)
PY
  then
    pass "release signature proof commercial contract"
  else
    fail "release signature proof failed commercial contract"
  fi
}

require_macos_notarization_contract() {
  local file="$1"
  local expected_target="$2"
  local expected_archive="$3"
  if [[ ! -f "$file" ]]; then
    return
  fi
  local python
  python="$(python_bin)"
  if [[ -z "$python" ]]; then
    fail "python3 or python is required to validate macOS notarization proof"
    return
  fi
  if "$python" - "$file" "$expected_target" "$expected_archive" <<'PY'
import json
import sys

path, expected_target, expected_archive = sys.argv[1:4]
with open(path, "r", encoding="utf-8") as handle:
    report = json.load(handle)

allowed_keys = {
    "schema",
    "status",
    "target",
    "archive",
    "accepted_at",
    "authority",
    "notarization_id",
    "notes",
}
required_strings = [
    "target",
    "archive",
    "accepted_at",
    "authority",
    "notarization_id",
]
placeholder_markers = (
    "todo",
    "tbd",
    "pending",
    "placeholder",
    "replace-me",
    "example.com",
    "example.test",
)

def filled(key):
    value = report.get(key)
    return isinstance(value, str) and bool(value.strip())

def not_placeholder(key):
    value = report.get(key)
    return isinstance(value, str) and not any(
        marker in value.lower() for marker in placeholder_markers
    )

checks = [
    set(report).issubset(allowed_keys),
    report.get("schema") == "kiana.macos-notarization.v1",
    report.get("status") == "accepted",
    report.get("target") == expected_target,
    report.get("archive") == expected_archive,
    all(filled(key) for key in required_strings),
    all(not_placeholder(key) for key in required_strings),
]
sys.exit(0 if all(checks) else 1)
PY
  then
    pass "macOS notarization proof commercial contract"
  else
    fail "macOS notarization proof failed commercial contract"
  fi
}

require_enterprise_manifest_contract() {
  local file="$1"
  local expected_version="$2"
  local current_dist_dir="$3"
  if [[ ! -f "$file" ]]; then
    return
  fi
  local python
  python="$(python_bin)"
  if [[ -z "$python" ]]; then
    fail "python3 or python is required to validate enterprise offline manifest"
    return
  fi
  if "$python" - "$file" "$expected_version" "$current_dist_dir" <<'PY'
import hashlib
import json
import sys
from pathlib import Path

path, expected_version, dist_dir_raw = sys.argv[1:4]
dist_dir = Path(dist_dir_raw)
with open(path, "r", encoding="utf-8") as handle:
    manifest = json.load(handle)

base_url = manifest.get("release_base_url")
artifacts = manifest.get("artifacts")
channels = manifest.get("channels")
placeholder_markers = (
    "github.com/kiana-project/kiana",
    "example.com",
    "example.test",
    "localhost",
    "127.0.0.1",
    "pending_",
    "blocked_",
    "dry_run",
)

def real_url(value):
    return (
        isinstance(value, str)
        and value.startswith("https://")
        and not any(marker in value.lower() for marker in placeholder_markers)
    )

def filled(item, key):
    value = item.get(key)
    return isinstance(value, str) and bool(value.strip())

def file_sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()

def first_checksum(path):
    with path.open("r", encoding="utf-8") as handle:
        for line in handle:
            parts = line.split()
            if parts:
                return parts[0]
    return ""

artifact_checks = []
if isinstance(artifacts, list):
    for item in artifacts:
        if not isinstance(item, dict):
            artifact_checks.append(False)
            continue
        url = item.get("url")
        archive = item.get("archive")
        local_path = item.get("local_path")
        checksum_path = item.get("checksum_path")
        binary_checksum_path = item.get("binary_checksum_path")
        archive_file = dist_dir / local_path if isinstance(local_path, str) else None
        archive_checksum_file = dist_dir / checksum_path if isinstance(checksum_path, str) else None
        binary_checksum_file = dist_dir / binary_checksum_path if isinstance(binary_checksum_path, str) else None
        checksum_matches = (
            archive_file is not None
            and archive_checksum_file is not None
            and binary_checksum_file is not None
            and archive_file.is_file()
            and archive_checksum_file.is_file()
            and binary_checksum_file.is_file()
            and item.get("sha256") == file_sha256(archive_file)
            and item.get("sha256") == first_checksum(archive_checksum_file)
            and item.get("binary_sha256") == first_checksum(binary_checksum_file)
        )
        artifact_checks.append(
            all(
                filled(item, key)
                for key in [
                    "target",
                    "archive",
                    "url",
                    "sha256",
                    "binary_sha256",
                    "local_path",
                    "checksum_path",
                    "binary_checksum_path",
                ]
            )
            and real_url(url)
            and isinstance(base_url, str)
            and url.startswith(base_url)
            and local_path == archive
            and checksum_path == f"{archive}.sha256"
            and binary_checksum_path == f"{archive[:-7]}.binary.sha256"
            and checksum_matches
        )

checks = [
    manifest.get("schema") == "kiana.enterprise.offline-manifest.v1",
    manifest.get("version") == expected_version,
    manifest.get("generated_by") == "scripts/generate-distribution-manifests.sh",
    real_url(base_url),
    isinstance(artifacts, list) and len(artifacts) > 0,
    bool(artifact_checks) and all(artifact_checks),
    isinstance(channels, dict),
    isinstance(channels, dict) and channels.get("github_releases") == "generated_from_release_base_url",
    isinstance(channels, dict) and channels.get("homebrew") == "generated",
    isinstance(channels, dict) and channels.get("winget") == "generated",
]
sys.exit(0 if all(checks) else 1)
PY
  then
    pass "enterprise offline manifest commercial contract"
  else
    fail "enterprise offline manifest failed commercial contract"
  fi
}

require_source_control_contract() {
  local file="$1"
  local expected_version="$2"
  if [[ ! -f "$file" ]]; then
    return
  fi
  local python
  python="$(python_bin)"
  if [[ -z "$python" ]]; then
    fail "python3 or python is required to validate source-control proof"
    return
  fi
  if "$python" - "$file" "$expected_version" <<'PY'
import json
import re
import sys

path, expected_version = sys.argv[1:3]
with open(path, "r", encoding="utf-8") as handle:
    proof = json.load(handle)

placeholder_markers = (
    "todo",
    "tbd",
    "pending",
    "placeholder",
    "replace-me",
    "example.com",
    "example.test",
)

def filled(key):
    value = proof.get(key)
    return isinstance(value, str) and bool(value.strip())

def not_placeholder(key):
    value = proof.get(key)
    return isinstance(value, str) and not any(
        marker in value.lower() for marker in placeholder_markers
    )

commit = proof.get("commit")
tagged_commit = proof.get("tagged_commit")
checks = [
    proof.get("schema") == "kiana.source-control-proof.v1",
    proof.get("version") == expected_version,
    proof.get("status") == "accepted",
    proof.get("accepted") is True,
    proof.get("pushed") is True,
    proof.get("reviewed") is True,
    proof.get("release_tag") == f"v{expected_version}",
    isinstance(commit, str) and re.fullmatch(r"[0-9a-f]{40}", commit) is not None,
    isinstance(tagged_commit, str) and re.fullmatch(r"[0-9a-f]{40}", tagged_commit) is not None,
    commit == tagged_commit,
    all(filled(key) for key in ["accepted_by", "accepted_at", "remote_url"]),
    all(not_placeholder(key) for key in ["accepted_by", "remote_url"]),
    isinstance(proof.get("remote_url"), str)
    and proof["remote_url"].startswith(("https://", "ssh://", "git@")),
]
sys.exit(0 if all(checks) else 1)
PY
  then
    pass "source-control proof commercial contract"
  else
    fail "source-control proof failed commercial contract"
  fi
}

require_commercial_proof_manifest_contract() {
  local file="$1"
  local expected_version="$2"
  local current_dist_dir="$3"
  if [[ ! -f "$file" ]]; then
    return
  fi
  local python
  python="$(python_bin)"
  if [[ -z "$python" ]]; then
    fail "python3 or python is required to validate commercial proof manifest"
    return
  fi
  if "$python" - "$file" "$expected_version" "$current_dist_dir" <<'PY'
import hashlib
import json
import sys
from pathlib import Path

path, expected_version, dist_dir_raw = sys.argv[1:4]
dist_dir = Path(dist_dir_raw)
with open(path, "r", encoding="utf-8") as handle:
    manifest = json.load(handle)

proofs = manifest.get("proofs")
if not isinstance(proofs, list):
    proofs = []
summary = manifest.get("summary")
if not isinstance(summary, dict):
    summary = {}

required_paths = {
    "proofs/source-control/source-control.json",
    "proofs/live-smoke/provider/model-catalog-live.json",
    "proofs/live-smoke/provider/model-smoke-live-tools.json",
    "proofs/live-smoke/remote/code-session-smoke.json",
    "proofs/entitlement/entitlement-proof.json",
    "proofs/product/product-acceptance.json",
    "proofs/release-ops/release-ops.json",
    "proofs/platform-security/platform-security-linux.json",
    "proofs/platform-security/platform-security-macos.json",
    "proofs/platform-security/platform-security-windows.json",
}
required_ids = {
    "source.control",
    "live.provider-catalog",
    "live.provider-smoke",
    "live.remote-code-session",
    "acceptance.entitlement",
    "acceptance.product",
    "acceptance.release-ops",
    "acceptance.platform-security.linux",
    "acceptance.platform-security.macos",
    "acceptance.platform-security.windows",
}

def normalize(value):
    text = str(value).replace("\\", "/")
    prefix = str(dist_dir).replace("\\", "/").rstrip("/") + "/"
    if text.startswith(prefix):
        return text[len(prefix):]
    marker = "/proofs/"
    if marker in text:
        return "proofs/" + text.split(marker, 1)[1]
    return text

def file_hash(file_path):
    digest = hashlib.sha256()
    with file_path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()

paths = {normalize(item.get("path", "")) for item in proofs if isinstance(item, dict)}
ids = {item.get("id") for item in proofs if isinstance(item, dict)}
platforms = set(summary.get("platforms") or [])
entries_valid = []
for item in proofs:
    if not isinstance(item, dict):
        entries_valid.append(False)
        continue
    normalized_path = normalize(item.get("path", ""))
    proof_file = dist_dir / normalized_path
    expected_sha = item.get("sha256")
    entries_valid.append(
        isinstance(item.get("id"), str)
        and item.get("id")
        and isinstance(item.get("schema"), str)
        and item.get("schema")
        and normalized_path.startswith("proofs/")
        and proof_file.is_file()
        and isinstance(expected_sha, str)
        and len(expected_sha) == 64
        and file_hash(proof_file) == expected_sha
        and "proof-templates" not in normalize(item.get("source", "")).lower()
    )

checks = [
    manifest.get("schema") == "kiana.commercial-proof-manifest.v1",
    manifest.get("version") == expected_version,
    isinstance(manifest.get("generated_at"), str) and bool(manifest["generated_at"].strip()),
    normalize(manifest.get("proof_root", "")).endswith("proofs"),
    required_paths.issubset(paths),
    required_ids.issubset(ids),
    {"linux", "macos", "windows"}.issubset(platforms),
    summary.get("proofs") == len(proofs),
    summary.get("accepted", 0) >= 7,
    summary.get("live", 0) >= 2,
    bool(entries_valid) and all(entries_valid),
]
if not all(checks):
    print(json.dumps(manifest, indent=2, sort_keys=True), file=sys.stderr)
sys.exit(0 if all(checks) else 1)
PY
  then
    pass "commercial proof manifest contract"
  else
    fail "commercial proof manifest failed contract"
  fi
}

has_windows_publishable_artifact() {
  local package="$1"
  [[ -f "${dist_dir}/${package}.zip" ]] || \
    [[ -f "${dist_dir}/${package}.msi" ]] || \
    [[ -f "${dist_dir}/${package}.exe" ]]
}

shopt -s nullglob
archives=("${dist_dir}/kiana-${version}-"*.tar.gz)
if (( ${#archives[@]} == 0 )); then
  fail "no release archives found in ${dist_dir}; run package-release on every release runner first"
fi

for archive in "${archives[@]}"; do
  filename="$(basename "$archive")"
  package="${filename%.tar.gz}"
  target="${package#kiana-${version}-}"
  signature_proof="${dist_dir}/${package}.signature.json"

  case "$target" in
    linux-*) seen_linux=1 ;;
    macos-*) seen_macos=1 ;;
    windows-*) seen_windows=1 ;;
  esac

  require_file "${archive}.sha256"
  require_file "${dist_dir}/${package}.binary.sha256"
  require_file "${archive}.sig"
  require_file "${dist_dir}/${package}.binary.sig"
  require_file "$signature_proof"
  require_json_pattern "$signature_proof" '"schema"[[:space:]]*:[[:space:]]*"kiana.release-signature.v1"' "release signature proof schema"
  require_json_pattern "$signature_proof" "\"target\"[[:space:]]*:[[:space:]]*\"${target}\"" "release signature target matches"
  require_json_pattern "$signature_proof" "\"archive\"[[:space:]]*:[[:space:]]*\"${filename}\"" "release signature archive matches"
  require_json_pattern "$signature_proof" '"archive_sha256"[[:space:]]*:' "release signature archive digest"
  require_json_pattern "$signature_proof" '"binary_sha256_file_sha256"[[:space:]]*:' "release signature binary checksum digest"
  reject_json_pattern "$signature_proof" '"signer"[[:space:]]*:[[:space:]]*"external-release-signer"' "default release signer placeholder"
  require_release_signature_contract "$signature_proof" "$target" "$filename" "${filename}.sig" "${package}.binary.sig" "$archive" "${dist_dir}/${package}.binary.sha256"
  verify_signature_file "$archive" "${archive}.sig" "${target} archive"
  verify_signature_file "${dist_dir}/${package}.binary.sha256" "${dist_dir}/${package}.binary.sig" "${target} binary checksum"

  if [[ "$target" == macos-* ]]; then
    notarization_proof="${dist_dir}/${package}.notarization.json"
    require_file "$notarization_proof"
    require_json_pattern "$notarization_proof" '"schema"[[:space:]]*:[[:space:]]*"kiana.macos-notarization.v1"' "macOS notarization proof schema"
    require_json_pattern "$notarization_proof" '"status"[[:space:]]*:[[:space:]]*"accepted"' "macOS notarization accepted"
    require_macos_notarization_contract "$notarization_proof" "$target" "$filename"
  fi

  if [[ "$target" == windows-* ]]; then
    if has_windows_publishable_artifact "$package"; then
      pass "Windows target has winget-compatible ZIP/MSI/EXE artifact"
    else
      fail "Windows target ${target} lacks a winget-compatible ZIP/MSI/EXE artifact"
    fi
  fi
done

if (( seen_linux == 1 )); then
  pass "Linux release artifact present"
else
  fail "Linux release artifact missing"
fi
if (( seen_macos == 1 )); then
  pass "macOS release artifact present"
else
  fail "macOS release artifact missing"
fi
if (( seen_windows == 1 )); then
  pass "Windows release artifact present"
else
  fail "Windows release artifact missing"
fi

if [[ -f "${manifest_dir}/homebrew/BLOCKED.md" ]]; then
  fail "Homebrew channel manifest is still blocked"
else
  homebrew_formulas=("${manifest_dir}/homebrew/"*.rb)
  if (( ${#homebrew_formulas[@]} > 0 )); then
    pass "Homebrew channel manifest is not blocked"
  else
    fail "Homebrew channel manifest is not blocked but no formula was generated"
  fi
fi

if [[ -f "${manifest_dir}/winget/BLOCKED.md" ]]; then
  fail "winget channel manifest is still blocked"
else
  winget_installer_manifests=("${manifest_dir}/winget/"*"/${version}/"*".installer.yaml")
  if (( ${#winget_installer_manifests[@]} > 0 )); then
    pass "winget channel manifest is not blocked"
  else
    fail "winget channel manifest is not blocked but no installer manifest was generated"
  fi
fi

enterprise_manifest="${manifest_dir}/enterprise/offline-manifest.json"
require_file "$enterprise_manifest"
require_json_pattern "$enterprise_manifest" '"schema"[[:space:]]*:[[:space:]]*"kiana.enterprise.offline-manifest.v1"' "enterprise offline manifest schema"
if [[ -f "$enterprise_manifest" ]] && grep -Eq 'pending_|dry_run|blocked_' "$enterprise_manifest"; then
  fail "enterprise offline manifest still contains pending/dry-run/blocked channel state"
elif [[ -f "$enterprise_manifest" ]]; then
  pass "enterprise offline manifest contains no pending channel states"
fi
require_enterprise_manifest_contract "$enterprise_manifest" "$version" "$dist_dir"

source_control_proof="${dist_dir}/proofs/source-control/source-control.json"
provider_catalog_proof="${dist_dir}/proofs/live-smoke/provider/model-catalog-live.json"
provider_smoke_proof="${dist_dir}/proofs/live-smoke/provider/model-smoke-live-tools.json"
remote_smoke_proof="${dist_dir}/proofs/live-smoke/remote/code-session-smoke.json"
entitlement_proof="${dist_dir}/proofs/entitlement/entitlement-proof.json"
product_acceptance_proof="${dist_dir}/proofs/product/product-acceptance.json"
release_ops_proof="${dist_dir}/proofs/release-ops/release-ops.json"
platform_security_proofs=("${dist_dir}/proofs/platform-security/"*.json)
commercial_proof_manifest="${dist_dir}/proofs/PROOF-MANIFEST.json"

require_proof_file "$commercial_proof_manifest" "kiana.commercial-proof-manifest.v1" "commercial proof manifest"
require_commercial_proof_manifest_contract "$commercial_proof_manifest" "$version" "$dist_dir"
require_proof_file "$source_control_proof" "kiana.source-control-proof.v1" "source-control proof"
require_json_pattern "$source_control_proof" '"status"[[:space:]]*:[[:space:]]*"accepted"' "source-control proof accepted"
require_json_pattern "$source_control_proof" '"accepted"[[:space:]]*:[[:space:]]*true' "source-control proof accepted flag"
require_json_pattern "$source_control_proof" '"pushed"[[:space:]]*:[[:space:]]*true' "source-control proof pushed"
require_json_pattern "$source_control_proof" '"reviewed"[[:space:]]*:[[:space:]]*true' "source-control proof reviewed"
require_source_control_contract "$source_control_proof" "$version"
require_proof_file "$provider_catalog_proof" "kiana.model-catalog.v1" "provider live catalog proof"
require_json_pattern "$provider_catalog_proof" '"live"[[:space:]]*:[[:space:]]*true' "provider live catalog proof is live"
require_proof_file "$provider_smoke_proof" "kiana.model-smoke.v1" "provider live smoke proof"
require_json_pattern "$provider_smoke_proof" '"live"[[:space:]]*:[[:space:]]*true' "provider live smoke proof is live"
require_json_pattern "$provider_smoke_proof" '"tools"[[:space:]]*:[[:space:]]*true' "provider live smoke proof includes tools"
require_provider_live_contract "$provider_catalog_proof" "$provider_smoke_proof"
require_proof_file "$remote_smoke_proof" "kiana.remote-code-session-smoke.v1" "remote live smoke proof"
require_json_pattern "$remote_smoke_proof" '"status"[[:space:]]*:[[:space:]]*"ok"' "remote live smoke proof status"
require_remote_smoke_contract "$remote_smoke_proof"
require_proof_file "$entitlement_proof" "kiana.entitlement-proof.v1" "entitlement proof"
require_json_pattern "$entitlement_proof" '"status"[[:space:]]*:[[:space:]]*"accepted"' "entitlement proof accepted"
require_json_pattern "$entitlement_proof" '"accepted"[[:space:]]*:[[:space:]]*true' "entitlement proof accepted flag"
require_json_pattern "$entitlement_proof" '"license_status"[[:space:]]*:[[:space:]]*"active"' "entitlement proof license active"
require_json_pattern "$entitlement_proof" '"entitlements"[[:space:]]*:' "entitlement proof has entitlements"
require_json_pattern "$entitlement_proof" '"backend"[[:space:]]*:' "entitlement proof has backend"
require_entitlement_contract "$entitlement_proof" "$version"
require_proof_file "$product_acceptance_proof" "kiana.product-acceptance.v1" "product acceptance proof"
require_json_pattern "$product_acceptance_proof" '"status"[[:space:]]*:[[:space:]]*"accepted"' "product acceptance proof accepted"
require_json_pattern "$product_acceptance_proof" '"accepted"[[:space:]]*:[[:space:]]*true' "product acceptance proof accepted flag"
require_product_acceptance_contract "$product_acceptance_proof" "$version"
require_proof_file "$release_ops_proof" "kiana.release-ops.v1" "release ops proof"
require_json_pattern "$release_ops_proof" '"status"[[:space:]]*:[[:space:]]*"accepted"' "release ops proof accepted"
require_json_pattern "$release_ops_proof" '"accepted"[[:space:]]*:[[:space:]]*true' "release ops proof accepted flag"
require_json_pattern "$release_ops_proof" '"vulnerability_report_channel"[[:space:]]*:' "release ops proof has vulnerability report channel"
require_json_pattern "$release_ops_proof" '"release_credentials_owner"[[:space:]]*:' "release ops proof has credentials owner"
require_json_pattern "$release_ops_proof" '"support_contact"[[:space:]]*:' "release ops proof has support contact"
require_json_pattern "$release_ops_proof" '"artifact_retention_days"[[:space:]]*:' "release ops proof has artifact retention"
require_json_pattern "$release_ops_proof" '"log_retention_days"[[:space:]]*:' "release ops proof has log retention"
require_json_pattern "$release_ops_proof" '"credential_review"[[:space:]]*:' "release ops proof has credential review"
require_release_ops_contract "$release_ops_proof" "$version"

seen_platform_linux=0
seen_platform_macos=0
seen_platform_windows=0
if (( ${#platform_security_proofs[@]} == 0 )); then
  fail "missing platform security proofs in ${dist_dir}/proofs/platform-security"
fi
for proof in "${platform_security_proofs[@]}"; do
  [[ -f "$proof" ]] || continue
  require_proof_file "$proof" "kiana.platform-security-proof.v1" "platform security proof"
  require_json_pattern "$proof" '"status"[[:space:]]*:[[:space:]]*"accepted"' "platform security proof accepted"
  require_json_pattern "$proof" '"accepted"[[:space:]]*:[[:space:]]*true' "platform security proof accepted flag"
  require_json_pattern "$proof" '"doctor_status"[[:space:]]*:[[:space:]]*"ready"' "platform security proof doctor ready"
  require_platform_security_contract "$proof" "$version"
  if grep -Eq '"platform"[[:space:]]*:[[:space:]]*"linux"' "$proof"; then
    seen_platform_linux=1
  fi
  if grep -Eq '"platform"[[:space:]]*:[[:space:]]*"macos"' "$proof"; then
    seen_platform_macos=1
  fi
  if grep -Eq '"platform"[[:space:]]*:[[:space:]]*"windows"' "$proof"; then
    seen_platform_windows=1
  fi
done
if (( seen_platform_linux == 1 )); then
  pass "Linux platform security proof present"
else
  fail "Linux platform security proof missing"
fi
if (( seen_platform_macos == 1 )); then
  pass "macOS platform security proof present"
else
  fail "macOS platform security proof missing"
fi
if (( seen_platform_windows == 1 )); then
  pass "Windows platform security proof present"
else
  fail "Windows platform security proof missing"
fi

if (( failures > 0 )); then
  echo "commercial release artifact verification failed with ${failures} issue(s)" >&2
  exit 1
fi

echo "commercial release artifact verification passed"
