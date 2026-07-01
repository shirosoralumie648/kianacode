#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

format="text"
fail_on_blockers=0

while (($# > 0)); do
  case "$1" in
    --json)
      format="json"
      ;;
    --fail-on-blockers)
      fail_on_blockers=1
      ;;
    -h|--help)
      cat <<'EOF'
usage: scripts/commercial-release-blockers-report.sh [--json] [--fail-on-blockers]

Writes a lightweight commercial release readiness report without running cargo,
network, signing, or package-manager publication gates.

Set KIANA_COMMERCIAL_BLOCKERS_OUT to also write the JSON report to a file.
EOF
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      exit 2
      ;;
  esac
  shift
done

python_bin() {
  command -v python3 2>/dev/null || command -v python 2>/dev/null || {
    echo "commercial release blockers report requires python3 or python" >&2
    exit 1
  }
}

KIANA_BLOCKERS_FORMAT="$format" \
KIANA_BLOCKERS_FAIL_ON_BLOCKERS="$fail_on_blockers" \
"$(python_bin)" - <<'PY'
import json
import os
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path


ROOT = Path.cwd()
VERSION = (ROOT / "VERSION").read_text(encoding="utf-8").strip()
EXPECTED_TAG = f"v{VERSION}"
PLACEHOLDER_MARKERS = (
    "todo",
    "tbd",
    "pending",
    "placeholder",
    "replace-me",
    "example.com",
    "example.test",
)


def run(command):
    return subprocess.run(
        command,
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )


def git_stdout(*args):
    result = run(["git", *args])
    if result.returncode != 0:
        return ""
    return result.stdout.strip()


def platform_id():
    result = run(["uname", "-s"])
    name = result.stdout.strip() if result.returncode == 0 else sys.platform
    if name.startswith("Linux"):
        return "linux"
    if name.startswith("Darwin"):
        return "macos"
    if name.startswith(("MSYS", "MINGW", "CYGWIN")) or sys.platform.startswith("win"):
        return "windows"
    return "unknown"


def load_json(path):
    try:
        with Path(path).open("r", encoding="utf-8") as handle:
            return json.load(handle), None
    except FileNotFoundError:
        return None, "missing"
    except json.JSONDecodeError as exc:
        return None, f"invalid json: {exc}"


def filled(mapping, key):
    value = mapping.get(key)
    return isinstance(value, str) and bool(value.strip())


def not_placeholder(mapping, key):
    value = mapping.get(key)
    return isinstance(value, str) and not any(
        marker in value.lower() for marker in PLACEHOLDER_MARKERS
    )


def check_status(ok):
    return "satisfied" if ok else "blocking"


checks = []


def add_check(
    *,
    id,
    category,
    title,
    ok,
    external,
    gate,
    evidence,
    required_action,
    paths=None,
    commands=None,
    env=None,
):
    checks.append(
        {
            "id": id,
            "category": category,
            "severity": "blocker",
            "status": check_status(ok),
            "external": bool(external),
            "title": title,
            "gate": gate,
            "evidence": evidence,
            "required_action": required_action,
            "paths": sorted(str(item) for item in (paths or [])),
            "commands": list(commands or []),
            "env": list(env or []),
        }
    )


remote_names = [line for line in git_stdout("remote").splitlines() if line.strip()]
origin_url = git_stdout("remote", "get-url", "origin")
add_check(
    id="source.remote",
    category="source-control",
    title="Real git remote is configured",
    ok=bool(remote_names or origin_url),
    external=True,
    gate="scripts/release-preflight.sh",
    evidence=origin_url or (", ".join(remote_names) if remote_names else "no git remote configured"),
    required_action="Create or connect the production repository remote and push the reviewed release commit.",
    commands=["git remote add origin <url>", "git push -u origin HEAD"],
)

