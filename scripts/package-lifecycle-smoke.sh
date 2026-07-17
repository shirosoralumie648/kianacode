#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

version="${VERSION:-$(tr -d '\r\n' < VERSION)}"
dist_dir="${DIST_DIR:-dist}"
archive="${PACKAGE_ARCHIVE:-}"

case "$(uname -s)" in
  Linux*) os="linux"; exe_ext="" ;;
  Darwin*) os="macos"; exe_ext="" ;;
  MSYS*|MINGW*|CYGWIN*) os="windows"; exe_ext=".exe" ;;
  *) echo "unsupported OS: $(uname -s)" >&2; exit 1 ;;
esac

case "$(uname -m)" in
  x86_64|amd64) arch="x86_64" ;;
  aarch64|arm64) arch="aarch64" ;;
  *) echo "unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac
host_package_target="${os}-${arch}"

if [[ -z "$archive" ]]; then
  shopt -s nullglob
  matches=("$dist_dir"/kiana-"$version"-*.tar.gz)
  shopt -u nullglob
  if [[ "${#matches[@]}" -ne 1 ]]; then
    echo "expected exactly one kiana-${version}-*.tar.gz in $dist_dir; found ${#matches[@]}" >&2
    echo "run DIST_DIR=\"$dist_dir\" scripts/package-release.sh first, or set PACKAGE_ARCHIVE" >&2
    exit 2
  fi
  archive="${matches[0]}"
fi

if [[ ! -f "$archive" ]]; then
  echo "package archive not found: $archive" >&2
  exit 2
fi

archive_dir="$(cd "$(dirname "$archive")" && pwd)"
archive_name="$(basename "$archive")"
package_name="${archive_name%.tar.gz}"
archive_target="${package_name#kiana-${version}-}"
if [[ "$archive_target" == "$package_name" || "$archive_target" != "$host_package_target" ]]; then
  echo "archive target does not match host package target: archive=${archive_target:-unknown} host=$host_package_target" >&2
  exit 1
fi
archive_sha="$archive_dir/${archive_name}.sha256"
binary_sha="$archive_dir/${package_name}.binary.sha256"

verify_checksum_file() {
  local checksum_file="$1"
  if [[ ! -f "$checksum_file" ]]; then
    echo "checksum file not found: $checksum_file" >&2
    exit 1
  fi
  if command -v sha256sum >/dev/null 2>&1; then
    (cd "$archive_dir" && sha256sum -c "$(basename "$checksum_file")" >/dev/null)
  elif command -v shasum >/dev/null 2>&1; then
    local expected target actual
    read -r expected target < "$checksum_file"
    actual="$(shasum -a 256 "$archive_dir/$target" | awk '{print $1}')"
    [[ "$actual" == "$expected" ]]
  else
    echo "no sha256sum or shasum available" >&2
    exit 1
  fi
}

file_hash() {
  local file="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$file" | awk '{print $1}'
  else
    shasum -a 256 "$file" | awk '{print $1}'
  fi
}

verify_binary_format() {
  local binary="$1"
  local python_bin
  python_bin="$(command -v python3 2>/dev/null || command -v python 2>/dev/null || true)"
  if [[ -z "$python_bin" ]]; then
    echo "python3 or python is required to verify packaged binary format" >&2
    exit 1
  fi
  "$python_bin" - "$binary" "$os" "$arch" <<'PY'
import struct
import sys
from pathlib import Path

path = Path(sys.argv[1])
expected_os = sys.argv[2]
expected_arch = sys.argv[3]
data = path.read_bytes()

def reject(message):
    print(f"packaged binary format mismatch: {path}: {message}", file=sys.stderr)
    raise SystemExit(1)

if expected_os == "linux":
    if len(data) < 20 or data[:4] != b"\x7fELF" or data[4] != 2:
        reject("expected a 64-bit ELF binary")
    endian = "<" if data[5] == 1 else ">" if data[5] == 2 else None
    if endian is None:
        reject("invalid ELF byte order")
    machine = struct.unpack_from(f"{endian}H", data, 18)[0]
    expected = {"x86_64": 62, "aarch64": 183}[expected_arch]
elif expected_os == "macos":
    if len(data) < 8 or data[:4] not in {b"\xcf\xfa\xed\xfe", b"\xfe\xed\xfa\xcf"}:
        reject("expected a 64-bit Mach-O binary")
    endian = "<" if data[:4] == b"\xcf\xfa\xed\xfe" else ">"
    machine = struct.unpack_from(f"{endian}I", data, 4)[0]
    expected = {"x86_64": 0x01000007, "aarch64": 0x0100000C}[expected_arch]
elif expected_os == "windows":
    if len(data) < 64 or data[:2] != b"MZ":
        reject("expected a PE binary")
    pe_offset = struct.unpack_from("<I", data, 0x3C)[0]
    if pe_offset + 6 > len(data) or data[pe_offset:pe_offset + 4] != b"PE\0\0":
        reject("invalid PE header")
    machine = struct.unpack_from("<H", data, pe_offset + 4)[0]
    expected = {"x86_64": 0x8664, "aarch64": 0xAA64}[expected_arch]
else:
    reject(f"unsupported expected OS {expected_os}")

if machine != expected:
    reject(f"expected {expected_arch} machine {expected:#x}, found {machine:#x}")
PY
}

