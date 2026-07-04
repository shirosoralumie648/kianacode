#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

format="text"
fail_on_blockers=0
handoff_out="${KIANA_COMMERCIAL_HANDOFF_OUT:-}"

while (($# > 0)); do
  case "$1" in
    --json)
      format="json"
      ;;
    --fail-on-blockers)
      fail_on_blockers=1
      ;;
    --handoff-md)
      shift
      if (($# == 0)); then
        echo "--handoff-md requires a path" >&2
        exit 2
      fi
      handoff_out="$1"
      ;;
    --handoff-md=*)
      handoff_out="${1#--handoff-md=}"
      ;;
    -h|--help)
      cat <<'EOF'
usage: scripts/commercial-release-blockers-report.sh [--json] [--fail-on-blockers] [--handoff-md PATH]

Writes a lightweight commercial release readiness report without running cargo,
network, signing, or package-manager publication gates.

Set KIANA_COMMERCIAL_BLOCKERS_OUT to also write the JSON report to a file.
Set KIANA_COMMERCIAL_HANDOFF_OUT or pass --handoff-md to write an assignment
handoff Markdown file for the remaining blockers.
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
KIANA_BLOCKERS_HANDOFF_OUT="$handoff_out" \
"$(python_bin)" - <<'PY'
import json
import os
import re
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
FINGERPRINT_PATTERN = re.compile(r"^sha256:[0-9a-f]{64}$")


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


def valid_fingerprint(mapping, key):
    value = mapping.get(key)
    return isinstance(value, str) and bool(FINGERPRINT_PATTERN.fullmatch(value))


def unique_paths(paths):
    seen = set()
    result = []
    for path in paths:
        if path is None:
            continue
        path = Path(path)
        if not str(path):
            continue
        key = str(path)
        if key in seen:
            continue
        seen.add(key)
        result.append(path)
    return result


def sha256_file(path):
    import hashlib

    digest = hashlib.sha256()
    with Path(path).open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def first_checksum(path):
    with Path(path).open("r", encoding="utf-8") as handle:
        for line in handle:
            parts = line.split()
            if parts:
                return parts[0]
    return ""


def real_release_url(value):
    if not isinstance(value, str):
        return False
    lowered = value.lower()
    return value.startswith("https://") and not any(
        marker in lowered for marker in PLACEHOLDER_MARKERS
    )


def enterprise_offline_manifest_contract(manifest, dist_dir):
    if not isinstance(manifest, dict):
        return False, "enterprise offline manifest missing or invalid"
    base_url = manifest.get("release_base_url")
    channels = manifest.get("channels")
    channels = channels if isinstance(channels, dict) else {}
    artifacts = manifest.get("artifacts")
    payload = json.dumps(manifest)
    placeholder_markers = (
        "github.com/kiana-project/kiana",
        "example.com",
        "example.test",
        "localhost",
        "127.0.0.1",
        "pending_",
        "blocked_",
        "dry_run",
    )
    basic_checks = [
        manifest.get("schema") == "kiana.enterprise.offline-manifest.v1",
        manifest.get("version") == VERSION,
        manifest.get("generated_by") == "scripts/generate-distribution-manifests.sh",
        isinstance(base_url, str) and base_url.startswith("https://"),
        isinstance(base_url, str)
        and not any(marker in base_url.lower() for marker in placeholder_markers),
        not any(marker in payload.lower() for marker in ("pending_", "blocked_", "dry_run")),
        channels.get("github_releases") == "generated_from_release_base_url",
        channels.get("homebrew") == "generated",
        channels.get("winget") == "generated",
        isinstance(artifacts, list) and len(artifacts) > 0,
    ]
    if not all(basic_checks):
        return False, "enterprise offline manifest failed channel/base contract"

    for item in artifacts:
        if not isinstance(item, dict):
            return False, "enterprise offline manifest failed artifact contract"
        archive = item.get("archive")
        local_path = item.get("local_path")
        checksum_path = item.get("checksum_path")
        binary_checksum_path = item.get("binary_checksum_path")
        url = item.get("url")
        archive_file = dist_dir / local_path if isinstance(local_path, str) else None
        archive_checksum_file = dist_dir / checksum_path if isinstance(checksum_path, str) else None
        binary_checksum_file = (
            dist_dir / binary_checksum_path if isinstance(binary_checksum_path, str) else None
        )
        required_fields = [
            "target",
            "archive",
            "url",
            "sha256",
            "binary_sha256",
            "local_path",
            "checksum_path",
            "binary_checksum_path",
        ]
        paths_match = (
            isinstance(archive, str)
            and local_path == archive
            and checksum_path == f"{archive}.sha256"
            and binary_checksum_path == f"{archive[:-7]}.binary.sha256"
        )
        files_match = (
            archive_file is not None
            and archive_checksum_file is not None
            and binary_checksum_file is not None
            and archive_file.is_file()
            and archive_checksum_file.is_file()
            and binary_checksum_file.is_file()
            and item.get("sha256") == sha256_file(archive_file)
            and item.get("sha256") == first_checksum(archive_checksum_file)
            and item.get("binary_sha256") == first_checksum(binary_checksum_file)
        )
        if not (
            all(filled(item, key) for key in required_fields)
            and real_release_url(url)
            and isinstance(base_url, str)
            and url.startswith(base_url)
            and paths_match
            and files_match
        ):
            return False, "enterprise offline manifest failed commercial contract"
    return True, "enterprise offline manifest commercial contract accepted"


