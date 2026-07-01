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
    isinstance(credential_review.get("reviewed_at"), str)
    and bool(credential_review["reviewed_at"].strip()),
    isinstance(credential_review.get("scope"), str)
    and bool(credential_review["scope"].strip()),
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
  if [[ ! -f "$file" ]]; then
    return
  fi
  local python
  python="$(python_bin)"
  if [[ -z "$python" ]]; then
    fail "python3 or python is required to validate release signature proof"
    return
  fi
  if "$python" - "$file" "$expected_target" "$expected_archive" "$expected_archive_sig" "$expected_binary_sig" <<'PY'
import json
import sys

path, expected_target, expected_archive, expected_archive_sig, expected_binary_sig = sys.argv[1:6]
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

checks = [
    report.get("schema") == "kiana.release-signature.v1",
    report.get("target") == expected_target,
    report.get("archive") == expected_archive,
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
  reject_json_pattern "$signature_proof" '"signer"[[:space:]]*:[[:space:]]*"external-release-signer"' "default release signer placeholder"
  require_release_signature_contract "$signature_proof" "$target" "$filename" "${filename}.sig" "${package}.binary.sig"
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

provider_catalog_proof="${dist_dir}/proofs/live-smoke/provider/model-catalog-live.json"
provider_smoke_proof="${dist_dir}/proofs/live-smoke/provider/model-smoke-live-tools.json"
remote_smoke_proof="${dist_dir}/proofs/live-smoke/remote/code-session-smoke.json"
entitlement_proof="${dist_dir}/proofs/entitlement/entitlement-proof.json"
product_acceptance_proof="${dist_dir}/proofs/product/product-acceptance.json"
release_ops_proof="${dist_dir}/proofs/release-ops/release-ops.json"
platform_security_proofs=("${dist_dir}/proofs/platform-security/"*.json)

require_proof_file "$provider_catalog_proof" "kiana.model-catalog.v1" "provider live catalog proof"
require_json_pattern "$provider_catalog_proof" '"live"[[:space:]]*:[[:space:]]*true' "provider live catalog proof is live"
require_proof_file "$provider_smoke_proof" "kiana.model-smoke.v1" "provider live smoke proof"
require_json_pattern "$provider_smoke_proof" '"live"[[:space:]]*:[[:space:]]*true' "provider live smoke proof is live"
require_json_pattern "$provider_smoke_proof" '"tools"[[:space:]]*:[[:space:]]*true' "provider live smoke proof includes tools"
require_proof_file "$remote_smoke_proof" "kiana.remote-code-session-smoke.v1" "remote live smoke proof"
require_json_pattern "$remote_smoke_proof" '"status"[[:space:]]*:[[:space:]]*"ok"' "remote live smoke proof status"
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