head_tags = [line for line in git_stdout("tag", "--points-at", "HEAD").splitlines() if line.strip()]
add_check(
    id="source.version-tag",
    category="source-control",
    title=f"HEAD is tagged with {EXPECTED_TAG}",
    ok=EXPECTED_TAG in head_tags,
    external=True,
    gate="scripts/release-preflight.sh",
    evidence=", ".join(head_tags) if head_tags else f"HEAD is not tagged with {EXPECTED_TAG}",
    required_action="Create the immutable release tag from a clean reviewed commit, then push the tag to the real remote.",
    commands=[f"git tag {EXPECTED_TAG}", f"git push origin {EXPECTED_TAG}"],
)

tracked_status = git_stdout("status", "--porcelain", "--untracked-files=no")
add_check(
    id="source.clean-tracked-tree",
    category="source-control",
    title="Tracked worktree is clean",
    ok=tracked_status == "",
    external=False,
    gate="scripts/release-preflight.sh",
    evidence="tracked tree clean" if tracked_status == "" else "tracked changes are present",
    required_action="Commit or intentionally revert tracked source changes before running full release preflight.",
    commands=["git status --short", "git diff --check"],
)

release_tag_env = os.environ.get("KIANA_RELEASE_TAG", "")
add_check(
    id="source.release-tag-env",
    category="source-control",
    title="KIANA_RELEASE_TAG matches VERSION when set",
    ok=not release_tag_env or release_tag_env == EXPECTED_TAG,
    external=False,
    gate="scripts/release-preflight.sh",
    evidence=f"KIANA_RELEASE_TAG={release_tag_env}" if release_tag_env else "KIANA_RELEASE_TAG is unset",
    required_action=f"Unset KIANA_RELEASE_TAG or set it to {EXPECTED_TAG}.",
    env=["KIANA_RELEASE_TAG"],
)

signing_command = os.environ.get("KIANA_SIGNING_COMMAND", "")
signature_verify_command = os.environ.get("KIANA_SIGNATURE_VERIFY_COMMAND", "")
release_signer = os.environ.get("KIANA_RELEASE_SIGNER", "")
signing_ok = (
    bool(signing_command.strip())
    and bool(signature_verify_command.strip())
    and bool(release_signer.strip())
    and release_signer != "external-release-signer"
)
add_check(
    id="signing.release-artifacts",
    category="signing",
    title="Release artifact signing and verification commands are configured",
    ok=signing_ok,
    external=True,
    gate="scripts/sign-release-artifacts.sh",
    evidence=(
        "signing command, verify command, and signer identity are configured"
        if signing_ok
        else "missing KIANA_SIGNING_COMMAND, KIANA_SIGNATURE_VERIFY_COMMAND, or reviewed KIANA_RELEASE_SIGNER"
    ),
    required_action="Configure production signing, signature verification, and reviewed signer identity in the release environment.",
    commands=["bash scripts/sign-release-artifacts.sh"],
    env=[
        "KIANA_SIGNING_COMMAND",
        "KIANA_SIGNATURE_VERIFY_COMMAND",
        "KIANA_RELEASE_SIGNER",
        "KIANA_MACOS_NOTARIZATION_COMMAND",
        "KIANA_MACOS_NOTARIZATION_PROOF_FILE",
    ],
)

dist_dir = Path(os.environ.get("DIST_DIR", "dist"))
manifest_dir = Path(os.environ.get("MANIFEST_DIR", dist_dir / "manifests"))
archives = sorted(dist_dir.glob(f"kiana-{VERSION}-*.tar.gz"))
seen_linux = any(f"-linux-" in archive.name for archive in archives)
seen_macos = any(f"-macos-" in archive.name for archive in archives)
seen_windows = any(f"-windows-" in archive.name for archive in archives)
platform_archives_ok = seen_linux and seen_macos and seen_windows
add_check(
    id="distribution.platform-artifacts",
    category="distribution",
    title="Combined release artifacts include Linux, macOS, and Windows packages",
    ok=platform_archives_ok,
    external=True,
    gate="scripts/verify-commercial-release-artifacts.sh",
    evidence=(
        f"linux={seen_linux} macos={seen_macos} windows={seen_windows} dist={dist_dir}"
        if archives
        else f"no release archives found in {dist_dir}"
    ),
    required_action="Run the release workflow on Linux, macOS, and Windows and combine the produced artifacts before artifact verification.",
    paths=[dist_dir],
    commands=["bash scripts/package-release.sh", "bash scripts/verify-commercial-release-artifacts.sh"],
    env=["DIST_DIR", "MANIFEST_DIR"],
)

