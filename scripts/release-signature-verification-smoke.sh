#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

version="${VERSION:-$(tr -d '\r\n' < VERSION)}"
tmp_root="$(mktemp -d)"
tmp_root="$(cd "$tmp_root" && pwd)"
dist_dir="$tmp_root/dist"
manifest_dir="$dist_dir/manifests"
sign_dist_dir="$tmp_root/sign-dist"
verify_command="$tmp_root/verify-fixture-signature.sh"
sign_command="$tmp_root/sign-fixture-artifact.sh"

cleanup() {
  rm -rf "$tmp_root"
}
trap cleanup EXIT

hash_file() {
  local file="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$file" | awk '{print $1}'
  else
    shasum -a 256 "$file" | awk '{print $1}'
  fi
}

write_checksum() {
  local file="$1"
  local checksum="$2"
  printf '%s  %s\n' "$(hash_file "$file")" "$(basename "$file")" > "$checksum"
}

write_signature() {
  local file="$1"
  local signature="$2"
  printf 'sha256:%s\n' "$(hash_file "$file")" > "$signature"
}

write_signature_proof() {
  local target="$1"
  local archive_name="$2"
  local package="$3"
  cat > "$dist_dir/${package}.signature.json" <<EOF
{
  "schema": "kiana.release-signature.v1",
  "target": "$target",
  "archive": "$archive_name",
  "signed_at": "2026-01-01T00:00:00Z",
  "signer": "kiana release engineering",
  "signature_files": {
    "archive": "${archive_name}.sig",
    "binary": "${package}.binary.sig"
  },
  "verification": {
    "method": "KIANA_SIGNATURE_VERIFY_COMMAND",
    "archive": "verified",
    "binary": "verified",
    "verified_at": "2026-01-01T00:00:01Z"
  }
}
EOF
}

cat > "$verify_command" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

: "${KIANA_SIGNATURE_VERIFY_TARGET:?}"
: "${KIANA_SIGNATURE_VERIFY_SIGNATURE:?}"

if command -v sha256sum >/dev/null 2>&1; then
  expected="sha256:$(sha256sum "$KIANA_SIGNATURE_VERIFY_TARGET" | awk '{print $1}')"
else
  expected="sha256:$(shasum -a 256 "$KIANA_SIGNATURE_VERIFY_TARGET" | awk '{print $1}')"
fi
actual="$(tr -d '\r\n' < "$KIANA_SIGNATURE_VERIFY_SIGNATURE")"
[[ "$actual" == "$expected" ]]
EOF
chmod +x "$verify_command"

cat > "$sign_command" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

: "${KIANA_SIGN_INPUT:?}"
: "${KIANA_SIGN_OUTPUT:?}"

if command -v sha256sum >/dev/null 2>&1; then
  hash="$(sha256sum "$KIANA_SIGN_INPUT" | awk '{print $1}')"
else
  hash="$(shasum -a 256 "$KIANA_SIGN_INPUT" | awk '{print $1}')"
fi
printf 'sha256:%s\n' "$hash" > "$KIANA_SIGN_OUTPUT"
EOF
chmod +x "$sign_command"

mkdir -p "$sign_dist_dir"
sign_package="kiana-${version}-linux-x86_64"
sign_archive="$sign_dist_dir/${sign_package}.tar.gz"
sign_binary_sha="$sign_dist_dir/${sign_package}.binary.sha256"
printf 'signing fixture archive\n' > "$sign_archive"
printf 'signing fixture binary checksum\n' > "$sign_binary_sha"
DIST_DIR="$sign_dist_dir" \
  KIANA_RELEASE_SIGNER="kiana release engineering" \
  KIANA_SIGNING_COMMAND="$sign_command" \
  KIANA_SIGNATURE_VERIFY_COMMAND="$verify_command" \
  bash scripts/sign-release-artifacts.sh >/dev/null
if ! grep -Fq '"verification"' "$sign_dist_dir/${sign_package}.signature.json"; then
  echo "signing proof did not record verification status" >&2
  exit 1
fi

mkdir -p \
  "$dist_dir/proofs/live-smoke/provider" \
  "$dist_dir/proofs/live-smoke/remote" \
  "$dist_dir/proofs/entitlement" \
  "$dist_dir/proofs/product" \
  "$dist_dir/proofs/release-ops" \
  "$manifest_dir/homebrew" \
  "$manifest_dir/winget/Kiana/${version}" \
  "$manifest_dir/enterprise"

