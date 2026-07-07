#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

python_bin() {
  command -v python3 2>/dev/null || command -v python 2>/dev/null || {
    echo "commercial release handoff smoke requires python3 or python" >&2
    exit 1
  }
}

python="$(python_bin)"
tmp_report="$(mktemp)"
tmp_handoff="$(mktemp)"
tmp_source_control="$(mktemp)"
tmp_proof_report="$(mktemp)"
tmp_proof_handoff="$(mktemp)"
tmp_dist="$(mktemp -d)"
tmp_local_rc_dist="$(mktemp -d)"
trap 'rm -f "$tmp_report" "$tmp_handoff" "$tmp_source_control" "$tmp_proof_report" "$tmp_proof_handoff"; rm -rf "$tmp_dist" "$tmp_local_rc_dist"' EXIT

KIANA_BLOCKER_OWNER_SOURCE_REMOTE="release-manager-test" \
  bash scripts/commercial-release-blockers-report.sh \
    --json \
    --handoff-md "$tmp_handoff" > "$tmp_report"

"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-commercial-release-blockers.v1.schema.json \
  "$tmp_report" >/dev/null

"$python" - "$tmp_report" "$tmp_handoff" <<'PY'
import json
import sys
from pathlib import Path

report = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
handoff = Path(sys.argv[2]).read_text(encoding="utf-8")

if report.get("schema") != "kiana.commercial-release-blockers.v1":
    raise SystemExit("commercial handoff report schema mismatch")
if not handoff.startswith("# Kiana Commercial Release Handoff "):
    raise SystemExit("commercial handoff markdown has an unexpected title")
if "## Blocking Assignments" not in handoff:
    raise SystemExit("commercial handoff markdown is missing the assignment section")

checks = report.get("checks", [])
if not checks:
    raise SystemExit("commercial blockers report has no checks")

required_keys = {
    "owner",
    "owner_status",
    "acceptance_artifacts",
    "verification_commands",
    "handoff_notes",
}
for check in checks:
    missing = sorted(required_keys.difference(check))
    if missing:
        raise SystemExit(f"{check.get('id', '<unknown>')} missing handoff keys: {missing}")
    if not check["owner"]:
        raise SystemExit(f"{check['id']} has an empty owner")
    if check["owner_status"] not in {
        "local-owner",
        "role-owner-required",
        "specific-owner-assigned",
    }:
        raise SystemExit(f"{check['id']} has invalid owner_status {check['owner_status']!r}")
    for key in ["acceptance_artifacts", "verification_commands", "handoff_notes"]:
        if not isinstance(check[key], list):
            raise SystemExit(f"{check['id']} {key} is not a list")

by_id = {check["id"]: check for check in checks}
source_remote = by_id.get("source.remote")
if not source_remote:
    raise SystemExit("source.remote check is missing")
if source_remote["owner"] != "release-manager-test":
    raise SystemExit("source.remote owner override was not applied")
if source_remote["owner_status"] != "specific-owner-assigned":
    raise SystemExit("source.remote owner override did not mark a specific owner")
if "source.remote" not in handoff:
    raise SystemExit("source.remote is missing from the handoff markdown")
if "release-manager-test" not in handoff:
    raise SystemExit("owner override is missing from the handoff markdown")

blocking = [check for check in checks if check.get("status") == "blocking"]
if blocking:
    for check in blocking:
        if check["id"] not in handoff:
            raise SystemExit(f"{check['id']} blocking assignment missing from markdown")
        if check["owner"] not in handoff:
            raise SystemExit(f"{check['id']} owner missing from markdown")
else:
    if "No blocking checks were detected." not in handoff:
        raise SystemExit("ready handoff does not state that no blockers were detected")
PY

cat > "$tmp_source_control" <<'JSON'
{
  "schema": "kiana.source-control-proof.v1",
  "version": "0.1.0",
  "status": "accepted",
  "accepted": true,
  "accepted_by": "release manager",
  "accepted_at": "2026-01-01T00:00:00Z",
  "remote_url": "https://github.com/acme/kiana.git",
  "commit": "0123456789abcdef0123456789abcdef01234567",
  "release_tag": "v0.1.0",
  "tagged_commit": "0123456789abcdef0123456789abcdef01234567",
  "pushed": true,
  "reviewed": true
}
JSON

