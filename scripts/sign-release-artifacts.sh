#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

mode="${1:-full}"
if [[ "$mode" == "--local-rc" ]]; then
  echo "release artifact signing skipped in local RC mode"
  exit 0
fi
if [[ "$mode" != "full" ]]; then
  echo "usage: $0 [full|--local-rc]" >&2
  exit 2
fi

version="${VERSION:-$(tr -d '\r\n' < VERSION)}"
dist_dir="${DIST_DIR:-dist}"
signer="${KIANA_RELEASE_SIGNER:-external-release-signer}"
signing_command="${KIANA_SIGNING_COMMAND:-}"
signature_verify_command="${KIANA_SIGNATURE_VERIFY_COMMAND:-}"

if [[ -z "$signing_command" ]]; then
  echo "KIANA_SIGNING_COMMAND is required to sign release artifacts" >&2
  echo 'example: KIANA_SIGNING_COMMAND='\''gpg --batch --yes --armor --detach-sign --output "$KIANA_SIGN_OUTPUT" "$KIANA_SIGN_INPUT"'\''' >&2
  exit 1
fi
if [[ -z "$signature_verify_command" ]]; then
  echo "KIANA_SIGNATURE_VERIFY_COMMAND is required to verify release artifact signatures" >&2
  echo 'example: KIANA_SIGNATURE_VERIFY_COMMAND='\''gpg --batch --verify "$KIANA_SIGNATURE_VERIFY_SIGNATURE" "$KIANA_SIGNATURE_VERIFY_TARGET"'\''' >&2
  exit 1
fi

json_escape() {
  local value="$1"
  value="${value//\\/\\\\}"
  value="${value//\"/\\\"}"
  value="${value//$'\n'/\\n}"
  printf '%s' "$value"
}

sign_file() {
  local input="$1"
  local output="$2"
  rm -f "$output"
  KIANA_SIGN_INPUT="$input" \
    KIANA_SIGN_OUTPUT="$output" \
    bash -c "$signing_command"
  if [[ ! -s "$output" ]]; then
    echo "signing command did not create signature: $output" >&2
    exit 1
  fi
}

verify_signature() {
  local input="$1"
  local signature="$2"
  if ! KIANA_SIGNATURE_VERIFY_TARGET="$input" \
    KIANA_SIGNATURE_VERIFY_SIGNATURE="$signature" \
    bash -c "$signature_verify_command"
  then
    echo "signature verification failed: $signature -> $input" >&2
    exit 1
  fi
}

write_signature_proof() {
  local target="$1"
  local archive_name="$2"
  local archive_sig="$3"
  local binary_sig="$4"
  local proof="$5"
  local signed_at
  local verified_at
  signed_at="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
  verified_at="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
  cat > "$proof" <<EOF
{
  "schema": "kiana.release-signature.v1",
  "target": "$(json_escape "$target")",
  "archive": "$(json_escape "$archive_name")",
  "signed_at": "$(json_escape "$signed_at")",
  "signer": "$(json_escape "$signer")",
  "signature_files": {
    "archive": "$(json_escape "$(basename "$archive_sig")")",
    "binary": "$(json_escape "$(basename "$binary_sig")")"
  },
  "verification": {
    "method": "KIANA_SIGNATURE_VERIFY_COMMAND",
    "archive": "verified",
    "binary": "verified",
    "verified_at": "$(json_escape "$verified_at")"
  }
}
EOF
}

write_notarization_proof() {
  local target="$1"
  local archive="$2"
  local proof="$3"
  if [[ -n "${KIANA_MACOS_NOTARIZATION_PROOF_FILE:-}" ]]; then
    cp "$KIANA_MACOS_NOTARIZATION_PROOF_FILE" "$proof"
  elif [[ -n "${KIANA_MACOS_NOTARIZATION_COMMAND:-}" ]]; then
    rm -f "$proof"
    KIANA_NOTARIZATION_TARGET="$target" \
      KIANA_NOTARIZATION_ARCHIVE="$archive" \
      KIANA_NOTARIZATION_PROOF="$proof" \
      bash -c "$KIANA_MACOS_NOTARIZATION_COMMAND"
  else
    echo "macOS target $target requires KIANA_MACOS_NOTARIZATION_COMMAND or KIANA_MACOS_NOTARIZATION_PROOF_FILE" >&2
    exit 1
  fi
  if [[ ! -s "$proof" ]]; then
    echo "macOS notarization proof was not created: $proof" >&2
    exit 1
  fi
  if ! grep -Eq '"schema"[[:space:]]*:[[:space:]]*"kiana.macos-notarization.v1"' "$proof" ||
    ! grep -Eq '"status"[[:space:]]*:[[:space:]]*"accepted"' "$proof"; then
    echo "macOS notarization proof must use kiana.macos-notarization.v1 with status=accepted: $proof" >&2
    exit 1
  fi
}

shopt -s nullglob
archives=("${dist_dir}/kiana-${version}-"*.tar.gz)
if (( ${#archives[@]} == 0 )); then
  echo "no release archives found in ${dist_dir}; run scripts/package-release.sh first" >&2
  exit 1
fi

for archive in "${archives[@]}"; do
  filename="$(basename "$archive")"
  package="${filename%.tar.gz}"
  target="${package#kiana-${version}-}"
  binary_sha="${dist_dir}/${package}.binary.sha256"
  archive_sig="${archive}.sig"
  binary_sig="${dist_dir}/${package}.binary.sig"
  signature_proof="${dist_dir}/${package}.signature.json"

  if [[ ! -f "$binary_sha" ]]; then
    echo "missing binary checksum file: $binary_sha" >&2
    exit 1
  fi

  sign_file "$archive" "$archive_sig"
  sign_file "$binary_sha" "$binary_sig"
  verify_signature "$archive" "$archive_sig"
  verify_signature "$binary_sha" "$binary_sig"
  write_signature_proof "$target" "$filename" "$archive_sig" "$binary_sig" "$signature_proof"

  if [[ "$target" == macos-* ]]; then
    write_notarization_proof "$target" "$archive" "${dist_dir}/${package}.notarization.json"
  fi
done

echo "release artifact signing proof written to ${dist_dir}"