def homebrew_formula_field(content, field):
    match = re.search(rf'^\s*{re.escape(field)}\s+"([^"]+)"\s*$', content, re.MULTILINE)
    return match.group(1) if match else ""


def homebrew_formula_contract(formulae, dist_dir):
    if not formulae:
        return False, "homebrew formula missing"
    for formula in formulae:
        try:
            content = formula.read_text(encoding="utf-8")
        except OSError as exc:
            return False, f"homebrew formula unreadable: {formula}: {exc}"
        url = homebrew_formula_field(content, "url")
        formula_sha = homebrew_formula_field(content, "sha256")
        version = homebrew_formula_field(content, "version")
        if not (
            version == VERSION
            and real_release_url(url)
            and url.endswith(".tar.gz")
            and re.fullmatch(r"[0-9a-fA-F]{64}", formula_sha or "") is not None
        ):
            return False, "homebrew formula failed commercial contract"
        archive_name = url.rsplit("/", 1)[-1]
        archive = dist_dir / archive_name
        archive_checksum = dist_dir / f"{archive_name}.sha256"
        if not (
            archive.is_file()
            and archive_checksum.is_file()
            and formula_sha.lower() == sha256_file(archive)
            and formula_sha.lower() == first_checksum(archive_checksum).lower()
        ):
            return False, "homebrew formula failed commercial contract"
    return True, "homebrew formula commercial contract accepted"


def winget_manifest_contract(manifests, dist_dir):
    if not manifests:
        return False, "winget manifest missing"
    for manifest in manifests:
        try:
            content = manifest.read_text(encoding="utf-8")
        except OSError as exc:
            return False, f"winget manifest unreadable: {manifest}: {exc}"
        fields = {}
        for raw_line in content.splitlines():
            line = raw_line.strip()
            if not line or ":" not in line:
                continue
            key, value = line.split(":", 1)
            fields[key.strip()] = value.strip().strip('"').strip("'")
        installer_url = fields.get("InstallerUrl", "")
        installer_sha = fields.get("InstallerSha256", "")
        installer_type = fields.get("InstallerType", "")
        manifest_type = fields.get("ManifestType", "")
        version = fields.get("PackageVersion", "")
        if not (
            installer_type == "zip"
            and manifest_type == "installer"
            and version == VERSION
            and real_release_url(installer_url)
            and installer_url.endswith(".zip")
            and re.fullmatch(r"[0-9a-fA-F]{64}", installer_sha or "") is not None
        ):
            return False, "winget manifest failed commercial contract"
        zip_name = installer_url.rsplit("/", 1)[-1]
        zip_file = dist_dir / zip_name
        zip_checksum = dist_dir / f"{zip_name}.sha256"
        if not (
            zip_file.is_file()
            and zip_checksum.is_file()
            and installer_sha.lower() == sha256_file(zip_file)
            and installer_sha.lower() == first_checksum(zip_checksum).lower()
        ):
            return False, "winget manifest failed commercial contract"
    return True, "winget manifest commercial contract accepted"


def source_control_proof_candidates():
    return [
        Path(value)
        for value in [
            os.environ.get("KIANA_SOURCE_CONTROL_PROOF_FILE", ""),
            os.environ.get("KIANA_SOURCE_CONTROL_PROOF_OUT", ""),
        ]
        if value
    ] + [
        Path(os.environ.get("DIST_DIR", "dist")) / "proofs/source-control/source-control.json",
        Path(f"docs/source-control/{VERSION}.json"),
    ]


def accepted_source_control_proof():
    for path in source_control_proof_candidates():
        data, error = load_json(path)
        if error is not None or not isinstance(data, dict):
            continue
        commit = data.get("commit")
        tagged_commit = data.get("tagged_commit")
        remote_url = data.get("remote_url")
        accepted = (
            data.get("schema") == "kiana.source-control-proof.v1"
            and data.get("version") == VERSION
            and data.get("status") == "accepted"
            and data.get("accepted") is True
            and data.get("pushed") is True
            and data.get("reviewed") is True
            and data.get("release_tag") == EXPECTED_TAG
            and isinstance(commit, str)
            and re.fullmatch(r"[0-9a-f]{40}", commit) is not None
            and isinstance(tagged_commit, str)
            and re.fullmatch(r"[0-9a-f]{40}", tagged_commit) is not None
            and commit == tagged_commit
            and all(filled(data, key) for key in ["accepted_by", "accepted_at", "remote_url"])
            and all(not_placeholder(data, key) for key in ["accepted_by", "remote_url"])
            and isinstance(remote_url, str)
            and remote_url.startswith(("https://", "ssh://", "git@"))
        )
        if accepted:
            return path, data
    return None, None


