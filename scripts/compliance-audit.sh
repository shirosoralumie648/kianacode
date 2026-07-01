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

metadata_file="$out_dir/cargo-metadata.json"
report_file="$out_dir/compliance-report.json"
sbom_file="$out_dir/sbom.cdx.json"

cargo metadata --locked --offline --format-version 1 > "$metadata_file"
bash scripts/generate-sbom.sh "$sbom_file" >/dev/null

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
  cargo audit --locked > "$out_dir/cargo-audit.txt"
else
  echo "cargo-audit is not installed" > "$out_dir/cargo-audit.txt"
  missing_optional=$((missing_optional + 1))
fi

if command -v cargo-deny >/dev/null 2>&1; then
  cargo deny check > "$out_dir/cargo-deny.txt"
else
  echo "cargo-deny is not installed" > "$out_dir/cargo-deny.txt"
  missing_optional=$((missing_optional + 1))
fi

if [[ "$mode" == "full" && "$missing_optional" -gt 0 ]]; then
  echo "full compliance audit requires cargo-audit and cargo-deny" >&2
  exit 1
fi

echo "Compliance audit artifacts written to $out_dir"
