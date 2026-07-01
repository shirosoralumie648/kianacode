#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

output="${1:-dist/sbom.cdx.json}"
mkdir -p "$(dirname "$output")"

if command -v python3 >/dev/null 2>&1; then
  python_bin="python3"
elif command -v python >/dev/null 2>&1; then
  python_bin="python"
else
  echo "python3 or python is required to generate the SBOM" >&2
  exit 1
fi

metadata_file="$(mktemp)"
cargo metadata --locked --offline --format-version 1 > "$metadata_file"

"$python_bin" - "$metadata_file" "$output" <<'PY'
import json
import pathlib
import sys
import uuid

metadata_path = pathlib.Path(sys.argv[1])
output_path = pathlib.Path(sys.argv[2])
metadata = json.loads(metadata_path.read_text(encoding="utf-8"))

workspace_members = set(metadata.get("workspace_members", []))
packages = sorted(metadata.get("packages", []), key=lambda p: (p.get("name", ""), p.get("version", "")))

components = []
for package in packages:
    name = package.get("name")
    version = package.get("version")
    source = package.get("source")
    component = {
        "type": "library",
        "name": name,
        "version": version,
        "bom-ref": package.get("id"),
    }
    license_id = package.get("license")
    if license_id:
        component["licenses"] = [{"license": {"id": license_id}}]
    repository = package.get("repository")
    if repository:
        component["externalReferences"] = [{"type": "vcs", "url": repository}]
    if source and source.startswith("registry+"):
        component["purl"] = f"pkg:cargo/{name}@{version}"
    if package.get("id") in workspace_members:
        component["scope"] = "required"
    components.append(component)

root_name = metadata.get("workspace_root", "kiana")
bom = {
    "bomFormat": "CycloneDX",
    "specVersion": "1.5",
    "serialNumber": f"urn:uuid:{uuid.uuid4()}",
    "version": 1,
    "metadata": {
        "component": {
            "type": "application",
            "name": "kiana",
            "version": pathlib.Path("VERSION").read_text(encoding="utf-8").strip(),
            "bom-ref": root_name,
        }
    },
    "components": components,
}

output_path.write_text(json.dumps(bom, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY

echo "Generated $output"