def accepted_release_signature_proofs(dist_dir):
    archives = sorted(Path(dist_dir).glob(f"kiana-{VERSION}-*.tar.gz"))
    accepted = []
    errors = []
    for archive in archives:
        package = archive.name.removesuffix(".tar.gz")
        target = package.removeprefix(f"kiana-{VERSION}-")
        binary_sha = archive.parent / f"{package}.binary.sha256"
        archive_sig = archive.parent / f"{archive.name}.sig"
        binary_sig = archive.parent / f"{package}.binary.sig"
        proof_path = archive.parent / f"{package}.signature.json"
        proof, error = load_json(proof_path)
        if error is not None or not isinstance(proof, dict):
            errors.append(f"{target}: signature proof {error or 'invalid'}")
            continue
        signature_files = proof.get("signature_files")
        signature_files = signature_files if isinstance(signature_files, dict) else {}
        verification = proof.get("verification")
        verification = verification if isinstance(verification, dict) else {}
        checks = [
            archive.is_file(),
            binary_sha.is_file(),
            archive_sig.is_file() and archive_sig.stat().st_size > 0,
            binary_sig.is_file() and binary_sig.stat().st_size > 0,
            proof.get("schema") == "kiana.release-signature.v1",
            proof.get("target") == target,
            proof.get("archive") == archive.name,
            proof.get("archive_sha256") == sha256_file(archive),
            proof.get("binary_sha256_file_sha256") == sha256_file(binary_sha)
            if binary_sha.is_file()
            else False,
            filled(proof, "signed_at"),
            filled(proof, "signer"),
            not_placeholder(proof, "signer"),
            signature_files.get("archive") == archive_sig.name,
            signature_files.get("binary") == binary_sig.name,
            verification.get("method") == "KIANA_SIGNATURE_VERIFY_COMMAND",
            verification.get("archive") == "verified",
            verification.get("binary") == "verified",
            filled(verification, "verified_at"),
        ]
        if all(checks):
            accepted.append(target)
        else:
            errors.append(f"{target}: signature proof failed accepted contract")
    return accepted, errors


def check_status(ok):
    return "satisfied" if ok else "blocking"


def blocker_env_key(prefix, check_id):
    normalized = "".join(ch if ch.isalnum() else "_" for ch in check_id.upper())
    return f"{prefix}_{normalized}"


def default_owner(category, external):
    if not external:
        return "local-release-automation"
    return {
        "source-control": "release-manager",
        "build-test": "release-engineering",
        "signing": "release-security",
        "distribution": "release-engineering",
        "live-service": "service-owner",
        "acceptance": "acceptance-owner",
    }.get(category, "release-owner")


def default_handoff_notes(external, paths, env):
    notes = []
    if external:
        notes.append("Assign this blocker to the named operational owner before the release review.")
    else:
        notes.append("Resolve this local blocker before asking external owners to act.")
    if paths:
        notes.append("Attach or regenerate every listed acceptance artifact before closing the check.")
    if env:
        notes.append("Run the verification commands with the listed environment variables set from production-approved sources.")
    return notes


def default_acceptance_artifacts(check_id, paths):
    if paths:
        return paths
    return {
        "source.remote": [
            f"docs/source-control/{VERSION}.json or external kiana.source-control-proof.v1 proof",
            "production git remote URL",
            "pushed reviewed release commit",
        ],
        "source.version-tag": [
            f"docs/source-control/{VERSION}.json or external kiana.source-control-proof.v1 proof",
            f"{EXPECTED_TAG} tag on the reviewed release commit",
            "pushed immutable release tag",
        ],
        "source.clean-tracked-tree": [
            "clean `git status --short` output",
            "clean `git diff --check` output",
        ],
        "source.release-tag-env": [
            "release preflight environment with matching KIANA_RELEASE_TAG",
        ],
        "signing.release-artifacts": [
            "dist/proofs/signing/release-signature.json",
            "dist/proofs/macos/notarization.json",
            "signature files beside every release archive and binary checksum",
        ],
        "build.locked-offline-cache": [
            "complete Cargo registry source cache for every locked registry package",
            "successful locked/offline Cargo metadata or release-smoke run",
        ],
    }.get(check_id, [])


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
    owner=None,
    owner_status=None,
    acceptance_artifacts=None,
    verification_commands=None,
    handoff_notes=None,
):
    paths = sorted(str(item) for item in (paths or []))
    commands = list(commands or [])
    env = list(env or [])
    owner_env = os.environ.get(blocker_env_key("KIANA_BLOCKER_OWNER", id), "").strip()
    effective_owner = owner_env or owner or default_owner(category, external)
    effective_owner_status = owner_status or (
        "specific-owner-assigned"
        if owner_env
        else ("role-owner-required" if external else "local-owner")
    )
    effective_acceptance_artifacts = list(
        acceptance_artifacts or default_acceptance_artifacts(id, paths)
    )
    effective_verification_commands = list(verification_commands or commands or [gate])
    effective_handoff_notes = list(handoff_notes or default_handoff_notes(external, paths, env))
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
            "paths": paths,
            "commands": commands,
            "env": env,
            "owner": effective_owner,
            "owner_status": effective_owner_status,
            "acceptance_artifacts": sorted(str(item) for item in effective_acceptance_artifacts),
            "verification_commands": effective_verification_commands,
            "handoff_notes": effective_handoff_notes,
        }
    )


