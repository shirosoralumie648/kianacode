#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  cat <<'EOF'
usage: scripts/stage-commercial-release-proofs.sh

Stages accepted commercial release proofs into DIST_DIR/proofs, writes
PROOF-MANIFEST.json and HANDOFF.md, and refuses blocked templates, local-RC
proofs, missing proofs, or placeholder accepted evidence.

Inputs may come from existing dist/proofs files, the standard target proof
outputs, default docs proof paths, or the KIANA_*_FILE / KIANA_*_DIR
environment variables used by the full preflight scripts.

Environment:
  DIST_DIR                                      default: dist
  KIANA_COMMERCIAL_PROOF_MANIFEST_OUT          default: DIST_DIR/proofs/PROOF-MANIFEST.json
  KIANA_COMMERCIAL_PROOF_HANDOFF_OUT           default: DIST_DIR/proofs/HANDOFF.md
  KIANA_SOURCE_CONTROL_PROOF_FILE
  KIANA_SOURCE_CONTROL_PROOF_OUT
  KIANA_LIVE_SMOKE_DIR
  KIANA_PROVIDER_LIVE_SMOKE_DIR
  KIANA_REMOTE_LIVE_SMOKE_DIR
  KIANA_PRODUCT_ACCEPTANCE_FILE
  KIANA_ENTITLEMENT_PROOF_FILE
  KIANA_RELEASE_OPS_FILE
  KIANA_PLATFORM_SECURITY_PROOF_FILE
  KIANA_PLATFORM_SECURITY_PROOF_DIR
EOF
  exit 0
fi

