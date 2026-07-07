#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

version="${VERSION:-$(tr -d '\r\n' < VERSION)}"
dist_dir="${DIST_DIR:-dist}"
archive="${PACKAGE_ARCHIVE:-}"

case "$(uname -s)" in
  MSYS*|MINGW*|CYGWIN*) exe_ext=".exe" ;;
  *) exe_ext="" ;;
esac

if [[ -z "$archive" ]]; then
  shopt -s nullglob
  matches=("$dist_dir"/kiana-"$version"-*.tar.gz)
  shopt -u nullglob
  if [[ "${#matches[@]}" -ne 1 ]]; then
    echo "expected exactly one kiana-${version}-*.tar.gz in $dist_dir; found ${#matches[@]}" >&2
    echo "run DIST_DIR=\"$dist_dir\" scripts/package-release.sh first, or set PACKAGE_ARCHIVE" >&2
    exit 2
  fi
  archive="${matches[0]}"
fi

if [[ ! -f "$archive" ]]; then
  echo "package archive not found: $archive" >&2
  exit 2
fi

archive_dir="$(cd "$(dirname "$archive")" && pwd)"
archive_name="$(basename "$archive")"
package_name="${archive_name%.tar.gz}"
archive_sha="$archive_dir/${archive_name}.sha256"
binary_sha="$archive_dir/${package_name}.binary.sha256"

verify_checksum_file() {
  local checksum_file="$1"
  if [[ ! -f "$checksum_file" ]]; then
    echo "checksum file not found: $checksum_file" >&2
    exit 1
  fi
  if command -v sha256sum >/dev/null 2>&1; then
    (cd "$archive_dir" && sha256sum -c "$(basename "$checksum_file")" >/dev/null)
  elif command -v shasum >/dev/null 2>&1; then
    local expected target actual
    read -r expected target < "$checksum_file"
    actual="$(shasum -a 256 "$archive_dir/$target" | awk '{print $1}')"
    [[ "$actual" == "$expected" ]]
  else
    echo "no sha256sum or shasum available" >&2
    exit 1
  fi
}

file_hash() {
  local file="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$file" | awk '{print $1}'
  else
    shasum -a 256 "$file" | awk '{print $1}'
  fi
}

native_env_path() {
  local path="$1"
  if command -v cygpath >/dev/null 2>&1; then
    cygpath -w "$path"
  else
    printf '%s\n' "$path"
  fi
}

tmp_root="$(mktemp -d)"
tmp_root="$(cd "$tmp_root" && pwd)"
touch "$tmp_root/.kiana-package-lifecycle-smoke"
cleanup() {
  if [[ -n "${tmp_root:-}" && -f "$tmp_root/.kiana-package-lifecycle-smoke" ]]; then
    rm -rf "$tmp_root"
  fi
}
trap cleanup EXIT

verify_checksum_file "$archive_sha"
verify_checksum_file "$binary_sha"

extract_dir="$tmp_root/extract"
mkdir -p "$extract_dir"
tar -xzf "$archive" -C "$extract_dir"

package_root="$extract_dir/$package_name"
if [[ ! -d "$package_root" ]]; then
  echo "expected package root not found after extraction: $package_root" >&2
  exit 1
fi

