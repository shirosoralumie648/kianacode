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
bash scripts/commercial-release-blockers-report.sh --json > "$tmp_blockers"

DIST_DIR="$dist_dir" \
VERSION_VALUE="$version" \
LOCAL_RC_EVIDENCE_OUT="$out" \
LOCAL_RC_LIFECYCLE_STATUS="$lifecycle_status" \
BLOCKERS_JSON="$tmp_blockers" \
"$(python_bin)" - <<'PY'
import json
import os
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

proofs = []
for proof in sorted((dist_dir / "proofs").glob("**/*.json")):
    if proof == out:
        continue
    try:
        data = json.loads(proof.read_text(encoding="utf-8"))
    except Exception:
        data = {}
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

local_blockers = int(summary.get("local_blocking", 0) or 0)
status = "local_rc_ready" if release_artifacts and local_blockers == 0 else "local_rc_incomplete"
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
    "proofs": proofs,
    "blockers": {
        "status": str(blockers_report.get("status", "blocked")),
        "blocking": int(summary.get("blocking", 0) or 0),
        "local_blocking": local_blockers,
        "external_blocking": int(summary.get("external_blocking", 0) or 0),
        "blocking_ids": blocking_ids,
    },
}

out.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"local RC evidence report written to {out}")
PY