KIANA_SOURCE_CONTROL_PROOF_FILE="$tmp_source_control" \
  bash scripts/commercial-release-blockers-report.sh \
    --json \
    --handoff-md "$tmp_proof_handoff" > "$tmp_proof_report"

"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-commercial-release-blockers.v1.schema.json \
  "$tmp_proof_report" >/dev/null

"$python" - "$tmp_proof_report" "$tmp_proof_handoff" <<'PY'
import json
import sys
from pathlib import Path

report = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
handoff = Path(sys.argv[2]).read_text(encoding="utf-8")
by_id = {check["id"]: check for check in report.get("checks", [])}

for check_id in ["source.remote", "source.version-tag"]:
    check = by_id.get(check_id)
    if not check:
        raise SystemExit(f"{check_id} check is missing")
    if check.get("status") != "satisfied":
        raise SystemExit(f"{check_id} was not satisfied by accepted source-control proof")
    if check_id in handoff:
        raise SystemExit(f"{check_id} should not appear as a blocking handoff assignment")
    if "source-control proof accepted" not in check.get("evidence", ""):
        raise SystemExit(f"{check_id} evidence does not name accepted source-control proof")
PY

"$python" - "$tmp_dist" <<'PY'
import hashlib
import json
import sys
from pathlib import Path

dist = Path(sys.argv[1])
version = "0.1.0"
target = "linux-x86_64"
package = f"kiana-{version}-{target}"
archive_name = f"{package}.tar.gz"
archive = dist / archive_name
binary_sha = dist / f"{package}.binary.sha256"
archive_sig = dist / f"{archive_name}.sig"
binary_sig = dist / f"{package}.binary.sig"
proof = dist / f"{package}.signature.json"

archive.write_text("signed fixture archive\n", encoding="utf-8")
binary_sha.write_text("0" * 64 + f"  {package}/kiana\n", encoding="utf-8")
archive_sig.write_text("fixture archive signature\n", encoding="utf-8")
binary_sig.write_text("fixture binary signature\n", encoding="utf-8")

def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

proof.write_text(
    json.dumps(
        {
            "schema": "kiana.release-signature.v1",
            "target": target,
            "archive": archive_name,
            "archive_sha256": sha256(archive),
            "binary_sha256_file_sha256": sha256(binary_sha),
            "signed_at": "2026-01-01T00:00:00Z",
            "signer": "release engineering",
            "signature_files": {
                "archive": archive_sig.name,
                "binary": binary_sig.name,
            },
            "verification": {
                "method": "KIANA_SIGNATURE_VERIFY_COMMAND",
                "archive": "verified",
                "binary": "verified",
                "verified_at": "2026-01-01T00:00:01Z",
            },
        },
        indent=2,
        sort_keys=True,
    )
    + "\n",
    encoding="utf-8",
)
PY

DIST_DIR="$tmp_dist" \
  bash scripts/commercial-release-blockers-report.sh \
    --json \
    --handoff-md "$tmp_proof_handoff" > "$tmp_proof_report"

"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-commercial-release-blockers.v1.schema.json \
  "$tmp_proof_report" >/dev/null

"$python" - "$tmp_proof_report" "$tmp_proof_handoff" <<'PY'
import json
import sys
from pathlib import Path

report = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
handoff = Path(sys.argv[2]).read_text(encoding="utf-8")
check = {item["id"]: item for item in report.get("checks", [])}.get("signing.release-artifacts")
if not check:
    raise SystemExit("signing.release-artifacts check is missing")
if check.get("status") != "satisfied":
    raise SystemExit("signing.release-artifacts was not satisfied by accepted signature proof")
if "signing.release-artifacts" in handoff:
    raise SystemExit("signing.release-artifacts should not appear as a blocking handoff assignment")
if "release signature proofs accepted" not in check.get("evidence", ""):
    raise SystemExit("signing.release-artifacts evidence does not name accepted signature proofs")
PY

"$python" - "$tmp_dist" <<'PY'
import hashlib
import json
import sys
from pathlib import Path