targets=(linux-x86_64 macos-x86_64 windows-x86_64)
for target in "${targets[@]}"; do
  package="kiana-${version}-${target}"
  archive_name="${package}.tar.gz"
  archive="$dist_dir/$archive_name"
  binary_sha="$dist_dir/${package}.binary.sha256"

  printf 'fixture archive for %s\n' "$target" > "$archive"
  printf 'fixture binary checksum for %s\n' "$target" > "$binary_sha"
  write_checksum "$archive" "${archive}.sha256"
  write_signature "$archive" "${archive}.sig"
  write_signature "$binary_sha" "$dist_dir/${package}.binary.sig"
  write_signature_proof "$target" "$archive_name" "$package"

  if [[ "$target" == macos-* ]]; then
    cat > "$dist_dir/${package}.notarization.json" <<EOF
{
  "schema": "kiana.macos-notarization.v1",
  "target": "$target",
  "status": "accepted"
}
EOF
  fi

  if [[ "$target" == windows-* ]]; then
    printf 'windows portable zip fixture\n' > "$dist_dir/${package}.zip"
  fi
done

cat > "$manifest_dir/homebrew/kiana.rb" <<'EOF'
class Kiana < Formula
  desc "Kiana"
end
EOF

cat > "$manifest_dir/winget/Kiana/${version}/Kiana.Kiana.installer.yaml" <<'EOF'
PackageIdentifier: Kiana.Kiana
PackageVersion: fixture
Installers: []
EOF

cat > "$manifest_dir/enterprise/offline-manifest.json" <<EOF
{
  "schema": "kiana.enterprise.offline-manifest.v1",
  "version": "$version",
  "artifacts": []
}
EOF

cat > "$dist_dir/proofs/live-smoke/provider/model-catalog-live.json" <<'EOF'
{
  "schema": "kiana.model-catalog.v1",
  "live": true
}
EOF

cat > "$dist_dir/proofs/live-smoke/provider/model-smoke-live-tools.json" <<'EOF'
{
  "schema": "kiana.model-smoke.v1",
  "live": true,
  "tools": true
}
EOF

cat > "$dist_dir/proofs/live-smoke/remote/code-session-smoke.json" <<'EOF'
{
  "schema": "kiana.remote-code-session-smoke.v1",
  "status": "ok"
}
EOF

cat > "$dist_dir/proofs/entitlement/entitlement-proof.json" <<EOF
{
  "schema": "kiana.entitlement-proof.v1",
  "version": "$version",
  "status": "accepted",
  "accepted": true,
  "accepted_by": "release engineering",
  "accepted_at": "2026-01-01T00:00:00Z",
  "account_id": "acct_live_fixture",
  "organization": "Kiana Customer",
  "plan": "enterprise",
  "license_key_fingerprint": "sha256:fixture",
  "support_contact": "support@kiana.local",
  "license_status": "active",
  "entitlements": ["commercial-use", "enterprise-support", "managed-policy"],
  "backend": {
    "name": "entitlement-service",
    "environment": "production",
    "checked_at": "2026-01-01T00:00:00Z",
    "request_id": "req-fixture"
  }
}
EOF

cat > "$dist_dir/proofs/product/product-acceptance.json" <<EOF
{
  "schema": "kiana.product-acceptance.v1",
  "version": "$version",
  "status": "accepted",
  "accepted": true
}
EOF

cat > "$dist_dir/proofs/release-ops/release-ops.json" <<EOF
{
  "schema": "kiana.release-ops.v1",
  "version": "$version",
  "status": "accepted",
  "accepted": true,
  "accepted_by": "release engineering",
  "accepted_at": "2026-01-01T00:00:00Z",
  "security_contact": "security@kiana.local",
  "vulnerability_report_channel": "private security intake",
  "release_credentials_owner": "release engineering",
  "support_contact": "support@kiana.local",
  "artifact_retention_days": 180,
  "log_retention_days": 90,
  "credential_review": {
    "status": "accepted",
    "reviewed_by": "security engineering",
    "reviewed_at": "2026-01-01T00:00:00Z",
    "scope": "release signing and publishing credentials"
  }
}
EOF

bad_signature="$dist_dir/kiana-${version}-linux-x86_64.tar.gz.sig"
good_signature="$(cat "$bad_signature")"
printf 'sha256:not-a-valid-signature\n' > "$bad_signature"
if DIST_DIR="$dist_dir" \
  MANIFEST_DIR="$manifest_dir" \
  KIANA_SIGNATURE_VERIFY_COMMAND="$verify_command" \
  bash scripts/verify-commercial-release-artifacts.sh >/dev/null 2>&1
then
  echo "commercial verifier accepted an invalid archive signature" >&2
  exit 1
fi

printf '%s\n' "$good_signature" > "$bad_signature"
DIST_DIR="$dist_dir" \
  MANIFEST_DIR="$manifest_dir" \
  KIANA_SIGNATURE_VERIFY_COMMAND="$verify_command" \
  bash scripts/verify-commercial-release-artifacts.sh >/dev/null

echo "release signature verification smoke passed"
