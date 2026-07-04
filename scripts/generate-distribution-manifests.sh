#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

version="${VERSION:-$(tr -d '\r\n' < VERSION)}"
dist_dir="${DIST_DIR:-dist}"
manifest_dir="${MANIFEST_DIR:-${dist_dir}/manifests}"
release_base_url="${KIANA_RELEASE_BASE_URL:-https://github.com/kiana-project/kiana/releases/download/v${version}}"

mkdir -p "$manifest_dir/homebrew" "$manifest_dir/winget" "$manifest_dir/enterprise"
rm -f "$manifest_dir/homebrew/BLOCKED.md" "$manifest_dir/winget/BLOCKED.md"

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
winget_written=0
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
    zip_file="${dist_dir}/${package}.zip"
    if [[ -f "$zip_file" ]]; then
      zip_filename="$(basename "$zip_file")"
      zip_sha="$(read_sha256 "${zip_file}.sha256")"
      winget_arch="neutral"
      case "$target" in
        *-x86_64) winget_arch="x64" ;;
        *-aarch64) winget_arch="arm64" ;;
      esac
      winget_package_id="${KIANA_WINGET_PACKAGE_IDENTIFIER:-Kiana.Kiana}"
      winget_manifest_version="${KIANA_WINGET_MANIFEST_VERSION:-1.6.0}"
      winget_dir="${manifest_dir}/winget/${winget_package_id}/${version}"
      zip_url="${release_base_url}/${zip_filename}"
      mkdir -p "$winget_dir"
      cat > "${winget_dir}/${winget_package_id}.yaml" <<EOF
PackageIdentifier: ${winget_package_id}
PackageVersion: ${version}
DefaultLocale: en-US
ManifestType: version
ManifestVersion: ${winget_manifest_version}
EOF
      cat > "${winget_dir}/${winget_package_id}.locale.en-US.yaml" <<EOF
PackageIdentifier: ${winget_package_id}
PackageVersion: ${version}
PackageLocale: en-US
Publisher: Kiana Project
PackageName: Kiana Code
License: MIT OR Apache-2.0
ShortDescription: Kiana Code AI coding assistant
ManifestType: defaultLocale
ManifestVersion: ${winget_manifest_version}
EOF
      cat > "${winget_dir}/${winget_package_id}.installer.yaml" <<EOF
PackageIdentifier: ${winget_package_id}
PackageVersion: ${version}
InstallerType: zip
NestedInstallerType: portable
Installers:
- Architecture: ${winget_arch}
  InstallerUrl: ${zip_url}
  InstallerSha256: ${zip_sha}
  NestedInstallerFiles:
  - RelativeFilePath: ${package}/kiana.exe
    PortableCommandAlias: kiana
ManifestType: installer
ManifestVersion: ${winget_manifest_version}
EOF
      winget_written=1
    else
      winget_blockers+=("${filename}")
    fi
  fi

  enterprise_targets+=(
    "{\"target\":\"$(json_escape "$target")\",\"archive\":\"$(json_escape "$filename")\",\"url\":\"$(json_escape "$url")\",\"sha256\":\"$(json_escape "$archive_sha")\",\"binary_sha256\":\"$(json_escape "$binary_sha")\",\"local_path\":\"$(json_escape "$filename")\",\"checksum_path\":\"$(json_escape "${filename}.sha256")\",\"binary_checksum_path\":\"$(json_escape "${package}.binary.sha256")\"}"
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
elif (( winget_written == 0 )); then
  cat > "$manifest_dir/winget/BLOCKED.md" <<EOF
# winget Manifest Blocked

No Windows artifact was present in ${dist_dir}.
Run the release package workflow on a Windows runner before generating winget metadata.
EOF
fi

homebrew_channel="blocked_no_unix_artifacts"
if (( homebrew_written == 1 )); then
  homebrew_channel="generated"
fi
winget_channel="blocked_no_windows_publishable_artifact"
if (( winget_written == 1 && ${#winget_blockers[@]} == 0 )); then
  winget_channel="generated"
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
  echo "    \"github_releases\": \"generated_from_release_base_url\","
  echo "    \"homebrew\": \"${homebrew_channel}\","
  echo "    \"winget\": \"${winget_channel}\""
  echo "  },"
  echo "  \"generated_by\": \"scripts/generate-distribution-manifests.sh\""
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