remote_names = [line for line in git_stdout("remote").splitlines() if line.strip()]
origin_url = git_stdout("remote", "get-url", "origin")
source_control_proof_path, source_control_proof = accepted_source_control_proof()
source_control_proof_evidence = ""
if source_control_proof:
    source_control_proof_evidence = (
        "source-control proof accepted: "
        f"{source_control_proof_path} "
        f"remote={source_control_proof.get('remote_url')} "
        f"tag={source_control_proof.get('release_tag')} "
        f"commit={source_control_proof.get('commit')}"
    )
add_check(
    id="source.remote",
    category="source-control",
    title="Real git remote is configured",
    ok=bool(remote_names or origin_url or source_control_proof),
    external=True,
    gate="scripts/release-preflight.sh",
    evidence=source_control_proof_evidence
    or origin_url
    or (", ".join(remote_names) if remote_names else "no git remote configured"),
    required_action="Create or connect the production repository remote and push the reviewed release commit.",
    commands=["git remote add origin <url>", "git push -u origin HEAD"],
)

head_tags = [line for line in git_stdout("tag", "--points-at", "HEAD").splitlines() if line.strip()]
add_check(
    id="source.version-tag",
    category="source-control",
    title=f"HEAD is tagged with {EXPECTED_TAG}",
    ok=EXPECTED_TAG in head_tags or bool(source_control_proof),
    external=True,
    gate="scripts/release-preflight.sh",
    evidence=source_control_proof_evidence
    or (", ".join(head_tags) if head_tags else f"HEAD is not tagged with {EXPECTED_TAG}"),
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


def locked_registry_packages(lock_path):
    packages = []
    current = {}
    if not lock_path.exists():
        return packages, f"missing: {lock_path}"
    for raw_line in lock_path.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if line == "[[package]]":
            if current.get("source", "").startswith("registry+") and current.get("name") and current.get("version"):
                packages.append((current["name"], current["version"]))
            current = {}
            continue
        if "=" not in line:
            continue
        key, value = line.split("=", 1)
        key = key.strip()
        value = value.strip().strip('"')
        if key in {"name", "version", "source"}:
            current[key] = value
    if current.get("source", "").startswith("registry+") and current.get("name") and current.get("version"):
        packages.append((current["name"], current["version"]))
    return sorted(set(packages)), None


def cargo_registry_src_roots():
    cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
    src_root = cargo_home / "registry" / "src"
    if not src_root.exists():
        return []
    return [path for path in src_root.iterdir() if path.is_dir()]


def missing_locked_cargo_sources():
    packages, error = locked_registry_packages(ROOT / "Cargo.lock")
    if error:
        return [], 0, error
    roots = cargo_registry_src_roots()
    if not roots and packages:
        return [f"{name}-{version}" for name, version in packages], len(packages), "no Cargo registry source cache found"
    missing = []
    for name, version in packages:
        dirname = f"{name}-{version}"
        if not any((root / dirname).is_dir() for root in roots):
            missing.append(dirname)
    return missing, len(packages), None


missing_cargo_sources, locked_cargo_packages, cargo_source_error = missing_locked_cargo_sources()
cargo_source_sample = ", ".join(missing_cargo_sources[:8])
if len(missing_cargo_sources) > 8:
    cargo_source_sample += f", ... (+{len(missing_cargo_sources) - 8} more)"
add_check(
    id="build.locked-offline-cache",
    category="build-test",
    title="Locked Cargo registry source cache supports offline release gates",
    ok=not missing_cargo_sources and cargo_source_error is None,
    external=False,
    gate="cargo test --workspace --locked --offline --no-fail-fast",
    evidence=(
        f"all {locked_cargo_packages} locked registry packages are cached"
        if not missing_cargo_sources and cargo_source_error is None
        else f"{cargo_source_error or 'missing locked crate sources'}: {cargo_source_sample}"
    ),
    required_action="Refresh the Cargo registry source cache for the locked dependency set, or run the strict release gates on a runner with the required locked crate sources available.",
    paths=["Cargo.lock", "${CARGO_HOME:-~/.cargo}/registry/src"],
    commands=[
        "cargo fetch --locked",
        "cargo test --workspace --locked --offline --no-fail-fast",
        "bash scripts/release-smoke.sh",
    ],
    env=["CARGO_HOME", "CARGO_REGISTRIES_CRATES_IO_PROTOCOL", "CARGO_NET_GIT_FETCH_WITH_CLI"],
)


signing_command = os.environ.get("KIANA_SIGNING_COMMAND", "")
signature_verify_command = os.environ.get("KIANA_SIGNATURE_VERIFY_COMMAND", "")
release_signer = os.environ.get("KIANA_RELEASE_SIGNER", "")
dist_dir = Path(os.environ.get("DIST_DIR", "dist"))
manifest_dir = Path(os.environ.get("MANIFEST_DIR", dist_dir / "manifests"))
accepted_signature_targets, signature_proof_errors = accepted_release_signature_proofs(dist_dir)
signing_env_ok = (
    bool(signing_command.strip())
    and bool(signature_verify_command.strip())
    and bool(release_signer.strip())
    and release_signer != "external-release-signer"
)
signing_proofs_ok = bool(accepted_signature_targets)
signing_ok = signing_env_ok or signing_proofs_ok
add_check(
    id="signing.release-artifacts",
    category="signing",
    title="Release artifact signing and verification proof is accepted",
    ok=signing_ok,
    external=True,
    gate="scripts/sign-release-artifacts.sh",
    evidence=(
        f"release signature proofs accepted: {', '.join(accepted_signature_targets)}"
        if signing_proofs_ok
        else (
            "signing command, verify command, and signer identity are configured"
            if signing_env_ok
            else "missing KIANA_SIGNING_COMMAND, KIANA_SIGNATURE_VERIFY_COMMAND, reviewed KIANA_RELEASE_SIGNER, or accepted release signature proof"
        )
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
homebrew_contract_ok, homebrew_contract_evidence = homebrew_formula_contract(
    homebrew_formulae, dist_dir
)
winget_contract_ok, winget_contract_evidence = winget_manifest_contract(
    winget_manifests, dist_dir
)
channel_ok = (
    (not homebrew_blocked)
    and homebrew_contract_ok
    and (not winget_blocked)
    and winget_contract_ok
)
add_check(
    id="distribution.package-channels",
    category="distribution",
    title="Homebrew and winget channel manifests are publishable",
    ok=channel_ok,
    external=True,
    gate="scripts/verify-commercial-release-artifacts.sh",
    evidence=(
        f"homebrew_blocked={homebrew_blocked} homebrew_formulae={len(homebrew_formulae)} "
        f"homebrew_contract={homebrew_contract_evidence} "
        f"winget_blocked={winget_blocked} winget_manifests={len(winget_manifests)} "
        f"winget_contract={winget_contract_evidence}"
    ),
    required_action="Generate publishable Homebrew formulae and winget manifests from real release URLs; remove channel BLOCKED.md files only when publication is actually possible.",
    paths=[manifest_dir / "homebrew", manifest_dir / "winget"],
    commands=["bash scripts/generate-distribution-manifests.sh", "bash scripts/verify-commercial-release-artifacts.sh"],
    env=["KIANA_RELEASE_BASE_URL", "DIST_DIR", "MANIFEST_DIR"],
)

enterprise_manifest = manifest_dir / "enterprise" / "offline-manifest.json"
enterprise, enterprise_error = load_json(enterprise_manifest)
enterprise_contract_ok, enterprise_contract_evidence = enterprise_offline_manifest_contract(
    enterprise, dist_dir
)
enterprise_ok = (
    enterprise_error is None
    and enterprise_contract_ok
)
add_check(
    id="distribution.enterprise-offline-manifest",
    category="distribution",
    title="Enterprise offline manifest has no pending or blocked channel state",
    ok=enterprise_ok,
    external=True,
    gate="scripts/verify-commercial-release-artifacts.sh",
    evidence=(
        f"{enterprise_contract_evidence}: {enterprise_manifest}"
        if enterprise_error is None
        else f"manifest={enterprise_error}: {enterprise_manifest}"
    ),
    required_action="Regenerate the enterprise offline manifest from final release artifacts and real release URLs after all channels are publishable.",
    paths=[enterprise_manifest],
    commands=["bash scripts/generate-distribution-manifests.sh", "bash scripts/verify-commercial-release-artifacts.sh"],
    env=["KIANA_RELEASE_BASE_URL", "DIST_DIR", "MANIFEST_DIR"],
)

live_root = Path(os.environ.get("KIANA_LIVE_SMOKE_DIR", "target/live-smoke"))
provider_dir = Path(os.environ.get("KIANA_PROVIDER_LIVE_SMOKE_DIR", live_root / "provider"))
provider_catalog_candidates = unique_paths(
    [
        Path(os.environ["KIANA_PROVIDER_CATALOG_PROOF_FILE"])
        if os.environ.get("KIANA_PROVIDER_CATALOG_PROOF_FILE")
        else None,
        dist_dir / "proofs/live-smoke/provider/model-catalog-live.json",
        provider_dir / "model-catalog-live.json",
    ]
)
provider_smoke_candidates = unique_paths(
    [
        Path(os.environ["KIANA_PROVIDER_SMOKE_PROOF_FILE"])
        if os.environ.get("KIANA_PROVIDER_SMOKE_PROOF_FILE")
        else None,
        dist_dir / "proofs/live-smoke/provider/model-smoke-live-tools.json",
        provider_dir / "model-smoke-live-tools.json",
    ]
)
provider_catalog = provider_catalog_candidates[0]
provider_smoke = provider_smoke_candidates[0]
catalog, catalog_error = None, "missing"
for candidate in provider_catalog_candidates:
    catalog, catalog_error = load_json(candidate)
    if catalog_error is None and isinstance(catalog, dict):
        provider_catalog = candidate
        break
smoke, smoke_error = None, "missing"
for candidate in provider_smoke_candidates:
    smoke, smoke_error = load_json(candidate)
    if smoke_error is None and isinstance(smoke, dict):
        provider_smoke = candidate
        break
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
    (
        f"provider live proofs accepted: text={live_text_passed} tools={live_tools_passed} "
        f"catalog={provider_catalog} smoke={provider_smoke}"
    )
    if provider_ok
    else (
        f"text={live_text_passed} tools={live_tools_passed} catalog={provider_catalog} smoke={provider_smoke}"
        if catalog_error is None and smoke_error is None
        else f"catalog={catalog_error or 'present'} smoke={smoke_error or 'present'}"
    )
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
    commands=["bash scripts/provider-live-smoke.sh --required", "bash scripts/stage-commercial-release-proofs.sh"],
    env=[
        "ANTHROPIC_API_KEY",
        "KIANA_OPENAI_API_KEY",
        "OPENAI_API_KEY",
        "KIANA_OLLAMA_BASE_URL",
        "OLLAMA_BASE_URL",
        "KIANA_PROVIDER_CATALOG_PROOF_FILE",
        "KIANA_PROVIDER_SMOKE_PROOF_FILE",
    ],
)

remote_dir = Path(os.environ.get("KIANA_REMOTE_LIVE_SMOKE_DIR", live_root / "remote"))
remote_candidates = unique_paths(
    [
        Path(os.environ["KIANA_REMOTE_SMOKE_PROOF_FILE"])
        if os.environ.get("KIANA_REMOTE_SMOKE_PROOF_FILE")
        else None,
        dist_dir / "proofs/live-smoke/remote/code-session-smoke.json",
        remote_dir / "code-session-smoke.json",
    ]
)
remote_proof = remote_candidates[0]
remote, remote_error = None, "missing"
for candidate in remote_candidates:
    remote, remote_error = load_json(candidate)
    if remote_error is None and isinstance(remote, dict):
        remote_proof = candidate
        break
remote_ok = (
    isinstance(remote, dict)
    and remote.get("schema") == "kiana.remote-code-session-smoke.v1"
    and remote.get("status") == "ok"
    and isinstance(remote.get("session_id"), str)
    and remote["session_id"].startswith("cse_")
    and isinstance(remote.get("api_base_url"), str)
    and remote["api_base_url"].startswith(("http://", "https://"))
    and isinstance(remote.get("sdk_url"), str)
    and remote["sdk_url"].startswith(("http://", "https://"))
    and isinstance(remote.get("expires_in"), int)
    and remote["expires_in"] > 0
    and isinstance(remote.get("worker_epoch"), int)
    and remote["worker_epoch"] >= 0
)
add_check(
    id="live.remote-code-session",
    category="live-service",
    title="Remote code-session live smoke has accepted proof",
    ok=remote_ok,
    external=True,
    gate="scripts/remote-live-smoke.sh --required",
    evidence=(
        f"remote code-session proof accepted: proof={remote_proof}"
        if remote_ok
        else (
            f"proof={remote_proof}"
            if remote_error is None
            else f"proof={remote_error}: {remote_proof}"
        )
    ),
    required_action="Run remote live smoke against the production-like CCR/session service and keep the proof JSON.",
    paths=[remote_proof],
    commands=["bash scripts/remote-live-smoke.sh --required", "bash scripts/stage-commercial-release-proofs.sh"],
    env=[
        "KIANA_REMOTE_ACCESS_TOKEN",
        "CLAUDE_ACCESS_TOKEN",
        "ANTHROPIC_AUTH_TOKEN",
        "KIANA_OAUTH_TOKENS_FILE",
        "KIANA_REMOTE_SMOKE_PROOF_FILE",
    ],
)

product_candidates = [
    Path(value)
    for value in [
        os.environ.get("KIANA_PRODUCT_ACCEPTANCE_FILE", ""),
        os.environ.get("KIANA_PRODUCT_ACCEPTANCE_OUT", ""),
    ]
    if value
] + [
    dist_dir / "proofs/product/product-acceptance.json",
    Path("target/product-acceptance/product-acceptance.json"),
    Path(f"docs/product-acceptance/{VERSION}.json"),
]
product_file = product_candidates[-1]
product = None
product_error = "missing"
for candidate in product_candidates:
    candidate_product, candidate_error = load_json(candidate)
    if candidate_error is None:
        product_file = candidate
        product = candidate_product
        product_error = None
        break
required_workflows = {
    "permission",
    "diff",
    "history",
    "onboarding",
    "resume",
    "settings",
    "app-server",
    "context-search",
    "context-cache-recovery",
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
        f"product acceptance proof accepted: {product_file}"
        if product_ok
        else f"missing workflows: {', '.join(missing_workflows)}"
        if product_error is None and missing_workflows
        else (f"proof={product_file}" if product_error is None else f"proof={product_error}: {product_file}")
    ),
    required_action="Record target-customer acceptance in kiana.product-acceptance.v1 with all required workflows.",
    paths=[product_file],
    commands=["bash scripts/product-acceptance-report.sh full", "bash scripts/stage-commercial-release-proofs.sh"],
    env=["KIANA_PRODUCT_ACCEPTANCE_FILE"],
)

entitlement_candidates = [
    Path(value)
    for value in [
        os.environ.get("KIANA_ENTITLEMENT_PROOF_FILE", ""),
        os.environ.get("KIANA_ENTITLEMENT_PROOF_OUT", ""),
    ]
    if value
] + [
    dist_dir / "proofs/entitlement/entitlement-proof.json",
    Path("target/entitlement-proof/entitlement-proof.json"),
    Path(f"docs/entitlements/{VERSION}.json"),
]
entitlement_file = entitlement_candidates[-1]
entitlement = None
entitlement_error = "missing"
for candidate in entitlement_candidates:
    candidate_entitlement, candidate_error = load_json(candidate)
    if candidate_error is None:
        entitlement_file = candidate
        entitlement = candidate_entitlement
        entitlement_error = None
        break
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
    and valid_fingerprint(entitlement, "license_key_fingerprint")
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
        f"entitlement proof accepted: {entitlement_file}"
        if entitlement_ok
        else f"missing entitlements: {', '.join(missing_entitlements)}"
        if entitlement_error is None and missing_entitlements
        else (f"proof={entitlement_file}" if entitlement_error is None else f"proof={entitlement_error}: {entitlement_file}")
    ),
    required_action="Record the production license/account backend acceptance in kiana.entitlement-proof.v1.",
    paths=[entitlement_file],
    commands=["bash scripts/entitlement-proof-report.sh full", "bash scripts/stage-commercial-release-proofs.sh"],
    env=["KIANA_ENTITLEMENT_PROOF_FILE", "KIANA_REQUIRED_ENTITLEMENTS"],
)