homebrew_blocked = (manifest_dir / "homebrew" / "BLOCKED.md").exists()
homebrew_formulae = sorted((manifest_dir / "homebrew").glob("*.rb"))
winget_blocked = (manifest_dir / "winget" / "BLOCKED.md").exists()
winget_manifests = sorted((manifest_dir / "winget").glob(f"*/{VERSION}/*.installer.yaml"))
channel_ok = (not homebrew_blocked) and bool(homebrew_formulae) and (not winget_blocked) and bool(winget_manifests)
add_check(
    id="distribution.package-channels",
    category="distribution",
    title="Homebrew and winget channel manifests are publishable",
    ok=channel_ok,
    external=True,
    gate="scripts/verify-commercial-release-artifacts.sh",
    evidence=(
        f"homebrew_blocked={homebrew_blocked} homebrew_formulae={len(homebrew_formulae)} "
        f"winget_blocked={winget_blocked} winget_manifests={len(winget_manifests)}"
    ),
    required_action="Generate publishable Homebrew formulae and winget manifests from real release URLs; remove channel BLOCKED.md files only when publication is actually possible.",
    paths=[manifest_dir / "homebrew", manifest_dir / "winget"],
    commands=["bash scripts/generate-distribution-manifests.sh", "bash scripts/verify-commercial-release-artifacts.sh"],
    env=["KIANA_RELEASE_BASE_URL", "DIST_DIR", "MANIFEST_DIR"],
)

enterprise_manifest = manifest_dir / "enterprise" / "offline-manifest.json"
enterprise, enterprise_error = load_json(enterprise_manifest)
enterprise_ok = (
    isinstance(enterprise, dict)
    and enterprise.get("schema") == "kiana.enterprise.offline-manifest.v1"
    and enterprise.get("version") == VERSION
    and "pending_" not in json.dumps(enterprise)
    and "dry_run" not in json.dumps(enterprise)
    and "blocked_" not in json.dumps(enterprise)
)
add_check(
    id="distribution.enterprise-offline-manifest",
    category="distribution",
    title="Enterprise offline manifest has no pending or blocked channel state",
    ok=enterprise_ok,
    external=True,
    gate="scripts/verify-commercial-release-artifacts.sh",
    evidence=f"manifest={enterprise_manifest}" if enterprise_error is None else f"manifest={enterprise_error}: {enterprise_manifest}",
    required_action="Regenerate the enterprise offline manifest from final release artifacts and real release URLs after all channels are publishable.",
    paths=[enterprise_manifest],
    commands=["bash scripts/generate-distribution-manifests.sh", "bash scripts/verify-commercial-release-artifacts.sh"],
    env=["KIANA_RELEASE_BASE_URL", "DIST_DIR", "MANIFEST_DIR"],
)

live_root = Path(os.environ.get("KIANA_LIVE_SMOKE_DIR", "target/live-smoke"))
provider_dir = Path(os.environ.get("KIANA_PROVIDER_LIVE_SMOKE_DIR", live_root / "provider"))
provider_catalog = provider_dir / "model-catalog-live.json"
provider_smoke = provider_dir / "model-smoke-live-tools.json"
catalog, catalog_error = load_json(provider_catalog)
smoke, smoke_error = load_json(provider_smoke)
live_text_passed = False
live_tools_passed = False
provider_ok = False
if isinstance(catalog, dict) and isinstance(smoke, dict):
    live_text_passed = any(
        item.get("provider_id") != "fake"
        and item.get("live") is True
        and item.get("capability") == "text"
        and item.get("status") == "passed"
        for item in smoke.get("results", [])
        if isinstance(item, dict)
    )
    live_tools_passed = any(
        item.get("provider_id") != "fake"
        and item.get("live") is True
        and item.get("capability") == "tools"
        and item.get("status") == "passed"
        for item in smoke.get("results", [])
        if isinstance(item, dict)
    )
    provider_ok = (
        catalog.get("schema") == "kiana.model-catalog.v1"
        and catalog.get("live") is True
        and smoke.get("schema") == "kiana.model-smoke.v1"
        and smoke.get("live") is True
        and smoke.get("tools") is True
        and live_text_passed
        and live_tools_passed
    )
