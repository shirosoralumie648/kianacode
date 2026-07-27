#!/usr/bin/env bash
# scripts/generate-license-summary.sh
# Phase 2 Wave 3 — Standalone dependency license compliance summary (D-13)
#
# Produces a per-package license table from cargo metadata (no network required
# when Cargo.lock is already resolved).
#
# Outputs:
#   --json    Print JSON to stdout (schema: kiana.license-compliance-summary.v1)
#   --md      Print Markdown table to stdout
#   --out PATH  Write JSON to file
#   --md-out PATH  Write Markdown to file
#
# Usage:
#   bash scripts/generate-license-summary.sh --json
#   bash scripts/generate-license-summary.sh --md
#   bash scripts/generate-license-summary.sh --json --out dist/license-summary.json
set -euo pipefail

cd "$(dirname "$0")/.."

format=""
json_out=""
md_out=""

while (($# > 0)); do
  case "$1" in
    --json) format="json" ;;
    --md)   format="md" ;;
    --out)
      shift; json_out="$1"
      [[ -z "$format" ]] && format="json"
      ;;
    --md-out)
      shift; md_out="$1"
      [[ -z "$format" ]] && format="md"
      ;;
    -h|--help)
      echo "usage: scripts/generate-license-summary.sh [--json|--md] [--out PATH] [--md-out PATH]"
      exit 0 ;;
    *)
      echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done

if [[ -z "$format" ]]; then
  echo "generate-license-summary: specify --json or --md" >&2
  exit 2
fi

python_bin() {
  command -v python3 2>/dev/null || command -v python 2>/dev/null || {
    echo "generate-license-summary requires python3" >&2; exit 1
  }
}

KIANA_LICENSE_FORMAT="$format" \
KIANA_LICENSE_JSON_OUT="$json_out" \
KIANA_LICENSE_MD_OUT="$md_out" \
"$(python_bin)" - <<'PY'
import json, os, subprocess, sys
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path.cwd()

def run_cargo_metadata():
    result = subprocess.run(
        ["cargo", "metadata", "--locked", "--offline", "--format-version", "1"],
        cwd=ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False,
    )
    if result.returncode != 0:
        # Try without --offline as fallback
        result = subprocess.run(
            ["cargo", "metadata", "--locked", "--format-version", "1"],
            cwd=ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False,
        )
    if result.returncode != 0:
        raise SystemExit(f"cargo metadata failed: {result.stderr.strip()[:300]}")
    return json.loads(result.stdout)

metadata = run_cargo_metadata()
workspace_members = set(metadata.get("workspace_members", []))

rows = []
for pkg in sorted(metadata.get("packages", []), key=lambda p: (p["name"].lower(), p["version"])):
    source_type = "workspace" if pkg["id"] in workspace_members else "registry"
    license_str = pkg.get("license") or ""
    if not license_str and pkg.get("license_file"):
        license_str = f"(see {pkg['license_file']})"
    if not license_str:
        license_str = "UNKNOWN"
    rows.append({
        "name": pkg["name"],
        "version": pkg["version"],
        "license": license_str,
        "source": source_type,
        "repository": pkg.get("repository") or "",
    })

summary = {
    "schema": "kiana.license-compliance-summary.v1",
    "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "total_packages": len(rows),
    "workspace_packages": sum(1 for r in rows if r["source"] == "workspace"),
    "third_party_packages": sum(1 for r in rows if r["source"] == "registry"),
    "unknown_license_count": sum(1 for r in rows if r["license"] == "UNKNOWN"),
    "packages": rows,
}

fmt = os.environ.get("KIANA_LICENSE_FORMAT", "json")
json_out = os.environ.get("KIANA_LICENSE_JSON_OUT", "")
md_out = os.environ.get("KIANA_LICENSE_MD_OUT", "")

json_text = json.dumps(summary, indent=2, sort_keys=True) + "\n"

def render_md(summary):
    lines = [
        "# Kiana Dependency License Summary",
        "",
        f"Generated: {summary['generated_at']}",
        f"Total packages: {summary['total_packages']} "
        f"(workspace: {summary['workspace_packages']}, "
        f"third-party: {summary['third_party_packages']}, "
        f"unknown license: {summary['unknown_license_count']})",
        "",
        "| Package | Version | License | Source |",
        "| --- | --- | --- | --- |",
    ]
    for row in summary["packages"]:
        lines.append(f"| {row['name']} | {row['version']} | {row['license']} | {row['source']} |")
    return "\n".join(lines) + "\n"

md_text = render_md(summary)

if json_out:
    Path(json_out).parent.mkdir(parents=True, exist_ok=True)
    Path(json_out).write_text(json_text, encoding="utf-8")

if md_out:
    Path(md_out).parent.mkdir(parents=True, exist_ok=True)
    Path(md_out).write_text(md_text, encoding="utf-8")

if fmt == "json":
    print(json_text, end="")
elif fmt == "md":
    print(md_text, end="")
PY