dist = Path(sys.argv[1])
version = "0.1.0"
package = f"kiana-{version}-linux-x86_64"
archive_name = f"{package}.tar.gz"
archive = dist / archive_name
archive_sha = dist / f"{archive_name}.sha256"
binary_sha = dist / f"{package}.binary.sha256"
manifest = dist / "manifests/enterprise/offline-manifest.json"
manifest.parent.mkdir(parents=True, exist_ok=True)

archive.write_text("enterprise offline fixture archive\n", encoding="utf-8")
archive_digest = hashlib.sha256(archive.read_bytes()).hexdigest()
archive_sha.write_text(f"{archive_digest}  {archive_name}\n", encoding="utf-8")
binary_sha.write_text(f"{archive_digest}  {package}/kiana\n", encoding="utf-8")

manifest.write_text(
    json.dumps(
        {
            "schema": "kiana.enterprise.offline-manifest.v1",
            "version": version,
            "release_base_url": "https://github.com/acme/kiana/releases/download/v0.1.0",
            "artifacts": [
                {
                    "target": "linux-x86_64",
                    "archive": archive_name,
                    "url": f"https://github.com/acme/kiana/releases/download/v0.1.0/{archive_name}",
                    "sha256": "0" * 64,
                    "binary_sha256": archive_digest,
                    "local_path": archive_name,
                    "checksum_path": f"{archive_name}.sha256",
                    "binary_checksum_path": f"{package}.binary.sha256",
                }
            ],
            "channels": {
                "github_releases": "generated_from_release_base_url",
                "homebrew": "generated",
                "winget": "generated",
            },
            "generated_by": "scripts/generate-distribution-manifests.sh",
        },
        indent=2,
        sort_keys=True,
    )
    + "\n",
    encoding="utf-8",
)
PY

DIST_DIR="$tmp_dist" \
  bash scripts/commercial-release-blockers-report.sh \
    --json \
    --handoff-md "$tmp_proof_handoff" > "$tmp_proof_report"

"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-commercial-release-blockers.v1.schema.json \
  "$tmp_proof_report" >/dev/null

"$python" - "$tmp_proof_report" "$tmp_proof_handoff" <<'PY'
import json
import sys
from pathlib import Path

report = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
handoff = Path(sys.argv[2]).read_text(encoding="utf-8")
check = {item["id"]: item for item in report.get("checks", [])}.get("distribution.enterprise-offline-manifest")
if not check:
    raise SystemExit("distribution.enterprise-offline-manifest check is missing")
if check.get("status") != "blocking":
    raise SystemExit("distribution.enterprise-offline-manifest accepted a checksum-mismatched manifest")
if "enterprise offline manifest failed commercial contract" not in check.get("evidence", ""):
    raise SystemExit("distribution.enterprise-offline-manifest evidence does not name the failed contract")
if "distribution.enterprise-offline-manifest" not in handoff:
    raise SystemExit("distribution.enterprise-offline-manifest should remain a blocking handoff assignment")
PY

"$python" - "$tmp_dist" <<'PY'
import hashlib
import sys
from pathlib import Path

dist = Path(sys.argv[1])
version = "0.1.0"
package = f"kiana-{version}-windows-x86_64"
zip_file = dist / f"{package}.zip"
zip_sha = dist / f"{package}.zip.sha256"
manifest = dist / f"manifests/winget/Kiana.Kiana/{version}/Kiana.Kiana.installer.yaml"
homebrew = dist / "manifests/homebrew/kiana-linux-x86_64.rb"
manifest.parent.mkdir(parents=True, exist_ok=True)
homebrew.parent.mkdir(parents=True, exist_ok=True)

zip_file.write_text("winget zip fixture\n", encoding="utf-8")
zip_digest = hashlib.sha256(zip_file.read_bytes()).hexdigest()
zip_sha.write_text(f"{zip_digest}  {zip_file.name}\n", encoding="utf-8")
homebrew.write_text("class Kiana < Formula\nend\n", encoding="utf-8")
manifest.write_text(
    "\n".join(
        [
            "PackageIdentifier: Kiana.Kiana",
            f"PackageVersion: {version}",
            "InstallerType: zip",
            "NestedInstallerType: portable",
            "Installers:",
            "- Architecture: x64",
            f"  InstallerUrl: https://github.com/acme/kiana/releases/download/v{version}/{zip_file.name}",
            f"  InstallerSha256: {'0' * 64}",
            "  NestedInstallerFiles:",
            "  - RelativeFilePath: kiana-0.1.0-windows-x86_64/kiana.exe",
            "    PortableCommandAlias: kiana",
            "ManifestType: installer",
            "ManifestVersion: 1.6.0",
            "",
        ]
    ),
    encoding="utf-8",
)
PY