for file in \
  "$package_root/kiana${exe_ext}" \
  "$package_root/SBOM.cdx.json" \
  "$package_root/docs/compliance-report.json" \
  "$package_root/docs/proof-templates/README.md" \
  "$package_root/docs/proof-templates/product-acceptance.example.json" \
  "$package_root/docs/proof-templates/entitlement-proof.example.json" \
  "$package_root/docs/proof-templates/release-ops.example.json" \
  "$package_root/docs/proof-templates/platform-security.example.json" \
  "$package_root/docs/sdk-runtime-events.md" \
  "$package_root/docs/schemas/kiana-app-server-conversations.v1.schema.json" \
  "$package_root/docs/schemas/kiana-app-server-config-resolved.v1.schema.json" \
  "$package_root/docs/schemas/kiana-app-server-contract.v1.schema.json" \
  "$package_root/docs/schemas/kiana-app-server-events.v1.schema.json" \
  "$package_root/docs/schemas/kiana-app-server-git-status.v1.schema.json" \
  "$package_root/docs/schemas/kiana-app-server-distribution-review.v1.schema.json" \
  "$package_root/docs/schemas/kiana-app-server-model-current.v1.schema.json" \
  "$package_root/docs/schemas/kiana-app-server-permissions-status.v1.schema.json" \
  "$package_root/docs/schemas/kiana-app-server-plugins.v1.schema.json" \
  "$package_root/docs/schemas/kiana-app-server-sandbox.v1.schema.json" \
  "$package_root/docs/schemas/kiana-app-server-secrets.v1.schema.json" \
  "$package_root/docs/schemas/kiana-app-server-settings.v1.schema.json" \
  "$package_root/docs/schemas/kiana-app-server-prompt-history.v1.schema.json" \
  "$package_root/docs/schemas/kiana-app-server-trust-status.v1.schema.json" \
  "$package_root/docs/schemas/kiana-app-server-commands.v1.schema.json" \
  "$package_root/docs/schemas/kiana-context-artifact-dependency-graph.v1.schema.json" \
  "$package_root/docs/schemas/kiana-context-artifact-readiness.v1.schema.json" \
  "$package_root/docs/schemas/kiana-context-artifact-store.v1.schema.json" \
  "$package_root/docs/schemas/kiana-context-index.v1.schema.json" \
  "$package_root/docs/schemas/kiana-context-search.v1.schema.json" \
  "$package_root/docs/schemas/kiana-context-pack.v1.schema.json" \
  "$package_root/docs/schemas/kiana-commercial-proof-manifest.v1.schema.json" \
  "$package_root/docs/schemas/kiana-commercial-release-blockers.v1.schema.json" \
  "$package_root/docs/schemas/kiana-local-rc-evidence.v1.schema.json" \
  "$package_root/docs/schemas/kiana-runtime-event.v1.schema.json" \
  "$package_root/docs/schemas/kiana-enterprise-offline-manifest.v1.schema.json" \
  "$package_root/docs/schemas/kiana-entitlement-proof.v1.schema.json" \
  "$package_root/docs/schemas/kiana-license-status.v1.schema.json" \
  "$package_root/docs/schemas/kiana-managed-plugin-policy.v1.schema.json" \
  "$package_root/docs/schemas/kiana-model-list.v1.schema.json" \
  "$package_root/docs/schemas/kiana-model-smoke.v1.schema.json" \
  "$package_root/docs/schemas/kiana-macos-notarization.v1.schema.json" \
  "$package_root/docs/schemas/kiana-plugin-install-receipt.v1.schema.json" \
  "$package_root/docs/schemas/kiana-product-acceptance.v1.schema.json" \
  "$package_root/docs/schemas/kiana-platform-security-proof.v1.schema.json" \
  "$package_root/docs/schemas/kiana-release-signature.v1.schema.json" \
  "$package_root/docs/schemas/kiana-remote-code-session-smoke.v1.schema.json" \
  "$package_root/docs/schemas/kiana-release-ops.v1.schema.json" \
  "$package_root/scripts/install-release-binary.sh" \
  "$package_root/scripts/validate-json-schema.py" \
  "$package_root/scripts/schema-contract-smoke.sh" \
  "$package_root/scripts/commercial-release-blockers-report.sh" \
  "$package_root/scripts/source-control-proof-report.sh" \
  "$package_root/scripts/distribution-review-report.sh" \
  "$package_root/scripts/local-rc-evidence-report.sh" \
  "$package_root/scripts/commercial-release-handoff-smoke.sh" \
  "$package_root/scripts/stage-commercial-release-proofs.sh" \
  "$package_root/scripts/entitlement-proof-report.sh" \
  "$package_root/scripts/product-acceptance-report.sh" \
  "$package_root/scripts/release-ops-report.sh" \
  "$package_root/scripts/platform-security-proof-report.sh" \
  "$package_root/scripts/provider-live-smoke.sh" \
  "$package_root/scripts/remote-live-smoke.sh" \
  "$package_root/scripts/sign-release-artifacts.sh" \
  "$package_root/scripts/verify-commercial-release-artifacts.sh" \
  "$package_root/scripts/release-signature-verification-smoke.sh"