ops_candidates = [
    Path(value)
    for value in [
        os.environ.get("KIANA_RELEASE_OPS_FILE", ""),
        os.environ.get("KIANA_RELEASE_OPS_OUT", ""),
    ]
    if value
] + [
    dist_dir / "proofs/release-ops/release-ops.json",
    Path("target/release-ops/release-ops.json"),
    Path(f"docs/release-ops/{VERSION}.json"),
]
ops_file = ops_candidates[-1]
ops = None
ops_error = "missing"
for candidate in ops_candidates:
    candidate_ops, candidate_error = load_json(candidate)
    if candidate_error is None:
        ops_file = candidate
        ops = candidate_ops
        ops_error = None
        break
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
    evidence=(
        f"release operations proof accepted: {ops_file}"
        if ops_ok
        else (f"proof={ops_file}" if ops_error is None else f"proof={ops_error}: {ops_file}")
    ),
    required_action="Record private vulnerability route, release credential ownership, support contact, retention policy, and credential review in kiana.release-ops.v1.",
    paths=[ops_file],
    commands=["bash scripts/release-ops-report.sh full", "bash scripts/stage-commercial-release-proofs.sh"],
    env=["KIANA_RELEASE_OPS_FILE"],
)

current_platform = platform_id()
expected_isolation_by_platform = {
    "linux": "linux_bwrap",
    "macos": "macos_exec_policy",
    "windows": "windows_exec_policy",
}
expected_isolation = expected_isolation_by_platform.get(current_platform, "unknown")
platform_candidates = []
for value in [
    os.environ.get("KIANA_PLATFORM_SECURITY_PROOF_FILE", ""),
    os.environ.get("KIANA_PLATFORM_SECURITY_PROOF_OUT", ""),
]:
    if value:
        platform_candidates.append(Path(value))