DIST_DIR="$tmp_dist" \
  bash scripts/commercial-release-blockers-report.sh \
    --json \
    --handoff-md "$tmp_proof_handoff" > "$tmp_proof_report"

"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-commercial-release-blockers.v1.schema.json \
  "$tmp_proof_report" >/dev/null

"$python" - "$tmp_proof_report" "$tmp_proof_handoff" <<'PY'
import json
import sys
from pathlib import Path

report = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
handoff = Path(sys.argv[2]).read_text(encoding="utf-8")
check = {item["id"]: item for item in report.get("checks", [])}.get("distribution.package-channels")
if not check:
    raise SystemExit("distribution.package-channels check is missing")
if check.get("status") != "blocking":
    raise SystemExit("distribution.package-channels accepted a checksum-mismatched winget manifest")
if "winget manifest failed commercial contract" not in check.get("evidence", ""):
    raise SystemExit("distribution.package-channels evidence does not name the winget contract failure")
if "distribution.package-channels" not in handoff:
    raise SystemExit("distribution.package-channels should remain a blocking handoff assignment")
PY

"$python" - "$tmp_dist" <<'PY'
import hashlib
import sys
from pathlib import Path

dist = Path(sys.argv[1])
version = "0.1.0"
linux_package = f"kiana-{version}-linux-x86_64"
linux_archive = dist / f"{linux_package}.tar.gz"
linux_archive_sha = dist / f"{linux_package}.tar.gz.sha256"
homebrew = dist / "manifests/homebrew/kiana-linux-x86_64.rb"
windows_package = f"kiana-{version}-windows-x86_64"
zip_file = dist / f"{windows_package}.zip"
zip_sha = dist / f"{windows_package}.zip.sha256"
manifest = dist / f"manifests/winget/Kiana.Kiana/{version}/Kiana.Kiana.installer.yaml"
homebrew.parent.mkdir(parents=True, exist_ok=True)
manifest.parent.mkdir(parents=True, exist_ok=True)

linux_archive.write_text("homebrew archive fixture\n", encoding="utf-8")
linux_digest = hashlib.sha256(linux_archive.read_bytes()).hexdigest()
linux_archive_sha.write_text(f"{linux_digest}  {linux_archive.name}\n", encoding="utf-8")
zip_file.write_text("winget zip fixture\n", encoding="utf-8")
zip_digest = hashlib.sha256(zip_file.read_bytes()).hexdigest()
zip_sha.write_text(f"{zip_digest}  {zip_file.name}\n", encoding="utf-8")
homebrew.write_text(
    "\n".join(
        [
            "class KianaLinuxX8664 < Formula",
            '  desc "Kiana command line assistant"',
            '  homepage "https://github.com/acme/kiana"',
            f'  url "https://github.com/acme/kiana/releases/download/v{version}/{linux_archive.name}"',
            f'  sha256 "{"0" * 64}"',
            f'  version "{version}"',
            "",
            "  def install",
            "    bin.install \"kiana\"",
            "  end",
            "end",
            "",
        ]
    ),
    encoding="utf-8",
)
manifest.write_text(
    "\n".join(
        [
            "PackageIdentifier: Kiana.Kiana",
            f"PackageVersion: {version}",
            "InstallerType: zip",
            "NestedInstallerType: portable",
            "Installers:",
            "- Architecture: x64",
            f"  InstallerUrl: https://github.com/acme/kiana/releases/download/v{version}/{zip_file.name}",
            f"  InstallerSha256: {zip_digest}",
            "  NestedInstallerFiles:",
            "  - RelativeFilePath: kiana-0.1.0-windows-x86_64/kiana.exe",
            "    PortableCommandAlias: kiana",
            "ManifestType: installer",
            "ManifestVersion: 1.6.0",
            "",
        ]
    ),
    encoding="utf-8",
)
PY

DIST_DIR="$tmp_dist" \
  bash scripts/commercial-release-blockers-report.sh \
    --json \
    --handoff-md "$tmp_proof_handoff" > "$tmp_proof_report"

