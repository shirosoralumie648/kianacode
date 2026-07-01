#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

mode="${1:-full}"
if [[ "$mode" != "full" && "$mode" != "--local-rc" ]]; then
  echo "usage: $0 [--local-rc]" >&2
  exit 2
fi

if command -v python3 >/dev/null 2>&1; then
  python_bin="python3"
elif command -v python >/dev/null 2>&1; then
  python_bin="python"
else
  echo "python3 or python is required for compliance audit" >&2
  exit 1
fi

out_dir="${COMPLIANCE_OUT_DIR:-dist/compliance}"
mkdir -p "$out_dir"

tool_root="${KIANA_COMPLIANCE_TOOL_ROOT:-target/compliance-tools}"
advisory_db="${KIANA_COMPLIANCE_ADVISORY_DB:-target/compliance-advisory-db}"
tool_cargo_home="${KIANA_COMPLIANCE_CARGO_HOME:-target/compliance-cargo-home}"
mkdir -p "$tool_cargo_home"
if [[ -d "$tool_root/bin" ]]; then
  export PATH="$PWD/$tool_root/bin:$tool_root/bin:$PATH"
fi

if [[ "$mode" == "full" && "${KIANA_COMPLIANCE_AUTO_INSTALL:-}" == "1" ]]; then
  bash scripts/install-compliance-tools.sh
  export PATH="$PWD/$tool_root/bin:$tool_root/bin:$PATH"
fi

metadata_file="$out_dir/cargo-metadata.json"
report_file="$out_dir/compliance-report.json"
sbom_file="$out_dir/sbom.cdx.json"

cargo_metadata_args=(metadata --locked --format-version 1)
if [[ "$mode" != "full" || "${KIANA_COMPLIANCE_OFFLINE:-}" == "1" ]]; then
  cargo_metadata_args+=(--offline)
fi
cargo "${cargo_metadata_args[@]}" > "$metadata_file"

if [[ "$mode" == "full" && "${KIANA_COMPLIANCE_OFFLINE:-}" != "1" ]]; then
  KIANA_CARGO_OFFLINE=0 bash scripts/generate-sbom.sh "$sbom_file" >/dev/null
else
  KIANA_CARGO_OFFLINE=1 bash scripts/generate-sbom.sh "$sbom_file" >/dev/null
fi

"$python_bin" - "$metadata_file" "$sbom_file" "$report_file" "$mode" <<'PY'
import json
import pathlib
import sys

metadata_path = pathlib.Path(sys.argv[1])
sbom_path = pathlib.Path(sys.argv[2])
report_path = pathlib.Path(sys.argv[3])
mode = sys.argv[4]

metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
json.loads(sbom_path.read_text(encoding="utf-8"))

workspace_members = set(metadata.get("workspace_members", []))
workspace_packages = [p for p in metadata.get("packages", []) if p.get("id") in workspace_members]
third_party_packages = [p for p in metadata.get("packages", []) if p.get("id") not in workspace_members]

workspace_missing = []
for package in workspace_packages:
    missing = [
        field
        for field in ("license", "repository", "rust_version")
        if not package.get(field)
    ]
    if package.get("publish") not in ([], None):
        missing.append("publish=false")
    if missing:
        workspace_missing.append({"name": package.get("name"), "missing": missing})

third_party_missing_license = [
    {"name": p.get("name"), "version": p.get("version")}
    for p in third_party_packages
    if not p.get("license") and not p.get("license_file")
]

report = {
    "mode": mode,
    "workspace_package_count": len(workspace_packages),
    "third_party_package_count": len(third_party_packages),
    "workspace_metadata_missing": workspace_missing,
    "third_party_missing_license_count": len(third_party_missing_license),
    "third_party_missing_license_sample": third_party_missing_license[:50],
    "sbom": str(sbom_path),
}

report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

if workspace_missing:
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    raise SystemExit("workspace package metadata is incomplete")

if mode == "full" and third_party_missing_license:
    print(json.dumps(report, indent=2, sort_keys=True), file=sys.stderr)
    raise SystemExit("third-party dependency license metadata is incomplete")
PY

missing_optional=0

if command -v cargo-audit >/dev/null 2>&1; then
  CARGO_HOME="$tool_cargo_home" cargo audit --db "$advisory_db" > "$out_dir/cargo-audit.txt" 2>&1
else
  echo "cargo-audit is not installed; run scripts/install-compliance-tools.sh or set KIANA_COMPLIANCE_AUTO_INSTALL=1" > "$out_dir/cargo-audit.txt"
  missing_optional=$((missing_optional + 1))
fi

if command -v cargo-deny >/dev/null 2>&1; then
  CARGO_HOME="$tool_cargo_home" cargo deny check > "$out_dir/cargo-deny.txt" 2>&1
else
  echo "cargo-deny is not installed; run scripts/install-compliance-tools.sh or set KIANA_COMPLIANCE_AUTO_INSTALL=1" > "$out_dir/cargo-deny.txt"
  missing_optional=$((missing_optional + 1))
fi

if [[ "$mode" == "full" && "$missing_optional" -gt 0 ]]; then
  echo "full compliance audit requires cargo-audit and cargo-deny" >&2
  exit 1
fi

echo "Compliance audit artifacts written to $out_dir"
