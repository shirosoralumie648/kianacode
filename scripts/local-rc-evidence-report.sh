#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

version="${VERSION:-$(tr -d '\r\n' < VERSION)}"
dist_dir="${DIST_DIR:-dist}"
out="${KIANA_LOCAL_RC_EVIDENCE_OUT:-${dist_dir}/proofs/local-rc-evidence.json}"
lifecycle_status="${KIANA_LOCAL_RC_LIFECYCLE_SMOKE_STATUS:-not_run}"

python_bin() {
  command -v python3 2>/dev/null || command -v python 2>/dev/null || {
    echo "local RC evidence report requires python3 or python" >&2
    exit 1
  }
}

mkdir -p "$(dirname "$out")"
tmp_blockers="$(mktemp)"
trap 'rm -f "$tmp_blockers"' EXIT
blocker_handoff_dir="${dist_dir}/proofs/local-rc/blockers"
blocker_report_out="${blocker_handoff_dir}/commercial-release-blockers.json"
blocker_handoff_out="${blocker_handoff_dir}/commercial-release-handoff.md"
DIST_DIR="$dist_dir" \
  KIANA_DISTRIBUTION_REVIEW_OUT="${KIANA_DISTRIBUTION_REVIEW_OUT:-${dist_dir}/proofs/local-rc/distribution/distribution-review.json}" \
  bash scripts/distribution-review-report.sh >/dev/null
DIST_DIR="$dist_dir" \
KIANA_COMMERCIAL_BLOCKERS_OUT="$blocker_report_out" \
  bash scripts/commercial-release-blockers-report.sh \
    --json \
    --handoff-md "$blocker_handoff_out" > "$tmp_blockers"

DIST_DIR="$dist_dir" \
VERSION_VALUE="$version" \
LOCAL_RC_EVIDENCE_OUT="$out" \
LOCAL_RC_LIFECYCLE_STATUS="$lifecycle_status" \
BLOCKERS_JSON="$tmp_blockers" \
BLOCKER_REPORT_OUT="$blocker_report_out" \
BLOCKER_HANDOFF_OUT="$blocker_handoff_out" \
"$(python_bin)" - <<'PY'
import hashlib
import json
import os
import shutil
import sys
from datetime import datetime, timezone
from pathlib import Path

version = os.environ["VERSION_VALUE"]
dist_dir = Path(os.environ["DIST_DIR"])
out = Path(os.environ["LOCAL_RC_EVIDENCE_OUT"])
lifecycle_status = os.environ.get("LOCAL_RC_LIFECYCLE_STATUS", "not_run")
if lifecycle_status not in {"passed", "not_run"}:
    raise SystemExit("KIANA_LOCAL_RC_LIFECYCLE_SMOKE_STATUS must be passed or not_run")

def first_sha(path: Path) -> str:
    if not path.is_file():
        return ""
    return path.read_text(encoding="utf-8").split()[0]

def read_json(path: Path) -> dict:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return {}
    return data if isinstance(data, dict) else {}

def sha256_file(path: Path) -> str:
    if not path.is_file():
        raise SystemExit(f"handoff artifact is missing: {path}")
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()

