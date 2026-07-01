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
  "$package_root/docs/schemas/kiana-model-smoke.v1.schema.json" \
  "$package_root/scripts/install-release-binary.sh"
do
  if [[ ! -f "$file" ]]; then
    echo "package missing required file: $file" >&2
    exit 1
  fi
done

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
run_installed model list --json | grep -Fq '"provider_id": "openai-compatible"'
run_installed model smoke --json | grep -Fq '"schema": "kiana.model-smoke.v1"'
run_installed model smoke --json | grep -Fq '"provider_id": "fake"'

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
