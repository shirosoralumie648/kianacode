#!/usr/bin/env bash
# scripts/sign-sbom.sh
# Phase 2 Wave 3 — SBOM signing + user-facing redacted export (D-11/D-12)
#
# Produces:
#   dist/sbom.cdx.json.sig   — opaque signature file (via KIANA_SIGNING_COMMAND)
#   dist/sbom.cdx.user.json  — bom-ref-redacted CycloneDX SBOM for public distribution
#
# Prerequisites:
#   - dist/sbom.cdx.json must exist (run scripts/generate-sbom.sh first)
#   - KIANA_SIGNING_COMMAND must be set to a command that signs stdin and writes
#     the signature to the path given as its last argument (same convention as
#     scripts/sign-release-artifacts.sh)
#
# Usage:
#   bash scripts/sign-sbom.sh
#   DIST_DIR=path/to/dist bash scripts/sign-sbom.sh
set -euo pipefail

cd "$(dirname "$0")/.."

DIST_DIR="${DIST_DIR:-dist}"

sbom_path="${DIST_DIR}/sbom.cdx.json"
sbom_sig="${DIST_DIR}/sbom.cdx.json.sig"
sbom_user="${DIST_DIR}/sbom.cdx.user.json"

if [[ ! -f "$sbom_path" ]]; then
  echo "sign-sbom: $sbom_path not found — run scripts/generate-sbom.sh first" >&2
  exit 1
fi

# ── Produce user-facing redacted SBOM ────────────────────────────────────────
python3 - "$sbom_path" "$sbom_user" <<'PY'
import json, pathlib, sys

src = pathlib.Path(sys.argv[1])
dst = pathlib.Path(sys.argv[2])

sbom = json.loads(src.read_text(encoding="utf-8"))

def redact_bom_ref(component):
    bom_ref = component.get("bom-ref", "")
    if "path+file://" in bom_ref or "path+file%3A" in bom_ref:
        component["bom-ref"] = f"{component.get('name', 'unknown')}@{component.get('version', '')}"
    refs = component.get("externalReferences", [])
    component["externalReferences"] = [
        r for r in refs
        if not r.get("url", "").startswith("file://")
    ] if refs else refs
    return component

# Redact workspace-member bom-refs and local externalReferences
components = sbom.get("components", [])
sbom["components"] = [redact_bom_ref(c) for c in components]

# Redact root metadata component bom-ref if it contains workspace path
meta_component = sbom.get("metadata", {}).get("component", {})
if meta_component and "path+file://" in meta_component.get("bom-ref", ""):
    meta_component["bom-ref"] = "kiana"

dst.parent.mkdir(parents=True, exist_ok=True)
dst.write_text(json.dumps(sbom, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"sign-sbom: user SBOM written to {dst} ({dst.stat().st_size} bytes)")
PY

# ── Sign the SBOM (if KIANA_SIGNING_COMMAND is set) ──────────────────────────
signing_command="${KIANA_SIGNING_COMMAND:-}"
if [[ -z "$signing_command" ]]; then
  echo "sign-sbom: KIANA_SIGNING_COMMAND not set — SBOM signing skipped (sbom.signed blocker will remain blocking)" >&2
  echo "sign-sbom: user SBOM at $sbom_user is ready for distribution"
  exit 0
fi

# Sign: invoke KIANA_SIGNING_COMMAND with the SBOM file and sig output path
eval "$signing_command" "$sbom_path" "$sbom_sig"
echo "sign-sbom: SBOM signed — signature at $sbom_sig"
echo "sign-sbom: user SBOM at $sbom_user"
