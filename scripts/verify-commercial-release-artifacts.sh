#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

mode="${1:-full}"
if [[ "$mode" == "--local-rc" ]]; then
  echo "commercial artifact verification skipped in local RC mode"
  exit 0
fi
if [[ "$mode" != "full" ]]; then
  echo "usage: $0 [full|--local-rc]" >&2
  exit 2
fi

version="${VERSION:-$(tr -d '\r\n' < VERSION)}"
dist_dir="${DIST_DIR:-dist}"
manifest_dir="${MANIFEST_DIR:-${dist_dir}/manifests}"
failures=0
seen_linux=0
seen_macos=0
seen_windows=0

fail() {
  echo "FAIL: $*" >&2
  failures=$((failures + 1))
}

pass() {
  echo "OK: $*"
}

require_file() {
  if [[ -f "$1" ]]; then
    pass "file exists: $1"
  else
    fail "missing file: $1"
  fi
}

require_json_pattern() {
  local file="$1"
  local pattern="$2"
  local label="$3"
  if [[ ! -f "$file" ]]; then
    return
  fi
  if grep -Eq "$pattern" "$file"; then
    pass "$label"
  else
    fail "$file missing $label"
  fi
}

has_windows_publishable_artifact() {
  local package="$1"
  [[ -f "${dist_dir}/${package}.zip" ]] || \
    [[ -f "${dist_dir}/${package}.msi" ]] || \
    [[ -f "${dist_dir}/${package}.exe" ]]
}

shopt -s nullglob
archives=("${dist_dir}/kiana-${version}-"*.tar.gz)
if (( ${#archives[@]} == 0 )); then
  fail "no release archives found in ${dist_dir}; run package-release on every release runner first"
fi

for archive in "${archives[@]}"; do
  filename="$(basename "$archive")"
  package="${filename%.tar.gz}"
  target="${package#kiana-${version}-}"
  signature_proof="${dist_dir}/${package}.signature.json"

  case "$target" in
    linux-*) seen_linux=1 ;;
    macos-*) seen_macos=1 ;;
    windows-*) seen_windows=1 ;;
  esac

  require_file "${archive}.sha256"
  require_file "${dist_dir}/${package}.binary.sha256"
  require_file "${archive}.sig"
  require_file "${dist_dir}/${package}.binary.sig"
  require_file "$signature_proof"
  require_json_pattern "$signature_proof" '"schema"[[:space:]]*:[[:space:]]*"kiana.release-signature.v1"' "release signature proof schema"
  require_json_pattern "$signature_proof" "\"target\"[[:space:]]*:[[:space:]]*\"${target}\"" "release signature target matches"
  require_json_pattern "$signature_proof" "\"archive\"[[:space:]]*:[[:space:]]*\"${filename}\"" "release signature archive matches"

  if [[ "$target" == macos-* ]]; then
    notarization_proof="${dist_dir}/${package}.notarization.json"
    require_file "$notarization_proof"
    require_json_pattern "$notarization_proof" '"schema"[[:space:]]*:[[:space:]]*"kiana.macos-notarization.v1"' "macOS notarization proof schema"
    require_json_pattern "$notarization_proof" '"status"[[:space:]]*:[[:space:]]*"accepted"' "macOS notarization accepted"
  fi

  if [[ "$target" == windows-* ]]; then
    if has_windows_publishable_artifact "$package"; then
      pass "Windows target has winget-compatible ZIP/MSI/EXE artifact"
    else
      fail "Windows target ${target} lacks a winget-compatible ZIP/MSI/EXE artifact"
    fi
  fi
done

if (( seen_linux == 1 )); then
  pass "Linux release artifact present"
else
  fail "Linux release artifact missing"
fi
if (( seen_macos == 1 )); then
  pass "macOS release artifact present"
else
  fail "macOS release artifact missing"
fi
if (( seen_windows == 1 )); then
  pass "Windows release artifact present"
else
  fail "Windows release artifact missing"
fi

if [[ -f "${manifest_dir}/homebrew/BLOCKED.md" ]]; then
  fail "Homebrew channel manifest is still blocked"
else
  pass "Homebrew channel manifest is not blocked"
fi

if [[ -f "${manifest_dir}/winget/BLOCKED.md" ]]; then
  fail "winget channel manifest is still blocked"
else
  pass "winget channel manifest is not blocked"
fi

enterprise_manifest="${manifest_dir}/enterprise/offline-manifest.json"
require_file "$enterprise_manifest"
require_json_pattern "$enterprise_manifest" '"schema"[[:space:]]*:[[:space:]]*"kiana.enterprise.offline-manifest.v1"' "enterprise offline manifest schema"
if [[ -f "$enterprise_manifest" ]] && grep -Eq 'pending_|dry_run|blocked_' "$enterprise_manifest"; then
  fail "enterprise offline manifest still contains pending/dry-run/blocked channel state"
elif [[ -f "$enterprise_manifest" ]]; then
  pass "enterprise offline manifest contains no pending channel states"
fi

if (( failures > 0 )); then
  echo "commercial release artifact verification failed with ${failures} issue(s)" >&2
  exit 1
fi

echo "commercial release artifact verification passed"