def stage_local_rc_proof_drafts() -> None:
    def current_platform() -> str:
        if sys.platform.startswith("win"):
            return "windows"
        if sys.platform == "darwin":
            return "macos"
        if sys.platform.startswith("linux"):
            return "linux"
        return "unknown"

    local_rc_statuses = {"headless_smoke_only", "headless_smoke_partial", "local_rc_only"}
    platform = current_platform()
    fixed_candidates = [
        (
            Path(os.environ.get("KIANA_SOURCE_CONTROL_PROOF_OUT", "target/source-control/source-control.json")),
            dist_dir / "proofs/local-rc/source-control/source-control.json",
        ),
        (
            Path(os.environ.get("KIANA_PRODUCT_ACCEPTANCE_OUT", "target/product-acceptance/product-acceptance.json")),
            dist_dir / "proofs/local-rc/product/product-acceptance.json",
        ),
        (
            Path(os.environ.get("KIANA_ENTITLEMENT_PROOF_OUT", "target/entitlement-proof/entitlement-proof.json")),
            dist_dir / "proofs/local-rc/entitlement/entitlement-proof.json",
        ),
        (
            Path(os.environ.get("KIANA_RELEASE_OPS_OUT", "target/release-ops/release-ops.json")),
            dist_dir / "proofs/local-rc/release-ops/release-ops.json",
        ),
    ]
    platform_candidates = []
    if platform != "unknown":
        platform_path = Path(
            os.environ.get(
                "KIANA_PLATFORM_SECURITY_PROOF_OUT",
                f"target/platform-security/platform-security-{platform}.json",
            )
        )
        platform_candidates.append(
            (platform_path, dist_dir / "proofs/local-rc/platform-security" / platform_path.name)
        )
    managed_destinations = [dest for _, dest in fixed_candidates]
    managed_destinations.extend(
        sorted((dist_dir / "proofs/local-rc/platform-security").glob("platform-security-*.json"))
    )
    for dest in managed_destinations:
        if dest.is_file():
            dest.unlink()
    for source, dest in fixed_candidates + platform_candidates:
        if not source.is_file():
            continue
        data = read_json(source)
        if data.get("version") != version:
            continue
        if data.get("accepted") is True or data.get("status") not in local_rc_statuses:
            continue
        dest.parent.mkdir(parents=True, exist_ok=True)
        if source.resolve() != dest.resolve():
            shutil.copy2(source, dest)

release_artifacts = []
for archive in sorted(dist_dir.glob(f"kiana-{version}-*.tar.gz")):
    package = archive.name[:-7]
    target = package.removeprefix(f"kiana-{version}-")
    release_artifacts.append({
        "target": target,
        "archive": str(archive),
        "archive_sha256": first_sha(Path(str(archive) + ".sha256")),
        "binary_sha256": first_sha(dist_dir / f"{package}.binary.sha256"),
        "lifecycle_smoke": lifecycle_status,
    })

manifest_dir = dist_dir / "manifests"
enterprise = manifest_dir / "enterprise" / "offline-manifest.json"
homebrew_formulae = sorted(str(path) for path in (manifest_dir / "homebrew").glob("*.rb"))
winget_manifests = sorted(str(path) for path in (manifest_dir / "winget").glob(f"*/{version}/*.yaml"))
blocked_channels = []
for blocked in sorted(manifest_dir.glob("*/*BLOCKED.md")):
    blocked_channels.append(str(blocked))

stage_local_rc_proof_drafts()
proofs = []
for proof in sorted((dist_dir / "proofs").glob("**/*.json")):
    if proof == out:
        continue
    data = read_json(proof)
    proofs.append({
        "path": str(proof),
        "schema": str(data.get("schema", "")),
        "status": str(data.get("status", "")),
        "accepted": bool(data.get("accepted", False)),
    })

with open(os.environ["BLOCKERS_JSON"], "r", encoding="utf-8") as handle:
    blockers_report = json.load(handle)
summary = blockers_report.get("summary") or {}
checks = blockers_report.get("checks") or []
blocking_ids = [check.get("id", "") for check in checks if check.get("status") == "blocking"]
local_blocking_ids = [
    check.get("id", "")
    for check in checks
    if check.get("status") == "blocking" and check.get("owner_status") == "local-owner"
]
external_blocking_ids = [
    check.get("id", "")
    for check in checks
    if check.get("status") == "blocking" and bool(check.get("external"))
]
resolution_scopes = [
    "local-automation",
    "release-owner",
    "release-security",
    "release-environment",
    "final-artifact-derived",
    "live-service",
    "acceptance-owner",
]
summary_scope_counts = summary.get("blocking_by_resolution_scope") or {}
blocking_ids_by_resolution_scope = {
    scope: [
        check.get("id", "")
        for check in checks
        if check.get("status") == "blocking" and check.get("resolution_scope") == scope
    ]
    for scope in resolution_scopes
}
blocking_by_resolution_scope = {
    scope: int(summary_scope_counts.get(scope, len(blocking_ids_by_resolution_scope[scope])) or 0)
    for scope in resolution_scopes
}
blocker_report_path = Path(os.environ["BLOCKER_REPORT_OUT"])
blocker_handoff_path = Path(os.environ["BLOCKER_HANDOFF_OUT"])
handoff_artifacts = [
    {
        "kind": "blockers_json",
        "path": str(blocker_report_path),
        "sha256": sha256_file(blocker_report_path),
    },
    {
        "kind": "handoff_markdown",
        "path": str(blocker_handoff_path),
        "sha256": sha256_file(blocker_handoff_path),
    },
]