platform_dir_override = os.environ.get("KIANA_PLATFORM_SECURITY_PROOF_DIR", "").strip()
if platform_dir_override:
    platform_candidates.extend(sorted(Path(platform_dir_override).glob(f"{VERSION}-*.json")))
platform_candidates.extend(sorted((dist_dir / "proofs/platform-security").glob("*.json")))
platform_candidates.extend(sorted(Path("target/platform-security").glob("*.json")))
platform_candidates.extend(sorted(Path("docs/platform-security").glob(f"{VERSION}-*.json")))

platform_results = {}
for proof_file in platform_candidates:
    proof, error = load_json(proof_file)
    platform = proof.get("platform") if isinstance(proof, dict) else None
    if platform not in expected_isolation_by_platform or platform in platform_results:
        continue
    expected = expected_isolation_by_platform[platform]
    ok = (
        isinstance(proof, dict)
        and proof.get("schema") == "kiana.platform-security-proof.v1"
        and proof.get("version") == VERSION
        and proof.get("status") == "accepted"
        and proof.get("accepted") is True
        and proof.get("platform") == platform
        and proof.get("isolation") == expected
        and proof.get("doctor_status") == "ready"
        and filled(proof, "accepted_by")
        and filled(proof, "accepted_at")
        and filled(proof, "runner")
        and isinstance(proof.get("controls"), list)
        and len(proof["controls"]) > 0
        and isinstance(proof.get("evidence"), list)
        and len(proof["evidence"]) > 0
    )
    platform_results[platform] = {
        "file": proof_file,
        "ok": ok,
        "error": error,
    }
