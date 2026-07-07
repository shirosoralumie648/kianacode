#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

version="${VERSION:-$(tr -d '\r\n' < VERSION)}"
dist_dir="${DIST_DIR:-dist}"
out="${KIANA_DISTRIBUTION_REVIEW_OUT:-${dist_dir}/proofs/local-rc/distribution/distribution-review.json}"

python_bin() {
  command -v python3 2>/dev/null || command -v python 2>/dev/null || {
    echo "distribution review report requires python3 or python" >&2
    exit 1
  }
}

mkdir -p "$(dirname "$out")"

VERSION_VALUE="$version" \
DIST_DIR_VALUE="$dist_dir" \
DISTRIBUTION_REVIEW_OUT="$out" \
"$(python_bin)" - <<'PY'
import json
import os
from datetime import datetime, timezone
from pathlib import Path

version = os.environ["VERSION_VALUE"]
dist_dir = Path(os.environ["DIST_DIR_VALUE"])
out = Path(os.environ["DISTRIBUTION_REVIEW_OUT"])
manifest_dir = dist_dir / "manifests"


def relative(path: Path) -> str:
    try:
        return path.relative_to(dist_dir).as_posix()
    except ValueError:
        return path.as_posix()


def package_stem(file_name: str) -> str:
    for suffix in (".tar.gz", ".zip", ".tgz"):
        if file_name.endswith(suffix):
            return file_name[: -len(suffix)]
    return file_name


def target_from_archive(file_name: str) -> str:
    stem = package_stem(file_name)
    prefix = f"kiana-{version}-"
    if stem.startswith(prefix):
        return stem[len(prefix) :]
    if stem.startswith("kiana-") and "-" in stem[len("kiana-") :]:
        return stem[len("kiana-") :].split("-", 1)[1]
    return "unknown"


def artifact_record(path: Path) -> dict:
    file_name = path.name
    stem = package_stem(file_name)
    sha256_file = path.with_name(f"{file_name}.sha256")
    binary_sha256_file = path.with_name(f"{stem}.binary.sha256")
    return {
        "target": target_from_archive(file_name),
        "archive": file_name,
        "path": relative(path),
        "sha256_file": relative(sha256_file) if sha256_file.is_file() else None,
        "binary_sha256_file": relative(binary_sha256_file)
        if binary_sha256_file.is_file()
        else None,
    }


def sorted_files(root: Path, pattern: str, recursive: bool = False) -> list[str]:
    if not root.is_dir():
        return []
    iterator = root.rglob(pattern) if recursive else root.glob(pattern)
    return sorted(relative(path) for path in iterator if path.is_file())


def read_json(path: Path) -> tuple[dict | None, str]:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except Exception as error:
        return None, str(error)
    if not isinstance(data, dict):
        return None, "manifest root is not a JSON object"
    return data, ""


def channel_status(ready: bool, blocked: bool) -> str:
    if blocked:
        return "blocked"
    if ready:
        return "ready"
    return "missing"


archive_patterns = ("*.tar.gz", "*.zip", "*.tgz")
archive_paths: list[Path] = []
if dist_dir.is_dir():
    for pattern in archive_patterns:
        archive_paths.extend(path for path in dist_dir.glob(pattern) if path.is_file())
archive_paths = sorted(set(archive_paths), key=lambda path: path.name)
artifacts = [artifact_record(path) for path in archive_paths]
targets = {artifact["target"] for artifact in artifacts}

homebrew_dir = manifest_dir / "homebrew"
winget_dir = manifest_dir / "winget"
homebrew_formulae = sorted_files(homebrew_dir, "*.rb")
winget_manifests = sorted_files(winget_dir, "*.yaml", recursive=True)
homebrew_blocked = (homebrew_dir / "BLOCKED.md").is_file()
winget_blocked_path = winget_dir / "BLOCKED.md"
winget_blocked = winget_blocked_path.is_file()