"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-commercial-release-blockers.v1.schema.json \
  "$tmp_proof_report" >/dev/null

"$python" - "$tmp_proof_report" "$tmp_proof_handoff" <<'PY'
import json
import sys
from pathlib import Path

report = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
handoff = Path(sys.argv[2]).read_text(encoding="utf-8")
check = {item["id"]: item for item in report.get("checks", [])}.get("distribution.package-channels")
if not check:
    raise SystemExit("distribution.package-channels check is missing")
if check.get("status") != "blocking":
    raise SystemExit("distribution.package-channels accepted a checksum-mismatched Homebrew formula")
if "homebrew formula failed commercial contract" not in check.get("evidence", ""):
    raise SystemExit("distribution.package-channels evidence does not name the Homebrew contract failure")
if "distribution.package-channels" not in handoff:
    raise SystemExit("distribution.package-channels should remain a blocking handoff assignment")
PY

mkdir -p "$tmp_dist/proofs/live-smoke/provider" "$tmp_dist/proofs/live-smoke/remote"
cat > "$tmp_dist/proofs/live-smoke/provider/model-catalog-live.json" <<'JSON'
{
  "schema": "kiana.model-catalog.v1",
  "live": true,
  "summary": {
    "providers": 1,
    "discovered_models": 1,
    "skipped": 0,
    "failed": 0
  },
  "providers": [
    {
      "provider_id": "openai-compatible",
      "display_name": "OpenAI-compatible",
      "protocol": "open_ai_chat_completions",
      "models_source": "user_configured",
      "status": "passed",
      "live": true,
      "model_ids": ["gpt-fixture"],
      "discovered_model_ids": ["gpt-fixture"],
      "message": "fixture live catalog passed",
      "base_url": "https://provider.kiana.local/v1"
    }
  ]
}
JSON
cat > "$tmp_dist/proofs/live-smoke/provider/model-smoke-live-tools.json" <<'JSON'
{
  "schema": "kiana.model-smoke.v1",
  "live": true,
  "tools": true,
  "summary": {
    "passed": 2,
    "skipped": 0,
    "failed": 0
  },
  "results": [
    {
      "provider_id": "openai-compatible",
      "model_id": "gpt-fixture",
      "status": "passed",
      "live": true,
      "capability": "text",
      "message": "fixture text smoke passed",
      "output_preview": "ok"
    },
    {
      "provider_id": "openai-compatible",
      "model_id": "gpt-fixture",
      "status": "passed",
      "live": true,
      "capability": "tools",
      "message": "fixture tool smoke passed",
      "output_preview": "tool_call"
    }
  ]
}
JSON
cat > "$tmp_dist/proofs/live-smoke/remote/code-session-smoke.json" <<'JSON'
{
  "schema": "kiana.remote-code-session-smoke.v1",
  "status": "ok",
  "checked_at": "2026-01-01T00:00:00Z",
  "session_id": "cse_fixture",
  "api_base_url": "https://remote.kiana.local/api",
  "sdk_url": "https://remote.kiana.local/sdk",
  "expires_in": 3600,
  "worker_epoch": 7
}
JSON

DIST_DIR="$tmp_dist" \
  bash scripts/commercial-release-blockers-report.sh \
    --json \
    --handoff-md "$tmp_proof_handoff" > "$tmp_proof_report"

"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-commercial-release-blockers.v1.schema.json \
  "$tmp_proof_report" >/dev/null

"$python" - "$tmp_proof_report" "$tmp_proof_handoff" <<'PY'
import json
import sys
from pathlib import Path

report = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
handoff = Path(sys.argv[2]).read_text(encoding="utf-8")
by_id = {item["id"]: item for item in report.get("checks", [])}
expected = {
    "live.provider-smoke": "provider live proofs accepted",
    "live.remote-code-session": "remote code-session proof accepted",
}
for check_id, evidence_text in expected.items():
    check = by_id.get(check_id)
    if not check:
        raise SystemExit(f"{check_id} check is missing")
    if check.get("status") != "satisfied":
        raise SystemExit(f"{check_id} was not satisfied by staged live proof")
    if check_id in handoff:
        raise SystemExit(f"{check_id} should not appear as a blocking handoff assignment")
    if evidence_text not in check.get("evidence", ""):
        raise SystemExit(f"{check_id} evidence does not name accepted staged proof")
