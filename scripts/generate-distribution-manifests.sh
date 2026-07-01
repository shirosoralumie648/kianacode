#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

version="${VERSION:-$(tr -d '\r\n' < VERSION)}"
dist_dir="${DIST_DIR:-dist}"
manifest_dir="${MANIFEST_DIR:-${dist_dir}/manifests}"
release_base_url="${KIANA_RELEASE_BASE_URL:-https://github.com/kiana-project/kiana/releases/download/v${version}}"

mkdir -p "$manifest_dir/homebrew" "$manifest_dir/winget" "$manifest_dir/enterprise"

shopt -s nullglob
archives=("${dist_dir}/kiana-${version}-"*.tar.gz)
if (( ${#archives[@]} == 0 )); then
  echo "no release archives found in ${dist_dir}; run scripts/package-release.sh first" >&2
  exit 1
fi

json_escape() {
  local value="$1"
  value="${value//\\/\\\\}"
  value="${value//\"/\\\"}"
  value="${value//$'\n'/\\n}"
  printf '%s' "$value"
}

read_sha256() {
  local checksum_file="$1"
  if [[ ! -f "$checksum_file" ]]; then
    echo "missing checksum file: $checksum_file" >&2
    exit 1
  fi
  awk 'NF { print $1; exit }' "$checksum_file"
}

homebrew_written=0
enterprise_targets=()
winget_blockers=()

for archive in "${archives[@]}"; do
  filename="$(basename "$archive")"
  package="${filename%.tar.gz}"
  target="${package#kiana-${version}-}"
  url="${release_base_url}/${filename}"
  archive_sha="$(read_sha256 "${archive}.sha256")"
  binary_sha_file="${dist_dir}/${package}.binary.sha256"
  binary_sha=""
  if [[ -f "$binary_sha_file" ]]; then
    binary_sha="$(read_sha256 "$binary_sha_file")"
  fi

  case "$target" in
    macos-*|linux-*)
      formula_name="${manifest_dir}/homebrew/kiana-${target}.rb"
      cat > "$formula_name" <<FORMULA
class Kiana < Formula
  desc "Kiana Code AI coding assistant"
  homepage "https://github.com/kiana-project/kiana"
  url "${url}"
  sha256 "${archive_sha}"
  version "${version}"
  license any_of: ["MIT", "Apache-2.0"]

  def install
    bin.install "kiana"
  end

  test do
    assert_match "Kiana Code", shell_output("#{bin}/kiana --version")
  end
end
FORMULA
      homebrew_written=1
      ;;
  esac

  if [[ "$target" == windows-* ]]; then
    winget_blockers+=("${filename}")
  fi

  enterprise_targets+=(
    "{\"target\":\"$(json_escape "$target")\",\"archive\":\"$(json_escape "$filename")\",\"url\":\"$(json_escape "$url")\",\"sha256\":\"$(json_escape "$archive_sha")\",\"binary_sha256\":\"$(json_escape "$binary_sha")\"}"
  )
done

if (( homebrew_written == 0 )); then
  cat > "$manifest_dir/homebrew/BLOCKED.md" <<EOF
# Homebrew Manifest Blocked

No macOS or Linux tarball was present in ${dist_dir}.
Run the release package workflow on macOS and Linux runners before publishing a Homebrew formula.
EOF
fi

if (( ${#winget_blockers[@]} > 0 )); then
  {
    echo "# winget Manifest Blocked"
    echo
    echo "Current Windows artifacts are tar.gz archives:"
    for artifact in "${winget_blockers[@]}"; do
      echo "- ${artifact}"
    done
    echo
    echo "Generate a winget-supported installer or portable ZIP/MSI/EXE before submitting a winget manifest."
  } > "$manifest_dir/winget/BLOCKED.md"
else
  cat > "$manifest_dir/winget/BLOCKED.md" <<EOF
# winget Manifest Blocked

No Windows artifact was present in ${dist_dir}.
Run the release package workflow on a Windows runner before generating winget metadata.
EOF
fi

{
  echo "{"
  echo "  \"schema\": \"kiana.enterprise.offline-manifest.v1\","
  echo "  \"version\": \"$(json_escape "$version")\","
  echo "  \"release_base_url\": \"$(json_escape "$release_base_url")\","
  echo "  \"artifacts\": ["
  for index in "${!enterprise_targets[@]}"; do
    suffix=","
    if (( index == ${#enterprise_targets[@]} - 1 )); then
      suffix=""
    fi
    echo "    ${enterprise_targets[$index]}${suffix}"
  done
  echo "  ],"
  echo "  \"channels\": {"
  echo "    \"github_releases\": \"pending_remote_release\","
  echo "    \"homebrew\": \"dry_run_or_blocked\","
  echo "    \"winget\": \"blocked_until_supported_windows_installer\""
  echo "  }"
  echo "}"
} > "$manifest_dir/enterprise/offline-manifest.json"

cat > "$manifest_dir/summary.txt" <<EOF
Distribution manifests generated
version: ${version}
release_base_url: ${release_base_url}
manifest_dir: ${manifest_dir}
artifacts: ${#archives[@]}
EOF

echo "Distribution manifests written to ${manifest_dir}"