if (($# > 0)); then
  echo "unknown argument: $1" >&2
  exit 2
fi

python_bin() {
  command -v python3 2>/dev/null || command -v python 2>/dev/null || {
    echo "commercial proof staging requires python3 or python" >&2
    exit 1
  }
}

"$(python_bin)" - <<'PY'
from __future__ import annotations

import hashlib
import json
import os
import shutil
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

ROOT = Path.cwd()
VERSION = (ROOT / "VERSION").read_text(encoding="utf-8").strip()
DIST_DIR = Path(os.environ.get("DIST_DIR", "dist"))
PROOF_ROOT = DIST_DIR / "proofs"
MANIFEST_OUT = Path(
    os.environ.get(
        "KIANA_COMMERCIAL_PROOF_MANIFEST_OUT",
        str(PROOF_ROOT / "PROOF-MANIFEST.json"),
    )
)
HANDOFF_OUT = Path(
    os.environ.get(
        "KIANA_COMMERCIAL_PROOF_HANDOFF_OUT",
        str(PROOF_ROOT / "HANDOFF.md"),
    )
)
REQUIRED_ENTITLEMENTS = {
    item.strip()
    for item in os.environ.get(
        "KIANA_REQUIRED_ENTITLEMENTS",
        "commercial-use,enterprise-support,managed-policy",
    ).split(",")
    if item.strip()
}
PLACEHOLDER_MARKERS = (
    "todo",
    "tbd",
    "pending",
    "placeholder",
    "replace-me",
    "example.com",
    "example.test",
)
EXPECTED_ISOLATION = {
    "linux": "linux_bwrap",
    "macos": "macos_exec_policy",
    "windows": "windows_exec_policy",
}

errors: list[str] = []
entries: list[dict[str, Any]] = []


def now() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def to_path(value: str | os.PathLike[str] | None) -> Path | None:
    if value is None:
        return None
    text = str(value).strip()
    if not text:
        return None
    return Path(text)


def unique_paths(paths: list[Path | None]) -> list[Path]:
    seen: set[str] = set()
    result: list[Path] = []
    for path in paths:
        if path is None:
            continue
        key = str(path)
        if key in seen:
            continue
        seen.add(key)
        result.append(path)
    return result


def slash(path: Path) -> str:
    return str(path).replace("\\", "/")


def rel(path: Path) -> str:
    try:
        return slash(path.resolve().relative_to(ROOT.resolve()))
    except Exception:
        return slash(path)


def load_json(path: Path) -> dict[str, Any] | None:
    try:
        with path.open("r", encoding="utf-8") as handle:
            data = json.load(handle)
    except FileNotFoundError:
        return None
    except Exception as exc:  # noqa: BLE001
        errors.append(f"{rel(path)}: failed to read JSON: {exc}")
        return None
    if not isinstance(data, dict):
        errors.append(f"{rel(path)}: proof root must be a JSON object")
        return None
    return data


def first_existing(label: str, paths: list[Path | None]) -> tuple[Path, dict[str, Any]] | None:
    for path in unique_paths(paths):
        if not path.is_file():
            continue
        if "proof-templates" in {part.lower() for part in path.parts}:
            errors.append(f"{label}: refusing proof template source {rel(path)}")
            continue
        data = load_json(path)
        if data is not None:
            return path, data
    errors.append(
        f"{label}: missing accepted proof; checked "
        + ", ".join(rel(path) for path in unique_paths(paths))
    )
    return None


def filled(mapping: dict[str, Any], key: str) -> bool:
    value = mapping.get(key)
    return isinstance(value, str) and bool(value.strip())


def not_placeholder(mapping: dict[str, Any], key: str) -> bool:
    value = mapping.get(key)
    return isinstance(value, str) and not any(
        marker in value.lower() for marker in PLACEHOLDER_MARKERS
    )


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def copy_and_record(
    *,
    id: str,
    category: str,
    source: Path,
    dest: Path,
    data: dict[str, Any],
    details: dict[str, Any] | None = None,
) -> None:
    dest.parent.mkdir(parents=True, exist_ok=True)
    if source.resolve() != dest.resolve():
        shutil.copy2(source, dest)
    entry = {
        "id": id,
        "category": category,
        "schema": data.get("schema"),
        "status": data.get("status", "present"),
        "accepted": data.get("accepted"),
        "live": data.get("live"),
        "source": rel(source),
        "path": rel(dest),
        "sha256": sha256(dest),
    }
    if details:
        entry["details"] = details
    entries.append(entry)


def require_contract(label: str, checks: list[bool], data: dict[str, Any], source: Path) -> bool:
    if all(checks):
        return True
    errors.append(f"{label}: {rel(source)} failed accepted commercial proof contract")
    errors.append(json.dumps(data, indent=2, sort_keys=True))
    return False


source_control = first_existing(
    "source-control proof",
    [
        to_path(os.environ.get("KIANA_SOURCE_CONTROL_PROOF_FILE")),
        to_path(os.environ.get("KIANA_SOURCE_CONTROL_PROOF_OUT")),
        DIST_DIR / "proofs/source-control/source-control.json",
        Path(f"docs/source-control/{VERSION}.json"),
    ],
)
if source_control:
    source, data = source_control
    expected_tag = f"v{VERSION}"
    if require_contract(
        "source-control proof",
        [
            data.get("schema") == "kiana.source-control-proof.v1",
            data.get("version") == VERSION,
            data.get("status") == "accepted",
            data.get("accepted") is True,
            filled(data, "accepted_by"),
            filled(data, "accepted_at"),
            filled(data, "remote_url"),
            filled(data, "commit"),
            filled(data, "tagged_commit"),
            data.get("release_tag") == expected_tag,
            data.get("commit") == data.get("tagged_commit"),
            data.get("pushed") is True,
            data.get("reviewed") is True,
            not_placeholder(data, "accepted_by"),
            not_placeholder(data, "remote_url"),
        ],
        data,
        source,
    ):
        copy_and_record(
            id="source.control",
            category="source-control",
            source=source,
            dest=DIST_DIR / "proofs/source-control/source-control.json",
            data=data,
            details={"release_tag": data.get("release_tag"), "commit": data.get("commit")},
        )


live_root = Path(os.environ.get("KIANA_LIVE_SMOKE_DIR", "target/live-smoke"))
provider_dir = Path(os.environ.get("KIANA_PROVIDER_LIVE_SMOKE_DIR", live_root / "provider"))
remote_dir = Path(os.environ.get("KIANA_REMOTE_LIVE_SMOKE_DIR", live_root / "remote"))

provider_catalog = first_existing(
    "provider live catalog",
    [
        to_path(os.environ.get("KIANA_PROVIDER_CATALOG_PROOF_FILE")),
        DIST_DIR / "proofs/live-smoke/provider/model-catalog-live.json",
        provider_dir / "model-catalog-live.json",
    ],
)
provider_smoke = first_existing(
    "provider live smoke",
    [
        to_path(os.environ.get("KIANA_PROVIDER_SMOKE_PROOF_FILE")),
        DIST_DIR / "proofs/live-smoke/provider/model-smoke-live-tools.json",
        provider_dir / "model-smoke-live-tools.json",
    ],
)
if provider_catalog and provider_smoke:
    catalog_source, catalog = provider_catalog
    smoke_source, smoke = provider_smoke
    catalog_summary = catalog.get("summary")
    if not isinstance(catalog_summary, dict):
        catalog_summary = {}
    providers = catalog.get("providers")
    if not isinstance(providers, list):
        providers = []
    smoke_summary = smoke.get("summary")
    if not isinstance(smoke_summary, dict):
        smoke_summary = {}
    results = smoke.get("results")
    if not isinstance(results, list):
        results = []
    live_text_passed = any(
        isinstance(item, dict)
        and item.get("provider_id") != "fake"
        and item.get("live") is True
        and item.get("capability") == "text"
        and item.get("status") == "passed"
        for item in results
    )
    live_tools_passed = any(
        isinstance(item, dict)
        and item.get("provider_id") != "fake"
        and item.get("live") is True
        and item.get("capability") == "tools"
        and item.get("status") == "passed"
        for item in results
    )
    if require_contract(
        "provider live catalog",
        [
            catalog.get("schema") == "kiana.model-catalog.v1",
            catalog.get("live") is True,
            len(providers) > 0,
            catalog_summary.get("failed") == 0,
        ],
        catalog,
        catalog_source,
    ):
        copy_and_record(
            id="live.provider-catalog",
            category="live-service",
            source=catalog_source,
            dest=DIST_DIR / "proofs/live-smoke/provider/model-catalog-live.json",
            data=catalog,
            details={"providers": catalog_summary.get("providers", len(providers))},
        )
    if require_contract(
        "provider live smoke",
        [
            smoke.get("schema") == "kiana.model-smoke.v1",
            smoke.get("live") is True,
            smoke.get("tools") is True,
            smoke_summary.get("failed") == 0,
            live_text_passed,
            live_tools_passed,
        ],
        smoke,
        smoke_source,
    ):
        copy_and_record(
            id="live.provider-smoke",
            category="live-service",
            source=smoke_source,
            dest=DIST_DIR / "proofs/live-smoke/provider/model-smoke-live-tools.json",
            data=smoke,
            details={"text": live_text_passed, "tools": live_tools_passed},
        )

remote = first_existing(
    "remote code-session smoke",
    [
        to_path(os.environ.get("KIANA_REMOTE_SMOKE_PROOF_FILE")),
        DIST_DIR / "proofs/live-smoke/remote/code-session-smoke.json",
        remote_dir / "code-session-smoke.json",
    ],
)
if remote:
    source, data = remote
    if require_contract(
        "remote code-session smoke",
        [
            data.get("schema") == "kiana.remote-code-session-smoke.v1",
            data.get("status") == "ok",
            isinstance(data.get("session_id"), str) and data["session_id"].startswith("cse_"),
            isinstance(data.get("api_base_url"), str)
            and data["api_base_url"].startswith(("http://", "https://")),
            isinstance(data.get("sdk_url"), str)
            and data["sdk_url"].startswith(("http://", "https://")),
            isinstance(data.get("expires_in"), int) and data["expires_in"] > 0,
            isinstance(data.get("worker_epoch"), int) and data["worker_epoch"] >= 0,
        ],
        data,
        source,
    ):
        copy_and_record(
            id="live.remote-code-session",
            category="live-service",
            source=source,
            dest=DIST_DIR / "proofs/live-smoke/remote/code-session-smoke.json",
            data=data,
            details={"session_id": data.get("session_id")},
        )

product = first_existing(
    "product acceptance",
    [
        to_path(os.environ.get("KIANA_PRODUCT_ACCEPTANCE_FILE")),
        to_path(os.environ.get("KIANA_PRODUCT_ACCEPTANCE_OUT")),
        DIST_DIR / "proofs/product/product-acceptance.json",
        Path("target/product-acceptance/product-acceptance.json"),
        Path(f"docs/product-acceptance/{VERSION}.json"),
    ],
)
if product:
    source, data = product
    workflows = set(data.get("workflows") or [])
    required_workflows = {
        "permission",
        "diff",
        "history",
        "onboarding",
        "resume",
        "settings",
        "app-server",
        "context-search",
    }
    if require_contract(
        "product acceptance",
        [
            data.get("schema") == "kiana.product-acceptance.v1",
            data.get("version") == VERSION,
            data.get("status") == "accepted",
            data.get("accepted") is True,
            filled(data, "accepted_by"),
            filled(data, "accepted_at"),
            filled(data, "scope"),
            required_workflows.issubset(workflows),
        ],
        data,
        source,
    ):
        copy_and_record(
            id="acceptance.product",
            category="acceptance",
            source=source,
            dest=DIST_DIR / "proofs/product/product-acceptance.json",
            data=data,
            details={"workflows": sorted(workflows)},
        )

entitlement = first_existing(
    "entitlement proof",
    [
        to_path(os.environ.get("KIANA_ENTITLEMENT_PROOF_FILE")),
        to_path(os.environ.get("KIANA_ENTITLEMENT_PROOF_OUT")),
        DIST_DIR / "proofs/entitlement/entitlement-proof.json",
        Path("target/entitlement-proof/entitlement-proof.json"),
        Path(f"docs/entitlements/{VERSION}.json"),
    ],
)
if entitlement:
    source, data = entitlement
    backend = data.get("backend")
    if not isinstance(backend, dict):
        backend = {}
    entitlements = set(data.get("entitlements") or [])
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
    if require_contract(
        "entitlement proof",
        [
            data.get("schema") == "kiana.entitlement-proof.v1",
            data.get("version") == VERSION,
            data.get("status") == "accepted",
            data.get("accepted") is True,
            data.get("license_status") == "active",
            REQUIRED_ENTITLEMENTS.issubset(entitlements),
            all(filled(data, key) and not_placeholder(data, key) for key in required_strings),
            all(filled(backend, key) and not_placeholder(backend, key) for key in backend_strings),
        ],
        data,
        source,
    ):
        copy_and_record(
            id="acceptance.entitlement",
            category="acceptance",
            source=source,
            dest=DIST_DIR / "proofs/entitlement/entitlement-proof.json",
            data=data,
            details={"entitlements": sorted(entitlements)},
        )

release_ops = first_existing(
    "release operations proof",
    [
        to_path(os.environ.get("KIANA_RELEASE_OPS_FILE")),
        to_path(os.environ.get("KIANA_RELEASE_OPS_OUT")),
        DIST_DIR / "proofs/release-ops/release-ops.json",
        Path("target/release-ops/release-ops.json"),
        Path(f"docs/release-ops/{VERSION}.json"),
    ],
)
if release_ops:
    source, data = release_ops
    credential_review = data.get("credential_review")
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
    if require_contract(
        "release operations proof",
        [
            data.get("schema") == "kiana.release-ops.v1",
            data.get("version") == VERSION,
            data.get("status") == "accepted",
            data.get("accepted") is True,
            all(filled(data, key) and not_placeholder(data, key) for key in required_strings),
            isinstance(data.get("artifact_retention_days"), int)
            and data["artifact_retention_days"] >= 90,
            isinstance(data.get("log_retention_days"), int)
            and data["log_retention_days"] >= 30,
            credential_review.get("status") == "accepted",
            filled(credential_review, "reviewed_by"),
            filled(credential_review, "reviewed_at"),
            filled(credential_review, "scope"),
        ],
        data,
        source,
    ):
        copy_and_record(
            id="acceptance.release-ops",
            category="acceptance",
            source=source,
            dest=DIST_DIR / "proofs/release-ops/release-ops.json",
            data=data,
            details={
                "artifact_retention_days": data.get("artifact_retention_days"),
                "log_retention_days": data.get("log_retention_days"),
            },
        )

platform_candidates: list[Path | None] = []
platform_candidates.append(to_path(os.environ.get("KIANA_PLATFORM_SECURITY_PROOF_FILE")))
platform_out = to_path(os.environ.get("KIANA_PLATFORM_SECURITY_PROOF_OUT"))
platform_candidates.append(platform_out)
platform_dir = to_path(os.environ.get("KIANA_PLATFORM_SECURITY_PROOF_DIR"))
if platform_dir is not None:
    platform_candidates.extend(sorted(platform_dir.glob(f"{VERSION}-*.json")))
platform_candidates.extend(sorted((DIST_DIR / "proofs/platform-security").glob("*.json")))
platform_candidates.extend(sorted(Path("target/platform-security").glob("*.json")))
platform_candidates.extend(sorted(Path("docs/platform-security").glob(f"{VERSION}-*.json")))

seen_platforms: dict[str, tuple[Path, dict[str, Any]]] = {}
for source in unique_paths(platform_candidates):
    if not source.is_file():
        continue
    if "proof-templates" in {part.lower() for part in source.parts}:
        errors.append(f"platform security proof: refusing proof template source {rel(source)}")
        continue
    data = load_json(source)
    if data is None:
        continue
    platform = data.get("platform")
    if platform not in EXPECTED_ISOLATION:
        errors.append(f"platform security proof: {rel(source)} has unsupported platform {platform!r}")
        continue
    if platform not in seen_platforms:
        seen_platforms[platform] = (source, data)

for platform in ["linux", "macos", "windows"]:
    if platform not in seen_platforms:
        errors.append(f"platform security proof: missing accepted {platform} proof")
        continue
    source, data = seen_platforms[platform]
    controls = data.get("controls")
    evidence = data.get("evidence")
    if require_contract(
        f"platform security proof {platform}",
        [
            data.get("schema") == "kiana.platform-security-proof.v1",
            data.get("version") == VERSION,
            data.get("status") == "accepted",
            data.get("accepted") is True,
            filled(data, "accepted_by"),
            filled(data, "accepted_at"),
            filled(data, "runner"),
            not_placeholder(data, "accepted_by"),
            not_placeholder(data, "runner"),
            data.get("platform") == platform,
            data.get("isolation") == EXPECTED_ISOLATION[platform],
            data.get("doctor_status") == "ready",
            isinstance(controls, list) and len(controls) > 0,
            isinstance(evidence, list) and len(evidence) > 0,
        ],
        data,
        source,
    ):
        copy_and_record(
            id=f"acceptance.platform-security.{platform}",
            category="acceptance",
            source=source,
            dest=DIST_DIR / f"proofs/platform-security/platform-security-{platform}.json",
            data=data,
            details={"platform": platform, "isolation": data.get("isolation")},
        )

if errors:
    print("commercial proof staging failed", file=sys.stderr)
    for error in errors:
        print(f"- {error}", file=sys.stderr)
    sys.exit(1)

proof_paths = {entry["path"] for entry in entries}
required_paths = {
    rel(DIST_DIR / "proofs/source-control/source-control.json"),
    rel(DIST_DIR / "proofs/live-smoke/provider/model-catalog-live.json"),
    rel(DIST_DIR / "proofs/live-smoke/provider/model-smoke-live-tools.json"),
    rel(DIST_DIR / "proofs/live-smoke/remote/code-session-smoke.json"),
    rel(DIST_DIR / "proofs/entitlement/entitlement-proof.json"),
    rel(DIST_DIR / "proofs/product/product-acceptance.json"),
    rel(DIST_DIR / "proofs/release-ops/release-ops.json"),
    rel(DIST_DIR / "proofs/platform-security/platform-security-linux.json"),
    rel(DIST_DIR / "proofs/platform-security/platform-security-macos.json"),
    rel(DIST_DIR / "proofs/platform-security/platform-security-windows.json"),
}
missing_paths = sorted(required_paths - proof_paths)
if missing_paths:
    print("commercial proof staging failed", file=sys.stderr)
    for path in missing_paths:
        print(f"- missing staged proof path: {path}", file=sys.stderr)
    sys.exit(1)

entries.sort(key=lambda item: item["id"])
platforms = sorted(
    entry.get("details", {}).get("platform")
    for entry in entries
    if isinstance(entry.get("details"), dict) and entry["id"].startswith("acceptance.platform-security.")
)
manifest = {
    "schema": "kiana.commercial-proof-manifest.v1",
    "version": VERSION,
    "generated_at": now(),
    "proof_root": rel(PROOF_ROOT),
    "summary": {
        "proofs": len(entries),
        "accepted": sum(1 for entry in entries if entry.get("accepted") is True),
        "live": sum(1 for entry in entries if entry.get("live") is True),
        "platforms": platforms,
    },
    "proofs": entries,
}

MANIFEST_OUT.parent.mkdir(parents=True, exist_ok=True)
with MANIFEST_OUT.open("w", encoding="utf-8") as handle:
    json.dump(manifest, handle, indent=2, sort_keys=True)
    handle.write("\n")

HANDOFF_OUT.parent.mkdir(parents=True, exist_ok=True)
handoff_lines = [
    "# Commercial Proof Handoff",
    "",
    f"Version: {VERSION}",
    f"Generated: {manifest['generated_at']}",
    f"Proof root: {manifest['proof_root']}",
    "",
    "## Staged Proofs",
    "",
]
for entry in entries:
    handoff_lines.append(
        f"- {entry['id']}: {entry['path']} sha256={entry['sha256']}"
    )
handoff_lines.extend(
    [
        "",
        "## Verification",
        "",
        "Run:",
        "",
        "```bash",
        "bash scripts/verify-commercial-release-artifacts.sh",
        "```",
        "",
        "This handoff only records already accepted/live proof files. It does not",
        "create source-control, acceptance, entitlement, operations, platform,",
        "provider, or remote service evidence.",
        "",
    ]
)
HANDOFF_OUT.write_text("\n".join(handoff_lines), encoding="utf-8")

print(
    "commercial proofs staged: "
    f"manifest={rel(MANIFEST_OUT)} handoff={rel(HANDOFF_OUT)} proofs={len(entries)}"
)
PY