native_env_path() {
  local path="$1"
  if command -v cygpath >/dev/null 2>&1; then
    cygpath -w "$path"
  else
    printf '%s\n' "$path"
  fi
}

tmp_root="$(mktemp -d)"
tmp_root="$(cd "$tmp_root" && pwd)"
touch "$tmp_root/.kiana-package-lifecycle-smoke"
cleanup() {
  if [[ -n "${tmp_root:-}" && -f "$tmp_root/.kiana-package-lifecycle-smoke" ]]; then
    rm -rf "$tmp_root"
  fi
}
trap cleanup EXIT

verify_checksum_file "$archive_sha"
verify_checksum_file "$binary_sha"

extract_dir="$tmp_root/extract"
mkdir -p "$extract_dir"
tar -xzf "$archive" -C "$extract_dir"

package_root="$extract_dir/$package_name"
if [[ ! -d "$package_root" ]]; then
  echo "expected package root not found after extraction: $package_root" >&2
  exit 1
fi
verify_binary_format "$package_root/kiana${exe_ext}"

for file in \
  "$package_root/kiana${exe_ext}" \
  "$package_root/SBOM.cdx.json" \
  "$package_root/docs/compliance-report.json" \
  "$package_root/docs/reference-migration-roadmap.md" \
  "$package_root/docs/reference-feature-matrix.md" \
  "$package_root/docs/reference_audit/kiana_capability_matrix.md" \
  "$package_root/docs/proof-templates/README.md" \
  "$package_root/docs/proof-templates/product-acceptance.example.json" \
  "$package_root/docs/proof-templates/entitlement-proof.example.json" \
  "$package_root/docs/proof-templates/release-ops.example.json" \
  "$package_root/docs/proof-templates/platform-security.example.json" \
  "$package_root/docs/sdk-runtime-events.md" \
  "$package_root/docs/schemas/kiana-app-server-conversations.v1.schema.json" \
  "$package_root/docs/schemas/kiana-app-server-config-resolved.v1.schema.json" \
  "$package_root/docs/schemas/kiana-app-server-contract.v1.schema.json" \
  "$package_root/docs/schemas/kiana-app-server-events.v1.schema.json" \
  "$package_root/docs/schemas/kiana-app-server-git-status.v1.schema.json" \
  "$package_root/docs/schemas/kiana-app-server-distribution-review.v1.schema.json" \
  "$package_root/docs/schemas/kiana-app-server-model-current.v1.schema.json" \
  "$package_root/docs/schemas/kiana-app-server-permissions-status.v1.schema.json" \
  "$package_root/docs/schemas/kiana-app-server-plugins.v1.schema.json" \
  "$package_root/docs/schemas/kiana-app-server-sandbox.v1.schema.json" \
  "$package_root/docs/schemas/kiana-app-server-secrets.v1.schema.json" \
  "$package_root/docs/schemas/kiana-app-server-settings.v1.schema.json" \
  "$package_root/docs/schemas/kiana-app-server-prompt-history.v1.schema.json" \
  "$package_root/docs/schemas/kiana-app-server-team-status.v1.schema.json" \
  "$package_root/docs/schemas/kiana-team-plan.v1.schema.json" \
  "$package_root/docs/schemas/kiana-app-server-trust-status.v1.schema.json" \
  "$package_root/docs/schemas/kiana-app-server-commands.v1.schema.json" \
  "$package_root/docs/schemas/kiana-app-server-command-run.v1.schema.json" \
  "$package_root/docs/schemas/kiana-context-artifact-dependency-graph.v1.schema.json" \
  "$package_root/docs/schemas/kiana-context-artifact-ingest.v1.schema.json" \
  "$package_root/docs/schemas/kiana-context-artifact-readiness.v1.schema.json" \
  "$package_root/docs/schemas/kiana-context-artifact-store.v1.schema.json" \
  "$package_root/docs/schemas/kiana-context-index.v1.schema.json" \
  "$package_root/docs/schemas/kiana-context-search.v1.schema.json" \
  "$package_root/docs/schemas/kiana-context-vector-search.v1.schema.json" \
  "$package_root/docs/schemas/kiana-context-pack.v1.schema.json" \
  "$package_root/docs/schemas/kiana-commercial-proof-manifest.v1.schema.json" \
  "$package_root/docs/schemas/kiana-commercial-release-blockers.v1.schema.json" \
  "$package_root/docs/schemas/kiana-eda-review.v1.schema.json" \
  "$package_root/docs/schemas/kiana-eval-baseline.v1.schema.json" \
  "$package_root/docs/schemas/kiana-eval-report.v1.schema.json" \
  "$package_root/docs/schemas/kiana-eval-suite.v1.schema.json" \
  "$package_root/docs/schemas/kiana-local-rc-evidence.v1.schema.json" \
  "$package_root/docs/schemas/kiana-runtime-event.v1.schema.json" \
  "$package_root/docs/schemas/kiana-enterprise-offline-manifest.v1.schema.json" \
  "$package_root/docs/schemas/kiana-entitlement-proof.v1.schema.json" \
  "$package_root/docs/schemas/kiana-license-status.v1.schema.json" \
  "$package_root/docs/schemas/kiana-managed-plugin-policy.v1.schema.json" \
  "$package_root/docs/schemas/kiana-plugin-app-manifest.v1.schema.json" \
  "$package_root/docs/schemas/kiana-tasks.v1.schema.json" \
  "$package_root/docs/schemas/kiana-model-list.v1.schema.json" \
  "$package_root/docs/schemas/kiana-model-smoke.v1.schema.json" \
  "$package_root/docs/schemas/kiana-macos-notarization.v1.schema.json" \
  "$package_root/docs/schemas/kiana-plugin-install-receipt.v1.schema.json" \
  "$package_root/docs/schemas/kiana-product-acceptance.v1.schema.json" \
  "$package_root/docs/schemas/kiana-platform-security-proof.v1.schema.json" \
  "$package_root/docs/schemas/kiana-release-signature.v1.schema.json" \
  "$package_root/docs/schemas/kiana-release-workflow-proof.v1.schema.json" \
  "$package_root/docs/schemas/kiana-remote-code-session-smoke.v1.schema.json" \
  "$package_root/docs/schemas/kiana-release-ops.v1.schema.json" \
  "$package_root/docs/eval/fixtures/basic-runtime-suite.json" \
  "$package_root/docs/eval/fixtures/basic-runtime-baseline.json" \
  "$package_root/docs/eval/fixtures/basic-tool-success.jsonl" \
  "$package_root/docs/eval/fixtures/basic-tool-failure.jsonl" \
  "$package_root/scripts/install-release-binary.sh" \
  "$package_root/scripts/validate-json-schema.py" \
  "$package_root/scripts/schema-contract-smoke.sh" \
  "$package_root/scripts/commercial-release-blockers-report.sh" \
  "$package_root/scripts/source-control-proof-report.sh" \
  "$package_root/scripts/distribution-review-report.sh" \
  "$package_root/scripts/local-rc-evidence-report.sh" \
  "$package_root/scripts/commercial-release-handoff-smoke.sh" \
  "$package_root/scripts/stage-commercial-release-proofs.sh" \
  "$package_root/scripts/entitlement-proof-report.sh" \
  "$package_root/scripts/product-acceptance-report.sh" \
  "$package_root/scripts/release-ops-report.sh" \
  "$package_root/scripts/platform-security-proof-report.sh" \
  "$package_root/scripts/provider-live-smoke.sh" \
  "$package_root/scripts/remote-live-smoke.sh" \
  "$package_root/scripts/sign-release-artifacts.sh" \
  "$package_root/scripts/verify-commercial-release-artifacts.sh" \
  "$package_root/scripts/release-signature-verification-smoke.sh"