provider_evidence = (
    f"text={live_text_passed} tools={live_tools_passed} catalog={provider_catalog} smoke={provider_smoke}"
    if catalog_error is None and smoke_error is None
    else f"catalog={catalog_error or 'present'} smoke={smoke_error or 'present'}"
)
add_check(
    id="live.provider-smoke",
    category="live-service",
    title="Provider live text and tool-call smoke has accepted proof",
    ok=provider_ok,
    external=True,
    gate="scripts/provider-live-smoke.sh --required",
    evidence=provider_evidence,
    required_action="Run provider live smoke against production-like Anthropic, OpenAI-compatible, or Ollama credentials and keep the proof JSON.",
    paths=[provider_catalog, provider_smoke],
    commands=["bash scripts/provider-live-smoke.sh --required"],
    env=[
        "ANTHROPIC_API_KEY",
        "KIANA_OPENAI_API_KEY",
        "OPENAI_API_KEY",
        "KIANA_OLLAMA_BASE_URL",
        "OLLAMA_BASE_URL",
    ],
)

remote_dir = Path(os.environ.get("KIANA_REMOTE_LIVE_SMOKE_DIR", live_root / "remote"))
remote_proof = remote_dir / "code-session-smoke.json"
remote, remote_error = load_json(remote_proof)
remote_ok = (
    isinstance(remote, dict)
    and remote.get("schema") == "kiana.remote-code-session-smoke.v1"
    and remote.get("status") == "ok"
    and isinstance(remote.get("session_id"), str)
    and remote["session_id"].startswith("cse_")
)
add_check(
    id="live.remote-code-session",
    category="live-service",
    title="Remote code-session live smoke has accepted proof",
    ok=remote_ok,
    external=True,
    gate="scripts/remote-live-smoke.sh --required",
    evidence=f"proof={remote_proof}" if remote_error is None else f"proof={remote_error}: {remote_proof}",
    required_action="Run remote live smoke against the production-like CCR/session service and keep the proof JSON.",
    paths=[remote_proof],
    commands=["bash scripts/remote-live-smoke.sh --required"],
    env=[
        "KIANA_REMOTE_ACCESS_TOKEN",
        "CLAUDE_ACCESS_TOKEN",
        "ANTHROPIC_AUTH_TOKEN",
        "KIANA_OAUTH_TOKENS_FILE",
    ],
)

product_file = Path(os.environ.get("KIANA_PRODUCT_ACCEPTANCE_FILE", f"docs/product-acceptance/{VERSION}.json"))
product, product_error = load_json(product_file)
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
product_workflows = set(product.get("workflows") or []) if isinstance(product, dict) else set()
product_ok = (
    isinstance(product, dict)
    and product.get("schema") == "kiana.product-acceptance.v1"
    and product.get("version") == VERSION
    and product.get("status") == "accepted"
    and product.get("accepted") is True
    and filled(product, "accepted_by")
    and filled(product, "accepted_at")
    and required_workflows.issubset(product_workflows)
)
missing_workflows = sorted(required_workflows - product_workflows)
add_check(
    id="acceptance.product",
    category="acceptance",
    title="Target-customer product acceptance proof is accepted",
    ok=product_ok,
    external=True,
    gate="scripts/product-acceptance-report.sh full",
    evidence=(
        f"missing workflows: {', '.join(missing_workflows)}"
        if product_error is None and missing_workflows
        else (f"proof={product_file}" if product_error is None else f"proof={product_error}: {product_file}")
    ),
    required_action="Record target-customer acceptance in kiana.product-acceptance.v1 with all required workflows.",
    paths=[product_file],
    commands=["bash scripts/product-acceptance-report.sh full"],
    env=["KIANA_PRODUCT_ACCEPTANCE_FILE"],
)