local_blockers = int(summary.get("local_blocking", 0) or 0)
required_proof_schemas = [
    "kiana.source-control-proof.v1",
    "kiana.product-acceptance.v1",
    "kiana.entitlement-proof.v1",
    "kiana.release-ops.v1",
    "kiana.platform-security-proof.v1",
]
required_proofs = []
missing_required_proofs = []
for schema in required_proof_schemas:
    paths = sorted(proof["path"] for proof in proofs if proof.get("schema") == schema)
    present = bool(paths)
    if not present:
        missing_required_proofs.append(schema)
    required_proofs.append({
        "schema": schema,
        "present": present,
        "paths": paths,
    })

release_artifacts_present = bool(release_artifacts)
lifecycle_smoke_passed = (
    release_artifacts_present
    and all(artifact.get("lifecycle_smoke") == "passed" for artifact in release_artifacts)
)
required_proofs_present = not missing_required_proofs
local_blockers_clear = local_blockers == 0
readiness_issues = []
if not release_artifacts_present:
    readiness_issues.append("missing release artifacts")
if not lifecycle_smoke_passed:
    readiness_issues.append("package lifecycle smoke has not passed for every release artifact")
if not required_proofs_present:
    readiness_issues.append(
        "missing required local RC proof schemas: " + ", ".join(missing_required_proofs)
    )
if not local_blockers_clear:
    readiness_issues.append("local commercial blockers remain: " + ", ".join(local_blocking_ids))
readiness_ready = (
    release_artifacts_present
    and lifecycle_smoke_passed
    and required_proofs_present
    and local_blockers_clear
)
status = "local_rc_ready" if readiness_ready else "local_rc_incomplete"
report = {
    "schema": "kiana.local-rc-evidence.v1",
    "version": version,
    "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "status": status,
    "dist_dir": str(dist_dir),
    "summary": {
        "release_artifacts": len(release_artifacts),
        "manifests": int(enterprise.is_file()) + len(homebrew_formulae) + len(winget_manifests),
        "proofs": len(proofs),
        "blockers_total": int(summary.get("blocking", 0) or 0),
        "local_blockers": local_blockers,
        "external_blockers": int(summary.get("external_blocking", 0) or 0),
    },
    "release_artifacts": release_artifacts,
    "distribution_manifests": {
        "enterprise_offline_manifest": str(enterprise) if enterprise.is_file() else "",
        "homebrew_formulae": homebrew_formulae,
        "winget_manifests": winget_manifests,
        "blocked_channels": blocked_channels,
    },
    "readiness": {
        "ready": readiness_ready,
        "release_artifacts_present": release_artifacts_present,
        "lifecycle_smoke_passed": lifecycle_smoke_passed,
        "required_proofs_present": required_proofs_present,
        "local_blockers_clear": local_blockers_clear,
        "required_proof_schemas": required_proofs,
        "issues": readiness_issues,
    },
    "proofs": proofs,
    "blockers": {
        "status": str(blockers_report.get("status", "blocked")),
        "blocking": int(summary.get("blocking", 0) or 0),
        "local_blocking": local_blockers,
        "external_blocking": int(summary.get("external_blocking", 0) or 0),
        "blocking_ids": blocking_ids,
        "external_blocking_ids": external_blocking_ids,
        "blocking_by_resolution_scope": blocking_by_resolution_scope,
        "blocking_ids_by_resolution_scope": blocking_ids_by_resolution_scope,
        "handoff_status": "external_action_required" if external_blocking_ids else "not_required",
        "handoff_artifacts": handoff_artifacts,
    },
}

out.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"local RC evidence report written to {out}")
PY
