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

rust_host="$(rustc -vV | awk '/^host:/ { print $2; exit }')"
case "$rust_host" in
  x86_64-*-linux-*) rust_os="linux"; rust_arch="x86_64"; rust_exe_ext="" ;;
  aarch64-*-linux-*) rust_os="linux"; rust_arch="aarch64"; rust_exe_ext="" ;;
  x86_64-apple-darwin*) rust_os="macos"; rust_arch="x86_64"; rust_exe_ext="" ;;
  aarch64-apple-darwin*) rust_os="macos"; rust_arch="aarch64"; rust_exe_ext="" ;;
  x86_64-*-windows-*) rust_os="windows"; rust_arch="x86_64"; rust_exe_ext=".exe" ;;
  aarch64-*-windows-*) rust_os="windows"; rust_arch="aarch64"; rust_exe_ext=".exe" ;;
  *) echo "unsupported rustc host target: ${rust_host:-unknown}" >&2; exit 1 ;;
esac

host_package_target="${os}-${arch}"
rust_package_target="${rust_os}-${rust_arch}"
if [[ "$rust_package_target" != "$host_package_target" || "$rust_exe_ext" != "$exe_ext" ]]; then
  echo "rustc host target $rust_host does not match host package target $host_package_target" >&2
  exit 1
fi

target="${KIANA_PACKAGE_TARGET:-$rust_package_target}"
if [[ "$target" != "$rust_package_target" ]]; then
  echo "KIANA_PACKAGE_TARGET=$target does not match host package target $rust_package_target" >&2
  echo "cross-platform packages must be built on their matching target runner" >&2
  exit 1
fi
package="kiana-${version}-${target}"
stage="${dist_dir}/${package}"
archive="${dist_dir}/${package}.tar.gz"
portable_zip="${dist_dir}/${package}.zip"
compliance_dir="${dist_dir}/compliance-${package}"
compliance_mode="${KIANA_PACKAGE_COMPLIANCE_MODE:---local-rc}"

verify_binary_format() {
  local binary="$1"
  local python_bin
  python_bin="$(command -v python3 2>/dev/null || command -v python 2>/dev/null || true)"
  if [[ -z "$python_bin" ]]; then
    echo "python3 or python is required to verify release binary format" >&2
    exit 1
  fi
  "$python_bin" - "$binary" "$os" "$arch" <<'PY'
import struct
import sys
from pathlib import Path

path = Path(sys.argv[1])
expected_os = sys.argv[2]
expected_arch = sys.argv[3]
data = path.read_bytes()

def reject(message):
    print(f"release binary format mismatch: {path}: {message}", file=sys.stderr)
    raise SystemExit(1)

if expected_os == "linux":
    if len(data) < 20 or data[:4] != b"\x7fELF" or data[4] != 2:
        reject("expected a 64-bit ELF binary")
    endian = "<" if data[5] == 1 else ">" if data[5] == 2 else None
    if endian is None:
        reject("invalid ELF byte order")
    machine = struct.unpack_from(f"{endian}H", data, 18)[0]
    expected = {"x86_64": 62, "aarch64": 183}[expected_arch]
elif expected_os == "macos":
    if len(data) < 8 or data[:4] not in {b"\xcf\xfa\xed\xfe", b"\xfe\xed\xfa\xcf"}:
        reject("expected a 64-bit Mach-O binary")
    endian = "<" if data[:4] == b"\xcf\xfa\xed\xfe" else ">"
    machine = struct.unpack_from(f"{endian}I", data, 4)[0]
    expected = {"x86_64": 0x01000007, "aarch64": 0x0100000C}[expected_arch]
elif expected_os == "windows":
    if len(data) < 64 or data[:2] != b"MZ":
        reject("expected a PE binary")
    pe_offset = struct.unpack_from("<I", data, 0x3C)[0]
    if pe_offset + 6 > len(data) or data[pe_offset:pe_offset + 4] != b"PE\0\0":
        reject("invalid PE header")
    machine = struct.unpack_from("<H", data, pe_offset + 4)[0]
    expected = {"x86_64": 0x8664, "aarch64": 0xAA64}[expected_arch]
else:
    reject(f"unsupported expected OS {expected_os}")

if machine != expected:
    reject(f"expected {expected_arch} machine {expected:#x}, found {machine:#x}")
PY
}

if [[ -e "$stage" || -e "$archive" || -e "$portable_zip" ]]; then
  echo "package output already exists: $stage, $archive, or $portable_zip" >&2
  echo "set DIST_DIR to a fresh directory or remove the previous package output" >&2
  exit 1
fi

cargo build --release --locked --offline -p kiana-entrypoints --bin kiana
verify_binary_format "target/release/kiana${exe_ext}"

mkdir -p "$stage/docs/eval/fixtures" "$stage/docs/proof-templates" "$stage/docs/reference_audit" "$stage/docs/schemas" "$stage/scripts"
cp "target/release/kiana${exe_ext}" "$stage/"
cp VERSION README.md RELEASE.md INSTALL.md CONFIG.md USAGE.md CHANGELOG.md UPGRADE.md SECURITY.md PRIVACY.md TELEMETRY.md "$stage/"
cp LICENSE-MIT LICENSE-APACHE "$stage/"
cp docs/reference-migration-roadmap.md docs/reference-feature-matrix.md docs/commercial-release-readiness.md docs/release-checklist.md docs/distribution-channels.md docs/sdk-runtime-events.md "$stage/docs/"
cp docs/reference_audit/*.md "$stage/docs/reference_audit/"
cp docs/proof-templates/README.md docs/proof-templates/*.example.json "$stage/docs/proof-templates/"
cp docs/eval/fixtures/* "$stage/docs/eval/fixtures/"
cp docs/schemas/*.json "$stage/docs/schemas/"
cp scripts/install-release-binary.sh scripts/package-lifecycle-smoke.sh scripts/product-shell-smoke.sh scripts/validate-json-schema.py scripts/schema-contract-smoke.sh scripts/commercial-release-blockers-report.sh scripts/source-control-proof-report.sh scripts/distribution-review-report.sh scripts/local-rc-evidence-report.sh scripts/commercial-release-handoff-smoke.sh scripts/stage-commercial-release-proofs.sh scripts/entitlement-proof-report.sh scripts/product-acceptance-report.sh scripts/release-ops-report.sh scripts/platform-security-proof-report.sh scripts/provider-live-smoke.sh scripts/remote-live-smoke.sh scripts/sign-release-artifacts.sh scripts/verify-commercial-release-artifacts.sh scripts/release-signature-verification-smoke.sh "$stage/scripts/"

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