enterprise_manifest_path = manifest_dir / "enterprise" / "offline-manifest.json"
enterprise_data, enterprise_error = read_json(enterprise_manifest_path)
if enterprise_data is None:
    enterprise_offline_manifest = {
        "present": enterprise_manifest_path.is_file(),
        "path": relative(enterprise_manifest_path),
        "valid": False,
        "error": enterprise_error or "missing",
    }
else:
    enterprise_offline_manifest = {
        "present": True,
        "path": relative(enterprise_manifest_path),
        "valid": enterprise_data.get("schema") == "kiana.enterprise.offline-manifest.v1",
        "schema": str(enterprise_data.get("schema", "")),
        "artifact_count": len(enterprise_data.get("artifacts", []))
        if isinstance(enterprise_data.get("artifacts"), list)
        else 0,
        "channels": enterprise_data.get("channels")
        if isinstance(enterprise_data.get("channels"), dict)
        else None,
    }
    if not enterprise_offline_manifest["valid"]:
        enterprise_offline_manifest["error"] = "schema is not kiana.enterprise.offline-manifest.v1"

missing_platforms = [
    platform
    for platform in ("linux", "macos", "windows")
    if not any(platform in target for target in targets)
]
blockers = []
if not artifacts:
    blockers.append(
        {
            "id": "distribution.artifacts",
            "blocking": True,
            "message": "no packaged release artifacts were found in the reviewed dist directory",
        }
    )
if missing_platforms:
    blockers.append(
        {
            "id": "distribution.platform-artifacts",
            "blocking": True,
            "message": f"missing platform artifacts: {', '.join(missing_platforms)}",
        }
    )
if homebrew_blocked or not homebrew_formulae:
    blockers.append(
        {
            "id": "distribution.homebrew",
            "blocking": True,
            "message": "Homebrew channel is explicitly blocked"
            if homebrew_blocked
            else "Homebrew formula is not present",
        }
    )
if winget_blocked or not winget_manifests:
    blockers.append(
        {
            "id": "distribution.winget",
            "blocking": True,
            "message": "winget channel is explicitly blocked"
            if winget_blocked
            else "winget manifest is not present",
        }
    )
if not enterprise_offline_manifest["valid"]:
    blockers.append(
        {
            "id": "distribution.enterprise-offline-manifest",
            "blocking": True,
            "message": "enterprise offline manifest is missing or invalid",
        }
    )

homebrew_status = channel_status(bool(homebrew_formulae), homebrew_blocked)
winget_status = channel_status(bool(winget_manifests), winget_blocked)
enterprise_ready = bool(enterprise_offline_manifest["valid"])
channels_ready = sum(
    1 for ready in (homebrew_status == "ready", winget_status == "ready", enterprise_ready) if ready
)
blocking_count = sum(1 for blocker in blockers if blocker["blocking"])

report = {
    "schema": "kiana.app-server.distribution-review.v1",
    "version": version,
    "dist_dir": str(dist_dir),
    "summary": {
        "artifacts": len(artifacts),
        "platforms": {
            "linux": any("linux" in target for target in targets),
            "macos": any("macos" in target for target in targets),
            "windows": any("windows" in target for target in targets),
        },
        "channels_ready": channels_ready,
        "blocking": blocking_count,
    },
    "artifacts": artifacts,
    "channels": {
        "github_releases": {
            "status": "ready" if artifacts else "missing",
            "artifact_count": len(artifacts),
        },
        "homebrew": {
            "status": homebrew_status,
            "formulae": homebrew_formulae,
        },
        "winget": {
            "status": winget_status,
            "manifests": winget_manifests,
            "blocked_path": relative(winget_blocked_path) if winget_blocked else None,
        },
    },
    "enterprise_offline_manifest": enterprise_offline_manifest,
    "blockers": blockers,
}

out.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"distribution review report written to {out}")
PY