do
  if [[ ! -f "$file" ]]; then
    echo "package missing required file: $file" >&2
    exit 1
  fi
done

for schema in docs/schemas/*.json; do
  packaged_schema="$package_root/docs/schemas/$(basename "$schema")"
  if [[ ! -f "$packaged_schema" ]]; then
    echo "package schema file missing: $packaged_schema" >&2
    exit 1
  fi
  if ! cmp -s "$schema" "$packaged_schema"; then
    echo "package schema file differs from source: $packaged_schema" >&2
    exit 1
  fi
done

package_python_bin="$(command -v python3 2>/dev/null || command -v python 2>/dev/null || true)"
if [[ -z "$package_python_bin" ]]; then
  echo "python3 or python is required to validate packaged JSON schema contracts" >&2
  exit 1
fi
bash "$package_root/scripts/schema-contract-smoke.sh" >/dev/null

install_dir="$tmp_root/install"
home_dir="$tmp_root/home"
mkdir -p "$install_dir" "$home_dir/.kiana"
installed="$install_dir/kiana${exe_ext}"
installer="$package_root/scripts/install-release-binary.sh"
home_env="$(native_env_path "$home_dir")"
kiana_home_env="$(native_env_path "$home_dir/.kiana")"
kiana_config_env="$(native_env_path "$home_dir/.kiana/config.toml")"

run_installed() {
  HOME="$home_env" \
  KIANA_HOME="$kiana_home_env" \
  KIANA_CONFIG_FILE="$kiana_config_env" \
  env \
    -u ANTHROPIC_AUTH_TOKEN \
    -u ANTHROPIC_API_KEY \
    -u ANTHROPIC_BASE_URL \
    -u ANTHROPIC_MODEL \
    -u KIANA_PROVIDER_SMOKE_LIVE \
    -u KIANA_LICENSE_FILE \
    -u KIANA_LICENSE_KEY \
    -u KIANA_LICENSE_PLAN \
    -u KIANA_LICENSE_ENTITLEMENTS \
    -u KIANA_LICENSE_OFFLINE \
    -u KIANA_ENTERPRISE_ACCOUNT_ID \
    -u KIANA_SUPPORT_CONTACT \
    -u KIANA_PROVIDER \
    -u KIANA_OPENAI_API_KEY \
    -u OPENAI_API_KEY \
    -u KIANA_REMOTE_ACCESS_TOKEN \
    -u CLAUDE_ACCESS_TOKEN \
    "$installed" "$@"
}

INSTALL_DIR="$install_dir" bash "$installer" --install >/dev/null
[[ -x "$installed" ]]
run_installed --version >/dev/null
run_installed doctor --json | grep -Fq '"schema": "kiana.doctor.v1"'
run_installed doctor --json | grep -Fq '"reference_capabilities"'
run_installed license status --json | grep -Fq '"schema": "kiana.license-status.v1"'
run_installed license status --json | grep -Fq '"status": "missing"'
run_installed model list --json | grep -Fq '"provider_id": "openai-compatible"'
run_installed model smoke --json | grep -Fq '"schema": "kiana.model-smoke.v1"'
run_installed model smoke --json | grep -Fq '"tools": false'
run_installed model smoke --json | grep -Fq '"provider_id": "fake"'
run_installed model smoke --tools --json | grep -Fq '"tools": true'
run_installed model smoke --tools --json | grep -Fq '"capability": "tools"'
context_fixture="$tmp_root/context-fixture"
context_artifact_root="$tmp_root/context-artifact-root"
mkdir -p "$context_fixture/src"
mkdir -p "$context_fixture/docs"
mkdir -p "$context_fixture/tests"
mkdir -p "$context_artifact_root/bundle"
printf '%s\n' 'pub fn lifecycle_search() {}' '// lifecycle lifecycle search' > "$context_fixture/src/lib.rs"
printf '%s\n' 'use kiana::lifecycle_search;' > "$context_fixture/tests/lib_test.rs"
printf '%s\n' 'first module summary referencing src/lib.rs' > "$context_fixture/docs/path-only.md"
printf '%s\n' 'first artifact line' > "$context_artifact_root/bundle/notes.md"
(
  cd "$context_fixture"
  run_installed context index --json | grep -Fq '"schema": "kiana.context-index.v1"'
  run_installed context index --json | grep -Fq '"path": "src/lib.rs"'
  run_installed context artifacts --json | grep -Fq '"schema": "kiana.context-artifacts.v1"'
  run_installed context artifacts --json | grep -Fq '"kind": "file"'
  run_installed context artifacts --json | grep -Fq '"path": "src/lib.rs"'
  run_installed context artifacts --json --cache .kiana/context-artifacts.json | grep -Fq '"status": "created"'
  run_installed context artifacts --json --cache .kiana/context-artifacts.json | grep -Fq '"reused_artifacts": 3'
  test -f .kiana/context-artifacts.json
  run_installed context artifact-graph --json | grep -Fq '"schema": "kiana.context-artifact-dependency-graph.v1"'
  run_installed context artifact-graph --json | grep -Fq '"relation": "test_of"'
  run_installed context artifact-graph --json | grep -Fq '"evidence": "tests/lib_test.rs matches src/lib.rs"'
  run_installed context artifact-graph --json | grep -Fq '"relation": "path_reference"'
  run_installed context artifact-graph --json | grep -Fq '"evidence": "docs/path-only.md references src/lib.rs"'
  run_installed context artifact-store --json | grep -Fq '"schema": "kiana.context-artifact-store.v1"'
  run_installed context artifact-store --json | grep -Fq '"artifact_count": 3'
  run_installed context artifact-store --json | grep -Fq '"dependency_count": 2'
  run_installed context artifact-store --json | grep -Fq '"role": "source"'
  run_installed context artifact-store --json | grep -Fq '"role": "test"'
  run_installed context artifact-store --json | grep -Fq '"dependency_graph_schema": "kiana.context-artifact-dependency-graph.v1"'
  run_installed context artifact-readiness --json | grep -Fq '"schema": "kiana.context-artifact-readiness.v1"'
  run_installed context artifact-readiness --json | grep -Fq '"status": "incomplete"'
  run_installed context artifact-readiness --json | grep -Fq '"missing_roles"'
  run_installed context artifact-store --json --cache .kiana/context-artifact-store.json | grep -Fq '"status": "created"'
  run_installed context artifact-store --json --cache .kiana/context-artifact-store.json | grep -Fq '"reused_artifacts": 3'
  run_installed context artifact-store --json --cache .kiana/context-artifact-store.json | grep -Fq '"reused_dependencies": 2'
  test -f .kiana/context-artifact-store.json
  run_installed context search lifecycle --json --limit 1 | grep -Fq '"schema": "kiana.context-search.v1"'
  run_installed context search lifecycle --json --limit 1 | grep -Fq '"path": "src/lib.rs"'
  run_installed context search docs/path-only.md --json --limit 1 | grep -Fq '"path": "docs/path-only.md"'
  run_installed context search docs/path-only.md --json --limit 1 | grep -Fq '"occurrences": 0'
  run_installed context pack lifecycle --json --limit 1 --max-snippet-lines 1 | grep -Fq '"schema": "kiana.context-pack.v1"'
  run_installed context pack lifecycle --json --limit 1 --max-snippet-lines 1 | grep -Fq '"path": "src/lib.rs"'
  run_installed context pack lifecycle --json --limit 1 --max-snippet-lines 1 | grep -Fq '"schema": "kiana.context-artifact-graph.v1"'
  run_installed context pack lifecycle --json --limit 1 --max-snippet-lines 1 | grep -Fq '"relation": "matched"'
  run_installed context pack docs/path-only.md --json --limit 1 --max-snippet-lines 1 | grep -Fq '"excerpt": "first module summary referencing src/lib.rs"'
  run_installed context pack docs/path-only.md --json --limit 1 --max-snippet-lines 1 | grep -Fq '"schema": "kiana.context-artifact-graph.v1"'
  run_installed context pack bundle/notes.md --root "$context_artifact_root" --json --limit 1 --max-snippet-lines 1 | grep -Fq '"path": "bundle/notes.md"'
  run_installed context pack bundle/notes.md --root "$context_artifact_root" --json --limit 1 --max-snippet-lines 1 | grep -Fq '"excerpt": "first artifact line"'
  run_installed context pack bundle/notes.md --root "$context_artifact_root" --json --limit 1 --max-snippet-lines 1 | grep -Fq '"schema": "kiana.context-artifact-graph.v1"'
)

offline_manifest="$archive_dir/manifests/enterprise/offline-manifest.json"
if [[ -f "$offline_manifest" ]]; then
  python_bin="$(command -v python3 2>/dev/null || command -v python 2>/dev/null || true)"
  if [[ -z "$python_bin" ]]; then
    echo "python3 or python is required to validate $offline_manifest" >&2
    exit 1
  fi
  OFFLINE_MANIFEST="$offline_manifest" \
  PACKAGE_ARCHIVE_NAME="$archive_name" \
  PACKAGE_ARCHIVE_SHA="$(awk 'NF { print $1; exit }' "$archive_sha")" \
  PACKAGE_VERSION="$version" \
  "$python_bin" - <<'PY'
import json
import os
import sys

with open(os.environ["OFFLINE_MANIFEST"], "r", encoding="utf-8") as handle:
    manifest = json.load(handle)

archive_name = os.environ["PACKAGE_ARCHIVE_NAME"]
archive_sha = os.environ["PACKAGE_ARCHIVE_SHA"]
version = os.environ["PACKAGE_VERSION"]
artifacts = manifest.get("artifacts")
artifact = None
if isinstance(artifacts, list):
    artifact = next((item for item in artifacts if item.get("archive") == archive_name), None)

checks = [
    manifest.get("schema") == "kiana.enterprise.offline-manifest.v1",
    manifest.get("version") == version,
    isinstance(manifest.get("release_base_url"), str) and manifest["release_base_url"],
    artifact is not None,
    artifact is not None and artifact.get("sha256") == archive_sha,
    artifact is not None and archive_name in artifact.get("url", ""),
    artifact is not None and artifact.get("local_path") == archive_name,
    artifact is not None and artifact.get("checksum_path") == f"{archive_name}.sha256",
    artifact is not None and artifact.get("binary_checksum_path") == f"{archive_name[:-7]}.binary.sha256",
    manifest.get("generated_by") == "scripts/generate-distribution-manifests.sh",
    isinstance(manifest.get("channels"), dict),
]
if not all(checks):
    print("enterprise offline manifest failed package lifecycle checks", file=sys.stderr)
    print(json.dumps(manifest, indent=2, sort_keys=True), file=sys.stderr)
    sys.exit(1)
PY
fi

pre_upgrade_hash="$(file_hash "$installed")"
rollback_dir="$tmp_root/rollback"
mkdir -p "$rollback_dir"
cp "$installed" "$rollback_dir/kiana${exe_ext}"

INSTALL_DIR="$install_dir" bash "$installer" --install >/dev/null
post_upgrade_hash="$(file_hash "$installed")"
if [[ "$post_upgrade_hash" != "$pre_upgrade_hash" ]]; then
  echo "same-package upgrade changed binary checksum unexpectedly" >&2
  exit 1
fi

cp "$rollback_dir/kiana${exe_ext}" "$installed"
chmod +x "$installed"
rollback_hash="$(file_hash "$installed")"
if [[ "$rollback_hash" != "$pre_upgrade_hash" ]]; then
  echo "rollback restore did not match pre-upgrade checksum" >&2
  exit 1
fi
run_installed --version >/dev/null

INSTALL_DIR="$install_dir" bash "$installer" --uninstall >/dev/null
if [[ -e "$installed" ]]; then
  echo "uninstall left binary behind: $installed" >&2
  exit 1
fi
INSTALL_DIR="$install_dir" bash "$installer" --uninstall >/dev/null

echo "package lifecycle smoke passed for $archive_name"