do
  if [[ ! -f "$file" ]]; then
    echo "package missing required file: $file" >&2
    exit 1
  fi
done

for fixture in docs/eval/fixtures/*; do
  packaged_fixture="$package_root/docs/eval/fixtures/$(basename "$fixture")"
  if [[ ! -f "$packaged_fixture" ]]; then
    echo "package eval fixture missing: $packaged_fixture" >&2
    exit 1
  fi
  if ! cmp -s "$fixture" "$packaged_fixture"; then
    echo "package eval fixture differs from source: $packaged_fixture" >&2
    exit 1
  fi
done

for source_doc in docs/reference-migration-roadmap.md docs/reference-feature-matrix.md docs/reference_audit/*.md; do
  packaged_doc="$package_root/$source_doc"
  if [[ ! -f "$packaged_doc" ]]; then
    echo "package reference evidence missing: $packaged_doc" >&2
    exit 1
  fi
  if ! cmp -s "$source_doc" "$packaged_doc"; then
    echo "package reference evidence differs from source: $packaged_doc" >&2
    exit 1
  fi
done

for schema in docs/schemas/*.json; do
  packaged_schema="$package_root/docs/schemas/$(basename "$schema")"
  if [[ ! -f "$packaged_schema" ]]; then
    echo "package schema file missing: $packaged_schema" >&2
    exit 1
  fi
  if ! cmp -s "$schema" "$packaged_schema"; then
    echo "package schema file differs from source: $packaged_schema" >&2
    exit 1
  fi
done

package_python_bin="$(command -v python3 2>/dev/null || command -v python 2>/dev/null || true)"
if [[ -z "$package_python_bin" ]]; then
  echo "python3 or python is required to validate packaged JSON schema contracts" >&2
  exit 1
fi
bash "$package_root/scripts/schema-contract-smoke.sh" >/dev/null

eval_output="$(
  "$package_root/kiana${exe_ext}" eval run \
    --suite "$package_root/docs/eval/fixtures/basic-runtime-suite.json" \
    --baseline "$package_root/docs/eval/fixtures/basic-runtime-baseline.json" \
    --json \
    --fail-on-failure
)"
KIANA_EVAL_REPORT="$eval_output" "$package_python_bin" - <<'PY'
import json
import os
import sys

try:
    report = json.loads(os.environ["KIANA_EVAL_REPORT"])
except Exception as exc:
    print(f"packaged eval output is not valid JSON: {exc}", file=sys.stderr)
    sys.exit(1)

if report.get("schema") != "kiana.eval-report.v1":
    print("packaged eval report schema mismatch", file=sys.stderr)
    sys.exit(1)
if report.get("status") != "passed":
    print("packaged eval suite did not pass", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)
baseline = report.get("baseline")
if not baseline or baseline.get("schema") != "kiana.eval-baseline.v1":
    print("packaged eval baseline missing", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)
if baseline.get("status") != "passed":
    print("packaged eval baseline did not pass", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)
PY

eda_project="$extract_dir/eda-project"
mkdir -p "$eda_project/hardware/gerber"
printf '%s\n' '5 V input; 3.3 V rail; SWD bring-up' > "$eda_project/hardware/requirements.md"
printf '%s\n' '(kicad_sch (version 20231120) (generator package-smoke))' > "$eda_project/hardware/main.kicad_sch"
printf '%s\n' 'Designator,MPN,Package,Quantity' 'R1,RC0402FR-0710KL,0402,1' 'U1,STM32F103C8T6,LQFP48,1' > "$eda_project/hardware/bom.csv"
printf '%s\n' 'Designator,Package,Mid X,Mid Y,Rotation,Layer' 'R1,0402,10,8,0,Top' 'U1,LQFP48,20,15,90,Top' > "$eda_project/hardware/cpl.csv"
printf '%s\n' 'G04 copper*' 'M02*' > "$eda_project/hardware/gerber/demo-F_Cu.gbr"
printf '%s\n' 'G04 edge*' 'M02*' > "$eda_project/hardware/gerber/demo-Edge_Cuts.gbr"
printf '%s\n' 'M48' 'M30' > "$eda_project/hardware/gerber/demo-PTH.drl"
printf '%s\n' '2 layers; minimum trace 0.15 mm' > "$eda_project/hardware/constraints.md"
printf '%s\n' \
  '<?xml version="1.0" encoding="UTF-8"?>' \
  '<export>' \
  '  <components>' \
  '    <comp ref="R1"><value>10k</value></comp>' \
  '    <comp ref="U1"><value>STM32F103C8T6</value></comp>' \
  '  </components>' \
  '  <nets>' \
  '    <net code="1" name="+3V3">' \
  '      <node ref="U1" pin="1" pintype="power_in" />' \
  '      <node ref="U1" pin="2" pintype="power_out" />' \
  '    </net>' \
  '    <net code="2" name="SWDIO">' \
  '      <node ref="U1" pin="3" pintype="bidirectional" />' \
  '      <node ref="R1" pin="1" pintype="passive" />' \
  '    </net>' \
  '  </nets>' \
  '</export>' > "$eda_project/hardware/main.xml"
package_binary="$(cd "$package_root" && pwd)/kiana${exe_ext}"
eda_output="$(
  cd "$eda_project"
  "$package_binary" eda review --json \
    --requirements hardware/requirements.md \
    --schematic hardware/main.kicad_sch \
    --bom hardware/bom.csv \
    --gerber hardware/gerber \
    --cpl hardware/cpl.csv \
    --constraints hardware/constraints.md \
    --netlist hardware/main.xml
)"
eda_identity="$(
  KIANA_EDA_REPORT="$eda_output" \
    KIANA_EDA_PROJECT="$(native_env_path "$eda_project")" \
    "$package_python_bin" - <<'PY'
import json
import os
import sys
from pathlib import Path

try:
    report = json.loads(os.environ["KIANA_EDA_REPORT"])
except Exception as exc:
    print(f"packaged EDA output is not valid JSON: {exc}", file=sys.stderr)
    sys.exit(1)
if report.get("schema") != "kiana.eda-review.v1" or report.get("status") != "pass":
    print("packaged EDA review did not pass", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)
summary = report.get("summary", {})
expected_summary = {
    "netlist_components": 2,
    "net_count": 2,
    "power_net_count": 1,
    "interface_net_count": 1,
    "dangling_net_count": 0,
}
if report.get("rule_version") != "eda-review-rules.v2" or any(
    summary.get(key) != value for key, value in expected_summary.items()
):
    print("packaged EDA netlist summary mismatch", file=sys.stderr)
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)
if not any(source.get("kind") == "netlist" for source in report.get("sources", [])):
    print("packaged EDA netlist source missing", file=sys.stderr)
    sys.exit(1)
if not any(
    check.get("check_id") == "netlist_structure" and check.get("status") == "pass"
    for check in report.get("checks", [])
):
    print("packaged EDA netlist_structure pass check missing", file=sys.stderr)
    sys.exit(1)
root = Path(os.environ["KIANA_EDA_PROJECT"])
run_dir = root / ".kiana" / "workflows" / report["run_id"]
review_dir = run_dir / "eda" / "reviews" / report["review_id"]
for name in ("eda_review.json", "bom_risk.md", "bringup-plan.md"):
    if not (review_dir / name).is_file():
        print(f"packaged EDA artifact missing: {name}", file=sys.stderr)
        sys.exit(1)
artifact_report = json.loads((review_dir / "eda_review.json").read_text(encoding="utf-8"))
if artifact_report != report:
    print("packaged EDA CLI output differs from persisted eda_review.json", file=sys.stderr)
    sys.exit(1)

eventlog_path = run_dir / "eventlog.jsonl"
if not eventlog_path.is_file():
    print(f"packaged EDA eventlog missing: {eventlog_path}", file=sys.stderr)
    sys.exit(1)
event_kinds = set()
for line_number, line in enumerate(eventlog_path.read_text(encoding="utf-8").splitlines(), start=1):
    if not line.strip():
        continue
    try:
        record = json.loads(line)
    except json.JSONDecodeError as exc:
        print(f"packaged EDA eventlog line {line_number} is invalid JSON: {exc}", file=sys.stderr)
        sys.exit(1)
    event = record.get("event", record) if isinstance(record, dict) else None
    if isinstance(event, dict) and isinstance(event.get("kind"), str):
        event_kinds.add(event["kind"])
required_events = {"artifact_written", "evidence_recorded", "verification_completed"}
missing_events = sorted(required_events - event_kinds)
if missing_events:
    print(f"packaged EDA eventlog missing events: {missing_events}", file=sys.stderr)
    sys.exit(1)

verification_dir = run_dir / "verification"
packet_paths = sorted(verification_dir.glob("*.json")) if verification_dir.is_dir() else []
if not packet_paths:
    print(f"packaged EDA verification packet missing: {verification_dir}", file=sys.stderr)
    sys.exit(1)
matching_packets = []
for packet_path in packet_paths:
    try:
        packet = json.loads(packet_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        print(f"packaged EDA verification packet is invalid JSON: {packet_path}: {exc}", file=sys.stderr)
        sys.exit(1)
    if (
        packet.get("profile") == "eda_review"
        and packet.get("run_id") == report.get("run_id")
        and packet.get("workflow_id") == report.get("workflow_id")
    ):
        matching_packets.append(packet)
if not any(packet.get("final_status") == "pass" for packet in matching_packets):
    print("packaged EDA pass VerificationPacket missing", file=sys.stderr)
    sys.exit(1)

print(report["run_id"], report["review_id"], sep="\t")
PY
)"
IFS=$'\t' read -r eda_run_id eda_review_id <<<"$eda_identity"
eda_review_path="$eda_project/.kiana/workflows/$eda_run_id/eda/reviews/$eda_review_id/eda_review.json"
"$package_python_bin" \
  "$(native_env_path "$package_root/scripts/validate-json-schema.py")" \
  "$(native_env_path "$package_root/docs/schemas/kiana-eda-review.v1.schema.json")" \
  "$(native_env_path "$eda_review_path")" >/dev/null

release_evidence_fixture="$tmp_root/release-evidence-fixture"
mkdir -p "$release_evidence_fixture/proofs"
cat > "$release_evidence_fixture/proofs/local-rc-evidence.json" <<'JSON'
{
  "schema": "kiana.local-rc-evidence.v1",
  "status": "local_rc_ready",
  "dist_dir": "package-lifecycle-fixture",
  "summary": {
    "release_artifacts": 1,
    "manifests": 2,
    "proofs": 3,
    "blockers_total": 4,
    "local_blockers": 0,
    "external_blockers": 4
  },
  "readiness": {
    "ready": true
  },
  "blockers": {
    "handoff_status": "external_action_required",
    "blocking_by_resolution_scope": {
      "release-owner": 2,
      "live-service": 2
    }
  }
}
JSON

install_dir="$tmp_root/install"
home_dir="$tmp_root/home"
mkdir -p "$install_dir" "$home_dir/.kiana"
installed="$install_dir/kiana${exe_ext}"
installer="$package_root/scripts/install-release-binary.sh"
home_env="$(native_env_path "$home_dir")"
kiana_home_env="$(native_env_path "$home_dir/.kiana")"
kiana_config_env="$(native_env_path "$home_dir/.kiana/config.toml")"

run_installed() {
  HOME="$home_env" \
  KIANA_HOME="$kiana_home_env" \
  KIANA_CONFIG_FILE="$kiana_config_env" \
  env \
    -u ANTHROPIC_AUTH_TOKEN \
    -u ANTHROPIC_API_KEY \
    -u ANTHROPIC_BASE_URL \
    -u ANTHROPIC_MODEL \
    -u KIANA_PROVIDER_SMOKE_LIVE \
    -u KIANA_LICENSE_FILE \
    -u KIANA_LICENSE_KEY \
    -u KIANA_LICENSE_PLAN \
    -u KIANA_LICENSE_ENTITLEMENTS \
    -u KIANA_LICENSE_OFFLINE \
    -u KIANA_ENTERPRISE_ACCOUNT_ID \
    -u KIANA_SUPPORT_CONTACT \
    -u KIANA_PROVIDER \
    -u KIANA_OPENAI_API_KEY \
    -u OPENAI_API_KEY \
    -u KIANA_REMOTE_ACCESS_TOKEN \
    -u CLAUDE_ACCESS_TOKEN \
  "$installed" "$@"
}

smoke_installed_project_trust() {
  local project="$tmp_root/installed-project-trust"
  local store_dir="$home_dir/.kiana/trust/projects"
  local unknown_json
  local trusted_json
  local reset_json

  mkdir -p "$project/.git" "$project/.kiana"
  printf '%s\n' '{"trusted":true}' > "$project/.kiana/trust.json"

  unknown_json="$(cd "$project" && run_installed trust json)"
  (cd "$project" && run_installed trust trust >/dev/null)
  trusted_json="$(cd "$project" && run_installed trust json)"
  (cd "$project" && run_installed trust reset >/dev/null)
  reset_json="$(cd "$project" && run_installed trust json)"

  PROJECT_TRUST_UNKNOWN_JSON="$unknown_json" \
  PROJECT_TRUST_TRUSTED_JSON="$trusted_json" \
  PROJECT_TRUST_RESET_JSON="$reset_json" \
  EXPECTED_PROJECT="$project" \
  EXPECTED_STORE_DIR="$store_dir" \
  "$package_python_bin" - <<'PY'
import json
import os
import re

def normalized(path):
    return str(path).replace("\\", "/").rstrip("/").casefold()

project = normalized(os.environ["EXPECTED_PROJECT"])
store_dir = normalized(os.environ["EXPECTED_STORE_DIR"])

for env_name, expected_trust, expected_source, expected_file_status in [
    ("PROJECT_TRUST_UNKNOWN_JSON", "unknown", "default", "missing"),
    ("PROJECT_TRUST_TRUSTED_JSON", "trusted", "user_store", "found"),
    ("PROJECT_TRUST_RESET_JSON", "unknown", "default", "missing"),
]:
    payload = json.loads(os.environ[env_name])
    file_info = payload["file"]
    legacy = payload["legacy_project_file"]
    trusted = expected_trust == "trusted"
    checks = [
        payload.get("schema") == "kiana.app-server.trust-status.v1",
        payload.get("project_trust") == expected_trust,
        payload.get("project_trusted") is trusted,
        payload.get("allows_project_resources") is trusted,
        payload.get("source") == expected_source,
        isinstance(payload.get("project_id"), str)
        and re.fullmatch(r"[0-9a-f]{64}", payload["project_id"]) is not None,
        normalized(payload.get("project_root", "")) == project,
        file_info.get("status") == expected_file_status,
        file_info.get("exists") is (expected_file_status == "found"),
        file_info.get("error") is None,
        normalized(file_info.get("path", "")).startswith(store_dir + "/"),
        normalized(file_info.get("path", ""))
        == normalized(store_dir + "/" + payload["project_id"] + ".json"),
        normalized(legacy.get("path", "")) == project + "/.kiana/trust.json",
        legacy.get("exists") is True,
        legacy.get("ignored") is True,
        legacy.get("reason") == "project_local_trust_is_not_authoritative",
    ]
    if not all(checks):
        raise SystemExit(
            f"installed project trust lifecycle assertion failed for {env_name}\n"
            + json.dumps(payload, indent=2, sort_keys=True)
        )
PY
}

INSTALL_DIR="$install_dir" bash "$installer" --install >/dev/null
[[ -x "$installed" ]]
run_installed --version >/dev/null
run_installed doctor --json | grep -Fq '"schema": "kiana.doctor.v1"'
run_installed doctor --json | grep -Fq '"reference_capabilities"'
run_installed license status --json | grep -Fq '"schema": "kiana.license-status.v1"'
run_installed license status --json | grep -Fq '"status": "missing"'
run_installed model list --json | grep -Fq '"provider_id": "openai-compatible"'
run_installed model smoke --json | grep -Fq '"schema": "kiana.model-smoke.v1"'
run_installed model smoke --json | grep -Fq '"tools": false'
run_installed model smoke --json | grep -Fq '"provider_id": "fake"'
run_installed model smoke --tools --json | grep -Fq '"tools": true'
run_installed model smoke --tools --json | grep -Fq '"capability": "tools"'
smoke_installed_project_trust

json_field() {
  JSON_DOCUMENT="$1" JSON_FIELD="$2" "$package_python_bin" - <<'PY'
import json
import os

document = json.loads(os.environ["JSON_DOCUMENT"])
field = os.environ["JSON_FIELD"]
value = document[field]
if not isinstance(value, (str, int)):
    raise SystemExit(f"JSON field {field} is not scalar")
print(value)
PY
}

workflow_fixture="$tmp_root/workflow-fixture"
mkdir -p "$workflow_fixture/scripts"
cat >"$workflow_fixture/scripts/release-smoke.sh" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
exit 0
SH
chmod +x "$workflow_fixture/scripts/release-smoke.sh"
run_installed tasks workflow integrity init --json >/dev/null
workflow_init="$(cd "$workflow_fixture" && run_installed tasks workflow init --json --type ship --profile gated 'Package lifecycle workflow completion')"
workflow_run_id="$(json_field "$workflow_init" run_id)"
workflow_artifact_dir="$workflow_fixture/.kiana/workflows/$workflow_run_id"
cat >"$workflow_artifact_dir/problem-definition.md" <<'MD'
# Package Lifecycle Workflow Evidence

This artifact records the real installed-binary transition, validation, completion, and integrity checks executed by package lifecycle smoke.
MD
for decision in clear not_needed fresh_enough ship report_only learned done; do
  transition_output="$(cd "$workflow_fixture" && run_installed tasks workflow advance --json --decision "$decision" --evidence problem-definition.md "$workflow_run_id")"
  grep -Fq '"schema": "kiana.workflow-transition.v1"' <<<"$transition_output"
done
validation_output="$(cd "$workflow_fixture" && run_installed validate --json --workflow "$workflow_run_id")"
grep -Fq '"final_status": "pass"' <<<"$validation_output"
workflow_verification_id="$(json_field "$validation_output" verification_id)"
completion_output="$(cd "$workflow_fixture" && run_installed tasks workflow complete --json --verification "$workflow_verification_id" "$workflow_run_id")"
grep -Fq '"schema": "kiana.workflow-completion.v1"' <<<"$completion_output"
grep -Fq '"status": "completed"' <<<"$completion_output"
workflow_integrity_output="$(cd "$workflow_fixture" && run_installed tasks workflow integrity verify --json "$workflow_run_id")"
grep -Fq '"status": "verified"' <<<"$workflow_integrity_output"
workflow_proof_output="$(cd "$workflow_fixture" && run_installed release workflow-proof --json --run-id "$workflow_run_id")"
grep -Fq '"schema": "kiana.release-workflow-proof.v1"' <<<"$workflow_proof_output"
workflow_latest_proof_output="$(cd "$workflow_fixture" && run_installed release workflow-proof --json --latest-completed --out dist/proofs/workflow/latest-recovery-integrity.json)"
grep -Fq '"mode": "latest_completed"' <<<"$workflow_latest_proof_output"
grep -Fq '"proof_status": "verified"' <<<"$workflow_proof_output"
test -f "$workflow_fixture/dist/proofs/workflow/recovery-integrity.json"
(
  cd "$package_root"
  KIANA_PYTHON_BIN="$package_python_bin" run_installed release blockers --json | grep -Fq '"schema": "kiana.commercial-release-blockers.v1"'
  KIANA_PYTHON_BIN="$package_python_bin" run_installed release blockers --json | grep -Fq '"local_blocking"'
  run_installed release evidence --json --dist-dir "$release_evidence_fixture" | grep -Fq '"schema": "kiana.local-rc-evidence.v1"'
  run_installed release evidence --json --dist-dir "$release_evidence_fixture" | grep -Fq '"local_blockers": 0'
)
context_fixture="$tmp_root/context-fixture"
context_artifact_root="$tmp_root/context-artifact-root"
mkdir -p "$context_fixture/src"
mkdir -p "$context_fixture/docs"
mkdir -p "$context_fixture/tests"
mkdir -p "$context_artifact_root/bundle"
printf '%s\n' 'pub fn lifecycle_search() {}' '// lifecycle lifecycle search' > "$context_fixture/src/lib.rs"
printf '%s\n' 'use kiana::lifecycle_search;' > "$context_fixture/tests/lib_test.rs"
printf '%s\n' 'first module summary referencing src/lib.rs' > "$context_fixture/docs/path-only.md"
printf '%s\n' 'first artifact line' > "$context_artifact_root/bundle/notes.md"
(
  cd "$context_fixture"
  run_installed context index --json | grep -Fq '"schema": "kiana.context-index.v1"'
  run_installed context index --json | grep -Fq '"path": "src/lib.rs"'
  run_installed context artifacts --json | grep -Fq '"schema": "kiana.context-artifacts.v1"'
  run_installed context artifacts --json | grep -Fq '"kind": "file"'
  run_installed context artifacts --json | grep -Fq '"path": "src/lib.rs"'
  run_installed context artifacts --json --cache .kiana/context-artifacts.json | grep -Fq '"status": "created"'
  run_installed context artifacts --json --cache .kiana/context-artifacts.json | grep -Fq '"reused_artifacts": 3'
  test -f .kiana/context-artifacts.json
  run_installed context ingest --source "$context_artifact_root" --json | grep -Fq '"schema": "kiana.context-artifact-ingest.v1"'
  run_installed context ingest --source "$context_artifact_root" --json | grep -Fq '"sync"'
  run_installed context ingest --source "$context_artifact_root" --json | grep -Fq '"source_path": "bundle/notes.md"'
  run_installed context ingest --source "$context_artifact_root" --json | grep -Fq '"stored_path": ".kiana/context-ingest/files/'
  run_installed context ingest --source "$context_artifact_root" --json | grep -Fq '"manifest_path": ".kiana/context-ingest/manifest.json"'
  test -f .kiana/context-ingest/manifest.json
  run_installed context artifact-graph --json | grep -Fq '"schema": "kiana.context-artifact-dependency-graph.v1"'
  run_installed context artifact-graph --json | grep -Fq '"relation": "test_of"'
  run_installed context artifact-graph --json | grep -Fq '"evidence": "tests/lib_test.rs matches src/lib.rs"'
  run_installed context artifact-graph --json | grep -Fq '"relation": "path_reference"'
  run_installed context artifact-graph --json | grep -Fq '"evidence": "docs/path-only.md references src/lib.rs"'
  run_installed context artifact-store --json | grep -Fq '"schema": "kiana.context-artifact-store.v1"'
  run_installed context artifact-store --json | grep -Fq '"artifact_count": 3'
  run_installed context artifact-store --json | grep -Fq '"dependency_count": 2'
  run_installed context artifact-store --json | grep -Fq '"role": "source"'
  run_installed context artifact-store --json | grep -Fq '"role": "test"'
  run_installed context artifact-store --json | grep -Fq '"dependency_graph_schema": "kiana.context-artifact-dependency-graph.v1"'
  run_installed context artifact-readiness --json | grep -Fq '"schema": "kiana.context-artifact-readiness.v1"'
  run_installed context artifact-readiness --json | grep -Fq '"status": "incomplete"'
  run_installed context artifact-readiness --json | grep -Fq '"missing_roles"'
  run_installed context artifact-store --json --cache .kiana/context-artifact-store.json | grep -Fq '"status": "created"'
  run_installed context artifact-store --json --cache .kiana/context-artifact-store.json | grep -Fq '"reused_artifacts": 3'
  run_installed context artifact-store --json --cache .kiana/context-artifact-store.json | grep -Fq '"reused_dependencies": 2'
  test -f .kiana/context-artifact-store.json
  run_installed context search lifecycle --json --limit 1 | grep -Fq '"schema": "kiana.context-search.v1"'
  run_installed context search lifecycle --json --limit 1 | grep -Fq '"path": "src/lib.rs"'
  run_installed context search docs/path-only.md --json --limit 1 | grep -Fq '"path": "docs/path-only.md"'
  run_installed context search docs/path-only.md --json --limit 1 | grep -Fq '"occurrences": 0'
  run_installed context vector-search lifecycle flow --json --limit 1 | grep -Fq '"schema": "kiana.context-vector-search.v1"'
  run_installed context vector-search lifecycle flow --json --limit 1 | grep -Fq '"embedding_model": "kiana.deterministic-hash-embedding.v1"'
  run_installed context vector-search lifecycle flow --json --limit 1 | grep -Fq '"path": "src/lib.rs"'
  run_installed context pack lifecycle --json --limit 1 --max-snippet-lines 1 | grep -Fq '"schema": "kiana.context-pack.v1"'
  run_installed context pack lifecycle --json --limit 1 --max-snippet-lines 1 | grep -Fq '"path": "src/lib.rs"'
  run_installed context pack lifecycle --json --limit 1 --max-snippet-lines 1 | grep -Fq '"schema": "kiana.context-artifact-graph.v1"'
  run_installed context pack lifecycle --json --limit 1 --max-snippet-lines 1 | grep -Fq '"relation": "matched"'
  run_installed context pack docs/path-only.md --json --limit 1 --max-snippet-lines 1 | grep -Fq '"excerpt": "first module summary referencing src/lib.rs"'
  run_installed context pack docs/path-only.md --json --limit 1 --max-snippet-lines 1 | grep -Fq '"schema": "kiana.context-artifact-graph.v1"'
  run_installed context pack bundle/notes.md --root "$context_artifact_root" --json --limit 1 --max-snippet-lines 1 | grep -Fq '"path": "bundle/notes.md"'
  run_installed context pack bundle/notes.md --root "$context_artifact_root" --json --limit 1 --max-snippet-lines 1 | grep -Fq '"excerpt": "first artifact line"'
  run_installed context pack bundle/notes.md --root "$context_artifact_root" --json --limit 1 --max-snippet-lines 1 | grep -Fq '"schema": "kiana.context-artifact-graph.v1"'
)

offline_manifest="$archive_dir/manifests/enterprise/offline-manifest.json"
if [[ -f "$offline_manifest" ]]; then
  python_bin="$(command -v python3 2>/dev/null || command -v python 2>/dev/null || true)"
  if [[ -z "$python_bin" ]]; then
    echo "python3 or python is required to validate $offline_manifest" >&2
    exit 1
  fi
  OFFLINE_MANIFEST="$offline_manifest" \
  PACKAGE_ARCHIVE_NAME="$archive_name" \
  PACKAGE_ARCHIVE_SHA="$(awk 'NF { print $1; exit }' "$archive_sha")" \
  PACKAGE_VERSION="$version" \
  "$python_bin" - <<'PY'
import json
import os
import sys

with open(os.environ["OFFLINE_MANIFEST"], "r", encoding="utf-8") as handle:
    manifest = json.load(handle)

archive_name = os.environ["PACKAGE_ARCHIVE_NAME"]
archive_sha = os.environ["PACKAGE_ARCHIVE_SHA"]
version = os.environ["PACKAGE_VERSION"]
artifacts = manifest.get("artifacts")
artifact = None
if isinstance(artifacts, list):
    artifact = next((item for item in artifacts if item.get("archive") == archive_name), None)

checks = [
    manifest.get("schema") == "kiana.enterprise.offline-manifest.v1",
    manifest.get("version") == version,
    isinstance(manifest.get("release_base_url"), str) and manifest["release_base_url"],
    artifact is not None,
    artifact is not None and artifact.get("sha256") == archive_sha,
    artifact is not None and archive_name in artifact.get("url", ""),
    artifact is not None and artifact.get("local_path") == archive_name,
    artifact is not None and artifact.get("checksum_path") == f"{archive_name}.sha256",
    artifact is not None and artifact.get("binary_checksum_path") == f"{archive_name[:-7]}.binary.sha256",
    manifest.get("generated_by") == "scripts/generate-distribution-manifests.sh",
    isinstance(manifest.get("channels"), dict),
]
if not all(checks):
    print("enterprise offline manifest failed package lifecycle checks", file=sys.stderr)
    print(json.dumps(manifest, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)
PY
fi

pre_upgrade_hash="$(file_hash "$installed")"
rollback_dir="$tmp_root/rollback"
mkdir -p "$rollback_dir"
cp "$installed" "$rollback_dir/kiana${exe_ext}"

INSTALL_DIR="$install_dir" bash "$installer" --install >/dev/null
post_upgrade_hash="$(file_hash "$installed")"
if [[ "$post_upgrade_hash" != "$pre_upgrade_hash" ]]; then
  echo "same-package upgrade changed binary checksum unexpectedly" >&2
  exit 1
fi

cp "$rollback_dir/kiana${exe_ext}" "$installed"
chmod +x "$installed"
rollback_hash="$(file_hash "$installed")"
if [[ "$rollback_hash" != "$pre_upgrade_hash" ]]; then
  echo "rollback restore did not match pre-upgrade checksum" >&2
  exit 1
fi
run_installed --version >/dev/null

INSTALL_DIR="$install_dir" bash "$installer" --uninstall >/dev/null
if [[ -e "$installed" ]]; then
  echo "uninstall left binary behind: $installed" >&2
  exit 1
fi
INSTALL_DIR="$install_dir" bash "$installer" --uninstall >/dev/null

echo "package lifecycle smoke passed for $archive_name"
