#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

version="${VERSION:-$(tr -d '\r\n' < VERSION)}"
dist_dir="${DIST_DIR:-dist}"

case "$(uname -s)" in
  Linux*) os="linux"; exe_ext="" ;;
  Darwin*) os="macos"; exe_ext="" ;;
  MSYS*|MINGW*|CYGWIN*) os="windows"; exe_ext=".exe" ;;
  *) echo "unsupported OS: $(uname -s)" >&2; exit 1 ;;
esac

case "$(uname -m)" in
  x86_64|amd64) arch="x86_64" ;;
  aarch64|arm64) arch="aarch64" ;;
  *) echo "unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac

target="${KIANA_PACKAGE_TARGET:-${os}-${arch}}"
package="kiana-${version}-${target}"
stage="${dist_dir}/${package}"
archive="${dist_dir}/${package}.tar.gz"
portable_zip="${dist_dir}/${package}.zip"
compliance_dir="${dist_dir}/compliance-${package}"
compliance_mode="${KIANA_PACKAGE_COMPLIANCE_MODE:---local-rc}"

if [[ -e "$stage" || -e "$archive" || -e "$portable_zip" ]]; then
  echo "package output already exists: $stage, $archive, or $portable_zip" >&2
  echo "set DIST_DIR to a fresh directory or remove the previous package output" >&2
  exit 1
fi

cargo build --release --locked --offline -p kiana-entrypoints --bin kiana

mkdir -p "$stage/docs/schemas" "$stage/scripts"
cp "target/release/kiana${exe_ext}" "$stage/"
cp VERSION README.md RELEASE.md INSTALL.md CONFIG.md USAGE.md CHANGELOG.md UPGRADE.md SECURITY.md PRIVACY.md TELEMETRY.md "$stage/"
cp LICENSE-MIT LICENSE-APACHE "$stage/"
cp docs/reference-migration-roadmap.md docs/commercial-release-readiness.md docs/release-checklist.md docs/distribution-channels.md "$stage/docs/"
cp docs/schemas/*.json "$stage/docs/schemas/"
cp scripts/install-release-binary.sh scripts/package-lifecycle-smoke.sh scripts/product-shell-smoke.sh scripts/product-acceptance-report.sh scripts/provider-live-smoke.sh scripts/remote-live-smoke.sh scripts/sign-release-artifacts.sh scripts/verify-commercial-release-artifacts.sh "$stage/scripts/"

COMPLIANCE_OUT_DIR="$compliance_dir" bash scripts/compliance-audit.sh "$compliance_mode" >/dev/null
cp "$compliance_dir/sbom.cdx.json" "$stage/SBOM.cdx.json"
cp "$compliance_dir/compliance-report.json" "$stage/docs/compliance-report.json"

(
  cd "$dist_dir"
  tar -czf "${package}.tar.gz" "$package"
  if [[ "$target" == windows-* ]]; then
    python_bin="$(command -v python3 2>/dev/null || command -v python 2>/dev/null || true)"
    if [[ -z "$python_bin" ]]; then
      echo "python3 or python is required to create the Windows portable ZIP" >&2
      exit 1
    fi
    "$python_bin" - "$package" "${package}.zip" <<'PY'
import os
import sys
import zipfile

root, output = sys.argv[1], sys.argv[2]
with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as archive:
    for current, _, files in os.walk(root):
        for name in sorted(files):
            path = os.path.join(current, name)
            archive.write(path, os.path.relpath(path, "."))
PY
  fi
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "${package}.tar.gz" > "${package}.tar.gz.sha256"
    sha256sum "${package}/kiana${exe_ext}" > "${package}.binary.sha256"
    if [[ -f "${package}.zip" ]]; then
      sha256sum "${package}.zip" > "${package}.zip.sha256"
    fi
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "${package}.tar.gz" > "${package}.tar.gz.sha256"
    shasum -a 256 "${package}/kiana${exe_ext}" > "${package}.binary.sha256"
    if [[ -f "${package}.zip" ]]; then
      shasum -a 256 "${package}.zip" > "${package}.zip.sha256"
    fi
  else
    echo "warning: sha256 tool not found; checksum files were not created" >&2
  fi
)

bash scripts/generate-distribution-manifests.sh

echo "Created ${archive}"
if [[ -f "$portable_zip" ]]; then
  echo "Created ${portable_zip}"
fi