PY

mkdir -p "$tmp_dist/proofs/product"
cat > "$tmp_dist/proofs/product/product-acceptance.json" <<'JSON'
{
  "schema": "kiana.product-acceptance.v1",
  "version": "0.1.0",
  "status": "accepted",
  "accepted": true,
  "accepted_by": "target customer acceptance lead",
  "accepted_at": "2026-01-01T00:00:00Z",
  "scope": "terminal product shell, local app-server, and context-search acceptance",
  "workflows": [
    "permission",
    "diff",
    "history",
    "onboarding",
    "resume",
    "settings",
    "app-server",
    "context-search",
    "context-cache-recovery"
  ]
}
JSON

DIST_DIR="$tmp_dist" \
  bash scripts/commercial-release-blockers-report.sh \
    --json \
    --handoff-md "$tmp_proof_handoff" > "$tmp_proof_report"

"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-commercial-release-blockers.v1.schema.json \
  "$tmp_proof_report" >/dev/null

"$python" - "$tmp_proof_report" "$tmp_proof_handoff" <<'PY'
import json
import sys
from pathlib import Path

report = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
handoff = Path(sys.argv[2]).read_text(encoding="utf-8")
check = {item["id"]: item for item in report.get("checks", [])}.get("acceptance.product")
if not check:
    raise SystemExit("acceptance.product check is missing")
if check.get("status") != "satisfied":
    raise SystemExit("acceptance.product was not satisfied by staged product acceptance proof")
if "acceptance.product" in handoff:
    raise SystemExit("acceptance.product should not appear as a blocking handoff assignment")
if "product acceptance proof accepted" not in check.get("evidence", ""):
    raise SystemExit("acceptance.product evidence does not name accepted staged proof")
PY

mkdir -p "$tmp_dist/proofs/entitlement" "$tmp_dist/proofs/release-ops"
cat > "$tmp_dist/proofs/entitlement/entitlement-proof.json" <<'JSON'
{
  "schema": "kiana.entitlement-proof.v1",
  "version": "0.1.0",
  "status": "accepted",
  "accepted": true,
  "accepted_by": "license operations",
  "accepted_at": "2026-01-01T00:00:00Z",
  "account_id": "acct_live_fixture",
  "organization": "Acme Corp",
  "plan": "Enterprise",
  "license_status": "active",
  "entitlements": ["commercial-use", "enterprise-support", "managed-policy"],
  "license_key_fingerprint": "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
  "support_contact": "support@acme.example",
  "backend": {
    "name": "entitlement-service",
    "environment": "production",
    "checked_at": "2026-01-01T00:00:01Z",
    "request_id": "req_entitlement_fixture"
  }
}
JSON
cat > "$tmp_dist/proofs/release-ops/release-ops.json" <<'JSON'
{
  "schema": "kiana.release-ops.v1",
  "version": "0.1.0",
  "status": "accepted",
  "accepted": true,
  "accepted_by": "release engineering",
  "accepted_at": "2026-01-01T00:00:00Z",
  "security_contact": "security@acme.example",
  "vulnerability_report_channel": "https://acme.example/security",
  "release_credentials_owner": "release engineering",
  "support_contact": "support@acme.example",
  "artifact_retention_days": 365,
  "log_retention_days": 90,
  "credential_review": {
    "status": "accepted",
    "reviewed_by": "security lead",
    "reviewed_at": "2026-01-01T00:00:01Z",
    "scope": "release signing and publishing credentials"
  }
}
JSON

DIST_DIR="$tmp_dist" \
  bash scripts/commercial-release-blockers-report.sh \
    --json \
    --handoff-md "$tmp_proof_handoff" > "$tmp_proof_report"

"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-commercial-release-blockers.v1.schema.json \
  "$tmp_proof_report" >/dev/null

"$python" - "$tmp_proof_report" "$tmp_proof_handoff" <<'PY'
import json
import sys
from pathlib import Path

report = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
handoff = Path(sys.argv[2]).read_text(encoding="utf-8")
by_id = {item["id"]: item for item in report.get("checks", [])}
expected = {
    "acceptance.entitlement": "entitlement proof accepted",
    "acceptance.release-ops": "release operations proof accepted",
}
for check_id, evidence_text in expected.items():
    check = by_id.get(check_id)
    if not check:
        raise SystemExit(f"{check_id} check is missing")
    if check.get("status") != "satisfied":
        raise SystemExit(f"{check_id} was not satisfied by staged proof")
    if check_id in handoff:
        raise SystemExit(f"{check_id} should not appear as a blocking handoff assignment")
    if evidence_text not in check.get("evidence", ""):
        raise SystemExit(f"{check_id} evidence does not name accepted staged proof")
PY

mkdir -p "$tmp_dist/proofs/platform-security"
cat > "$tmp_dist/proofs/platform-security/platform-security-linux.json" <<'JSON'
{
  "schema": "kiana.platform-security-proof.v1",
  "version": "0.1.0",
  "status": "accepted",
  "accepted": true,
  "accepted_by": "platform security lead",
  "accepted_at": "2026-01-01T00:00:00Z",
  "platform": "linux",
  "runner": "linux-release-runner",
  "isolation": "linux_bwrap",
  "controls": ["permission_profile:commercial", "permission_mode:ask", "bwrap:strict"],
  "doctor_status": "ready",
  "evidence": [{"label": "doctor", "value": "commercial_security ready"}]
}
JSON
cat > "$tmp_dist/proofs/platform-security/platform-security-macos.json" <<'JSON'
{
  "schema": "kiana.platform-security-proof.v1",
  "version": "0.1.0",
  "status": "accepted",
  "accepted": true,
  "accepted_by": "platform security lead",
  "accepted_at": "2026-01-01T00:00:00Z",
  "platform": "macos",
  "runner": "macos-release-runner",
  "isolation": "macos_exec_policy",
  "controls": ["permission_profile:commercial", "permission_mode:ask", "exec_policy"],
  "doctor_status": "ready",
  "evidence": [{"label": "doctor", "value": "commercial_security ready"}]
}
JSON
cat > "$tmp_dist/proofs/platform-security/platform-security-windows.json" <<'JSON'
{
  "schema": "kiana.platform-security-proof.v1",
  "version": "0.1.0",
  "status": "accepted",
  "accepted": true,
  "accepted_by": "platform security lead",
  "accepted_at": "2026-01-01T00:00:00Z",
  "platform": "windows",
  "runner": "windows-release-runner",
  "isolation": "windows_exec_policy",
  "controls": ["permission_profile:commercial", "permission_mode:ask", "exec_policy"],
  "doctor_status": "ready",
  "evidence": [{"label": "doctor", "value": "commercial_security ready"}]
}
JSON

DIST_DIR="$tmp_dist" \
  bash scripts/commercial-release-blockers-report.sh \
    --json \
    --handoff-md "$tmp_proof_handoff" > "$tmp_proof_report"

"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-commercial-release-blockers.v1.schema.json \
  "$tmp_proof_report" >/dev/null

"$python" - "$tmp_proof_report" "$tmp_proof_handoff" <<'PY'
import json
import sys
from pathlib import Path

report = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
handoff = Path(sys.argv[2]).read_text(encoding="utf-8")
check = {item["id"]: item for item in report.get("checks", [])}.get("acceptance.platform-security")
if not check:
    raise SystemExit("acceptance.platform-security check is missing")
if check.get("status") != "satisfied":
    raise SystemExit("acceptance.platform-security was not satisfied by staged platform security proofs")
if "acceptance.platform-security" in handoff:
    raise SystemExit("acceptance.platform-security should not appear as a blocking handoff assignment")
if "platform security proofs accepted" not in check.get("evidence", ""):
    raise SystemExit("acceptance.platform-security evidence does not name accepted staged proofs")
PY