for platform in ["linux", "macos", "windows"]:
    platform_results.setdefault(
        platform,
        {
            "file": Path(f"docs/platform-security/{VERSION}-{platform}.json"),
            "ok": False,
            "error": "missing",
        },
    )

platform_order = ["linux", "macos", "windows"]
platform_ok = all(platform_results[platform]["ok"] for platform in platform_order)
platform_evidence = ", ".join(
    f"{platform}="
    + ("accepted" if result["ok"] else f"missing_or_invalid:{result['file']}")
    for platform in platform_order
    for result in [platform_results[platform]]
)
platform_paths = [platform_results[platform]["file"] for platform in platform_order]
add_check(
    id="acceptance.platform-security",
    category="acceptance",
    title="Linux, macOS, and Windows platform security proofs are accepted",
    ok=platform_ok,
    external=True,
    gate="scripts/platform-security-proof-report.sh full",
    evidence=(
        f"platform security proofs accepted: {platform_evidence}"
        if platform_ok
        else platform_evidence
    ),
    required_action="Record accepted platform security proofs from the real Linux, macOS, and Windows release runners with ready doctor status and expected isolation.",
    paths=platform_paths,
    commands=["bash scripts/platform-security-proof-report.sh full", "bash scripts/stage-commercial-release-proofs.sh"],
    env=[
        "KIANA_PLATFORM_SECURITY_PROOF_FILE",
        "KIANA_PLATFORM_SECURITY_PROOF_OUT",
        "KIANA_PLATFORM_SECURITY_PROOF_DIR",
    ],
    acceptance_artifacts=[
        f"docs/platform-security/{VERSION}-linux.json",
        f"docs/platform-security/{VERSION}-macos.json",
        f"docs/platform-security/{VERSION}-windows.json",
    ],
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


def md_escape(value):
    return str(value).replace("|", "\\|").replace("\n", " ")


def render_handoff_markdown(report):
    summary = report["summary"]
    lines = [
        f"# Kiana Commercial Release Handoff {report['version']} ({report['release_tag']})",
        "",
        f"Generated: {report['generated_at']}",
        f"Status: {report['status']}",
        (
            "Summary: "
            f"{summary['blocking']} blocking, "
            f"{summary['external_blocking']} external, "
            f"{summary['local_blocking']} local, "
            f"{summary['satisfied']} satisfied"
        ),
        "",
        "## Blocking Assignments",
        "",
    ]
    if not blocking_checks:
        lines.extend(["No blocking checks were detected.", ""])
        return "\n".join(lines).rstrip() + "\n"

    lines.extend(
        [
            "| ID | Owner | Status | Gate | Acceptance Artifacts | Verification |",
            "| --- | --- | --- | --- | --- | --- |",
        ]
    )
    for check in blocking_checks:
        artifacts = "<br>".join(md_escape(item) for item in check["acceptance_artifacts"])
        verification = "<br>".join(md_escape(item) for item in check["verification_commands"])
        lines.append(
            "| "
            + " | ".join(
                [
                    md_escape(check["id"]),
                    md_escape(check["owner"]),
                    md_escape(check["owner_status"]),
                    md_escape(check["gate"]),
                    artifacts or "n/a",
                    verification or "n/a",
                ]
            )
            + " |"
        )

    lines.append("")
    for check in blocking_checks:
        lines.extend(
            [
                f"## {check['id']}",
                "",
                f"- Title: {check['title']}",
                f"- Owner: {check['owner']} ({check['owner_status']})",
                f"- Evidence: {check['evidence']}",
                f"- Required action: {check['required_action']}",
                f"- Gate: {check['gate']}",
            ]
        )
        if check["env"]:
            lines.append("- Environment:")
            for item in check["env"]:
                lines.append(f"  - `{item}`")
        if check["acceptance_artifacts"]:
            lines.append("- Acceptance artifacts:")
            for item in check["acceptance_artifacts"]:
                lines.append(f"  - `{item}`")
        if check["verification_commands"]:
            lines.append("- Verification commands:")
            for item in check["verification_commands"]:
                lines.append(f"  - `{item}`")
        if check["handoff_notes"]:
            lines.append("- Handoff notes:")
            for item in check["handoff_notes"]:
                lines.append(f"  - {item}")
        lines.append("")
    return "\n".join(lines).rstrip() + "\n"


out = os.environ.get("KIANA_COMMERCIAL_BLOCKERS_OUT", "")
if out:
    path = Path(out)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

handoff_out = os.environ.get("KIANA_BLOCKERS_HANDOFF_OUT", "") or os.environ.get(
    "KIANA_COMMERCIAL_HANDOFF_OUT", ""
)
if handoff_out:
    path = Path(handoff_out)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(render_handoff_markdown(report), encoding="utf-8")

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
    if handoff_out:
        print(f"Handoff: {handoff_out}")
    if blocking_checks:
        print("")
        print("Blocking:")
        for check in blocking_checks:
            owner = "external" if check["external"] else "local"
            print(f"- {check['id']} [{owner}]: {check['title']}")
            print(f"  owner: {check['owner']} ({check['owner_status']})")
            print(f"  evidence: {check['evidence']}")
            print(f"  gate: {check['gate']}")
            print(f"  action: {check['required_action']}")
    else:
        print("")
        print("No blocking checks were detected by the lightweight report.")

if os.environ.get("KIANA_BLOCKERS_FAIL_ON_BLOCKERS") == "1" and blocking_checks:
    sys.exit(1)
PY