entitlement_file = Path(os.environ.get("KIANA_ENTITLEMENT_PROOF_FILE", f"docs/entitlements/{VERSION}.json"))
entitlement, entitlement_error = load_json(entitlement_file)
required_entitlements = {
    item.strip()
    for item in os.environ.get(
        "KIANA_REQUIRED_ENTITLEMENTS",
        "commercial-use,enterprise-support,managed-policy",
    ).split(",")
    if item.strip()
}
backend = entitlement.get("backend") if isinstance(entitlement, dict) else {}
backend = backend if isinstance(backend, dict) else {}
entitlement_values = set(entitlement.get("entitlements") or []) if isinstance(entitlement, dict) else set()
entitlement_ok = (
    isinstance(entitlement, dict)
    and entitlement.get("schema") == "kiana.entitlement-proof.v1"
    and entitlement.get("version") == VERSION
    and entitlement.get("status") == "accepted"
    and entitlement.get("accepted") is True
    and entitlement.get("license_status") == "active"
    and required_entitlements.issubset(entitlement_values)
    and all(filled(entitlement, key) and not_placeholder(entitlement, key) for key in [
        "accepted_by",
        "accepted_at",
        "account_id",
        "organization",
        "plan",
        "license_key_fingerprint",
        "support_contact",
    ])
    and all(filled(backend, key) and not_placeholder(backend, key) for key in [
        "name",
        "environment",
        "checked_at",
        "request_id",
    ])
)
missing_entitlements = sorted(required_entitlements - entitlement_values)
add_check(
    id="acceptance.entitlement",
    category="acceptance",
    title="Enterprise entitlement proof is active and accepted",
    ok=entitlement_ok,
    external=True,
    gate="scripts/entitlement-proof-report.sh full",
    evidence=(
        f"missing entitlements: {', '.join(missing_entitlements)}"
        if entitlement_error is None and missing_entitlements
        else (f"proof={entitlement_file}" if entitlement_error is None else f"proof={entitlement_error}: {entitlement_file}")
    ),
    required_action="Record the production license/account backend acceptance in kiana.entitlement-proof.v1.",
    paths=[entitlement_file],
    commands=["bash scripts/entitlement-proof-report.sh full"],
    env=["KIANA_ENTITLEMENT_PROOF_FILE", "KIANA_REQUIRED_ENTITLEMENTS"],
)

ops_file = Path(os.environ.get("KIANA_RELEASE_OPS_FILE", f"docs/release-ops/{VERSION}.json"))
ops, ops_error = load_json(ops_file)
credential_review = ops.get("credential_review") if isinstance(ops, dict) else {}
credential_review = credential_review if isinstance(credential_review, dict) else {}
ops_ok = (
    isinstance(ops, dict)
    and ops.get("schema") == "kiana.release-ops.v1"
    and ops.get("version") == VERSION
    and ops.get("status") == "accepted"
    and ops.get("accepted") is True
    and all(filled(ops, key) and not_placeholder(ops, key) for key in [
        "accepted_by",
        "accepted_at",
        "security_contact",
        "vulnerability_report_channel",
        "release_credentials_owner",
        "support_contact",
    ])
    and isinstance(ops.get("artifact_retention_days"), int)
    and ops["artifact_retention_days"] >= 90
    and isinstance(ops.get("log_retention_days"), int)
    and ops["log_retention_days"] >= 30
    and credential_review.get("status") == "accepted"
    and all(filled(credential_review, key) for key in ["reviewed_by", "reviewed_at", "scope"])
)
add_check(
    id="acceptance.release-ops",
    category="acceptance",
    title="Release operations proof is accepted",
    ok=ops_ok,
    external=True,
    gate="scripts/release-ops-report.sh full",
    evidence=f"proof={ops_file}" if ops_error is None else f"proof={ops_error}: {ops_file}",
    required_action="Record private vulnerability route, release credential ownership, support contact, retention policy, and credential review in kiana.release-ops.v1.",
    paths=[ops_file],
    commands=["bash scripts/release-ops-report.sh full"],
    env=["KIANA_RELEASE_OPS_FILE"],
)