mkdir -p "$tmp_local_rc_dist/source-proofs"
cat > "$tmp_local_rc_dist/source-proofs/product-acceptance.json" <<'JSON'
{
  "schema": "kiana.product-acceptance.v1",
  "version": "0.1.0",
  "status": "headless_smoke_only",
  "accepted": false,
  "accepted_by": "",
  "accepted_at": "2026-01-01T00:00:00Z",
  "scope": "headless smoke only",
  "workflows": ["permission", "diff", "history", "onboarding", "resume", "settings", "app-server", "context-search", "context-cache-recovery"]
}
JSON
cat > "$tmp_local_rc_dist/source-proofs/entitlement-proof.json" <<'JSON'
{
  "schema": "kiana.entitlement-proof.v1",
  "version": "0.1.0",
  "status": "local_rc_only",
  "accepted": false,
  "accepted_by": "",
  "accepted_at": "2026-01-01T00:00:00Z",
  "account_id": "",
  "organization": "",
  "plan": "",
  "license_status": "missing",
  "entitlements": ["commercial-use", "enterprise-support", "managed-policy"],
  "license_key_fingerprint": "",
  "support_contact": "",
  "backend": {"name": "", "environment": "", "checked_at": "2026-01-01T00:00:00Z", "request_id": ""}
}
JSON
cat > "$tmp_local_rc_dist/source-proofs/release-ops.json" <<'JSON'
{
  "schema": "kiana.release-ops.v1",
  "version": "0.1.0",
  "status": "local_rc_only",
  "accepted": false,
  "accepted_by": "",
  "accepted_at": "2026-01-01T00:00:00Z",
  "security_contact": "",
  "vulnerability_report_channel": "",
  "release_credentials_owner": "",
  "support_contact": "",
  "artifact_retention_days": 90,
  "log_retention_days": 30,
  "credential_review": {"status": "pending", "reviewed_by": "", "reviewed_at": "", "scope": "local RC only"}
}
JSON
cat > "$tmp_local_rc_dist/source-proofs/platform-security-windows.json" <<'JSON'
{
  "schema": "kiana.platform-security-proof.v1",
  "version": "0.1.0",
  "status": "local_rc_only",
  "accepted": false,
  "accepted_by": "",
  "accepted_at": "2026-01-01T00:00:00Z",
  "platform": "windows",
  "runner": "windows-local-rc",
  "isolation": "windows_exec_policy",
  "controls": ["permission_profile:commercial", "permission_mode:ask"],
  "doctor_status": "unknown",
  "evidence": [{"label": "mode", "value": "local_rc_only"}]
}
JSON

DIST_DIR="$tmp_local_rc_dist" \
KIANA_PRODUCT_ACCEPTANCE_OUT="$tmp_local_rc_dist/source-proofs/product-acceptance.json" \
KIANA_ENTITLEMENT_PROOF_OUT="$tmp_local_rc_dist/source-proofs/entitlement-proof.json" \
KIANA_RELEASE_OPS_OUT="$tmp_local_rc_dist/source-proofs/release-ops.json" \
KIANA_PLATFORM_SECURITY_PROOF_OUT="$tmp_local_rc_dist/source-proofs/platform-security-windows.json" \
KIANA_LOCAL_RC_LIFECYCLE_SMOKE_STATUS=passed \
  bash scripts/local-rc-evidence-report.sh >/dev/null

"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-local-rc-evidence.v1.schema.json \
  "$tmp_local_rc_dist/proofs/local-rc-evidence.json" >/dev/null

"$python" - "$tmp_local_rc_dist/proofs/local-rc-evidence.json" <<'PY'
import json
import sys
from pathlib import Path

report = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
proofs = report.get("proofs", [])
by_schema = {proof.get("schema"): proof for proof in proofs}
expected = {
    "kiana.product-acceptance.v1": "headless_smoke_only",
    "kiana.entitlement-proof.v1": "local_rc_only",
    "kiana.release-ops.v1": "local_rc_only",
    "kiana.platform-security-proof.v1": "local_rc_only",
}
if report.get("summary", {}).get("proofs") != len(expected):
    raise SystemExit("local RC evidence did not stage the expected proof draft count")
for schema, status in expected.items():
    proof = by_schema.get(schema)
    if not proof:
        raise SystemExit(f"local RC evidence missing staged proof for {schema}")
    if proof.get("status") != status or proof.get("accepted") is not False:
        raise SystemExit(f"local RC evidence staged proof has wrong status: {proof}")
    if "proofs" not in proof.get("path", "") or "local-rc" not in proof.get("path", ""):
        raise SystemExit(f"local RC proof was not staged under proofs/local-rc: {proof}")
PY

echo "commercial release handoff smoke passed"