current_platform = platform_id()
expected_isolation = {
    "linux": "linux_bwrap",
    "macos": "macos_exec_policy",
    "windows": "windows_exec_policy",
}.get(current_platform, "unknown")
platform_dir = Path(os.environ.get("KIANA_PLATFORM_SECURITY_PROOF_DIR", "docs/platform-security"))
platform_file = Path(os.environ.get("KIANA_PLATFORM_SECURITY_PROOF_FILE", platform_dir / f"{VERSION}-{current_platform}.json"))
platform_proof, platform_error = load_json(platform_file)
platform_ok = (
    current_platform != "unknown"
    and isinstance(platform_proof, dict)
    and platform_proof.get("schema") == "kiana.platform-security-proof.v1"
    and platform_proof.get("version") == VERSION
    and platform_proof.get("status") == "accepted"
    and platform_proof.get("accepted") is True
    and platform_proof.get("platform") == current_platform
    and platform_proof.get("isolation") == expected_isolation
    and platform_proof.get("doctor_status") == "ready"
    and filled(platform_proof, "accepted_by")
    and filled(platform_proof, "accepted_at")
    and filled(platform_proof, "runner")
    and isinstance(platform_proof.get("controls"), list)
    and len(platform_proof["controls"]) > 0
    and isinstance(platform_proof.get("evidence"), list)
    and len(platform_proof["evidence"]) > 0
)
add_check(
    id="acceptance.platform-security",
    category="acceptance",
    title=f"{current_platform} platform security proof is accepted",
    ok=platform_ok,
    external=True,
    gate="scripts/platform-security-proof-report.sh full",
    evidence=f"proof={platform_file}" if platform_error is None else f"proof={platform_error}: {platform_file}",
    required_action="Record accepted platform security proof from the real release runner with ready doctor status and expected isolation.",
    paths=[platform_file],
    commands=["bash scripts/platform-security-proof-report.sh full"],
    env=["KIANA_PLATFORM_SECURITY_PROOF_FILE", "KIANA_PLATFORM_SECURITY_PROOF_DIR"],
)

blocking_checks = [check for check in checks if check["status"] == "blocking"]
external_blocking = [check for check in blocking_checks if check["external"]]
local_blocking = [check for check in blocking_checks if not check["external"]]
report = {
    "schema": "kiana.commercial-release-blockers.v1",
    "version": VERSION,
    "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "status": "blocked" if blocking_checks else "ready",
    "release_tag": EXPECTED_TAG,
    "summary": {
        "total_checks": len(checks),
        "satisfied": len(checks) - len(blocking_checks),
        "blocking": len(blocking_checks),
        "external_blocking": len(external_blocking),
        "local_blocking": len(local_blocking),
    },
    "checks": checks,
}

out = os.environ.get("KIANA_COMMERCIAL_BLOCKERS_OUT", "")
if out:
    path = Path(out)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

if os.environ.get("KIANA_BLOCKERS_FORMAT") == "json":
    print(json.dumps(report, indent=2, sort_keys=True))
else:
    summary = report["summary"]
    print(f"Kiana commercial release blockers for {VERSION} ({EXPECTED_TAG})")
    print(
        "Status: "
        f"{report['status']} "
        f"({summary['blocking']} blocking, "
        f"{summary['external_blocking']} external, "
        f"{summary['local_blocking']} local)"
    )
    if out:
        print(f"JSON report: {out}")
    if blocking_checks:
        print("")
        print("Blocking:")
        for check in blocking_checks:
            owner = "external" if check["external"] else "local"
            print(f"- {check['id']} [{owner}]: {check['title']}")
            print(f"  evidence: {check['evidence']}")
            print(f"  gate: {check['gate']}")
            print(f"  action: {check['required_action']}")
    else:
        print("")
        print("No blocking checks were detected by the lightweight report.")

if os.environ.get("KIANA_BLOCKERS_FAIL_ON_BLOCKERS") == "1" and blocking_checks:
    sys.exit(1)
PY
