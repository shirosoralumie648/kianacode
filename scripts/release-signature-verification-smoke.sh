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
  local signer="${4:-kiana release engineering}"
  cat > "$dist_dir/${package}.signature.json" <<EOF
{
  "schema": "kiana.release-signature.v1",
  "target": "$target",
  "archive": "$archive_name",
  "signed_at": "2026-01-01T00:00:00Z",
  "signer": "$signer",
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

run_commercial_verifier() {
  DIST_DIR="$dist_dir" \
    MANIFEST_DIR="$manifest_dir" \
    KIANA_SIGNATURE_VERIFY_COMMAND="$verify_command" \
    bash scripts/verify-commercial-release-artifacts.sh
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

set +e
missing_signer_output="$(
  DIST_DIR="$sign_dist_dir" \
    KIANA_SIGNING_COMMAND="$sign_command" \
    KIANA_SIGNATURE_VERIFY_COMMAND="$verify_command" \
    bash scripts/sign-release-artifacts.sh 2>&1
)"
missing_signer_status=$?
set -e
if [[ "$missing_signer_status" -eq 0 ]] ||
  ! grep -Fq 'KIANA_RELEASE_SIGNER is required' <<<"$missing_signer_output"; then
  echo "signing script accepted a missing release signer" >&2
  echo "$missing_signer_output" >&2
  exit 1
fi

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
  "$dist_dir/proofs/platform-security" \
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
  "accepted_at": "2026-07-01T00:00:00Z",
  "archive": "$archive_name",
  "authority": "fixture-apple-notary",
  "notarization_id": "fixture-notary-${target}",
  "schema": "kiana.macos-notarization.v1",
  "status": "accepted",
  "target": "$target"
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
  "release_base_url": "https://github.com/acme/kiana/releases/download/v${version}",
  "artifacts": [
    {
      "target": "linux-x86_64",
      "archive": "kiana-${version}-linux-x86_64.tar.gz",
      "url": "https://github.com/acme/kiana/releases/download/v${version}/kiana-${version}-linux-x86_64.tar.gz",
      "sha256": "fixture-linux-archive-sha",
      "binary_sha256": "fixture-linux-binary-sha"
    },
    {
      "target": "macos-x86_64",
      "archive": "kiana-${version}-macos-x86_64.tar.gz",
      "url": "https://github.com/acme/kiana/releases/download/v${version}/kiana-${version}-macos-x86_64.tar.gz",
      "sha256": "fixture-macos-archive-sha",
      "binary_sha256": "fixture-macos-binary-sha"
    },
    {
      "target": "windows-x86_64",
      "archive": "kiana-${version}-windows-x86_64.tar.gz",
      "url": "https://github.com/acme/kiana/releases/download/v${version}/kiana-${version}-windows-x86_64.tar.gz",
      "sha256": "fixture-windows-archive-sha",
      "binary_sha256": "fixture-windows-binary-sha"
    }
  ],
  "channels": {
    "github_releases": "generated_from_release_base_url",
    "homebrew": "generated",
    "winget": "generated"
  }
}
EOF

cat > "$dist_dir/proofs/live-smoke/provider/model-catalog-live.json" <<'EOF'
{
  "schema": "kiana.model-catalog.v1",
  "live": true,
  "summary": {
    "providers": 1,
    "discovered_models": 1,
    "skipped": 0,
    "failed": 0
  },
  "providers": [
    {
      "provider_id": "openai-compatible",
      "display_name": "OpenAI-compatible",
      "protocol": "open_ai_chat_completions",
      "models_source": "user_configured",
      "status": "passed",
      "live": true,
      "model_ids": ["gpt-fixture"],
      "discovered_model_ids": ["gpt-fixture"],
      "message": "fixture live catalog passed",
      "base_url": "https://provider.kiana.local/v1"
    }
  ]
}
EOF

cat > "$dist_dir/proofs/live-smoke/provider/model-smoke-live-tools.json" <<'EOF'
{
  "schema": "kiana.model-smoke.v1",
  "live": true,
  "tools": true,
  "summary": {
    "passed": 2,
    "skipped": 0,
    "failed": 0
  },
  "results": [
    {
      "provider_id": "openai-compatible",
      "model_id": "gpt-fixture",
      "status": "passed",
      "live": true,
      "capability": "text",
      "message": "fixture text smoke passed",
      "output_preview": "ok"
    },
    {
      "provider_id": "openai-compatible",
      "model_id": "gpt-fixture",
      "status": "passed",
      "live": true,
      "capability": "tools",
      "message": "fixture tool smoke passed",
      "output_preview": "tool_call"
    }
  ]
}
EOF

cat > "$dist_dir/proofs/live-smoke/remote/code-session-smoke.json" <<'EOF'
{
  "schema": "kiana.remote-code-session-smoke.v1",
  "status": "ok",
  "checked_at": "2026-01-01T00:00:00Z",
  "session_id": "cse_fixture",
  "api_base_url": "https://remote.kiana.local/api",
  "sdk_url": "https://remote.kiana.local/sdk/cse_fixture",
  "expires_in": 3600,
  "worker_epoch": 1
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
  "accepted": true,
  "accepted_by": "target customer acceptance lead",
  "accepted_at": "2026-01-01T00:00:00Z",
  "scope": "terminal product shell, local app-server, and context-search acceptance",
  "workflows": [
    "permission",
    "diff",
    "history",
    "onboarding",
    "resume",
    "settings",
    "app-server",
    "context-search"
  ]
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

for platform in linux macos windows; do
  case "$platform" in
    linux) isolation="linux_bwrap" ;;
    macos) isolation="macos_exec_policy" ;;
    windows) isolation="windows_exec_policy" ;;
  esac
  cat > "$dist_dir/proofs/platform-security/platform-security-${platform}.json" <<EOF
{
  "schema": "kiana.platform-security-proof.v1",
  "version": "$version",
  "status": "accepted",
  "accepted": true,
  "accepted_by": "security engineering",
  "accepted_at": "2026-01-01T00:00:00Z",
  "platform": "$platform",
  "runner": "${platform}-release-runner",
  "isolation": "$isolation",
  "controls": ["permission_profile:commercial", "permission_mode:ask"],
  "doctor_status": "ready",
  "evidence": [
    {
      "label": "fixture",
      "value": "${platform} platform security accepted"
    }
  ]
}
EOF
done

linux_package="kiana-${version}-linux-x86_64"
linux_archive_name="${linux_package}.tar.gz"
bad_signature="$dist_dir/${linux_archive_name}.sig"
good_signature="$(cat "$bad_signature")"
printf 'sha256:not-a-valid-signature\n' > "$bad_signature"
write_signature_proof "linux-x86_64" "$linux_archive_name" "$linux_package" "external-release-signer"
touch "$manifest_dir/homebrew/BLOCKED.md"
windows_zip="$dist_dir/kiana-${version}-windows-x86_64.zip"
mv "$windows_zip" "${windows_zip}.missing"
for proof in \
  "$dist_dir/proofs/entitlement/entitlement-proof.json" \
  "$dist_dir/proofs/product/product-acceptance.json" \
  "$dist_dir/proofs/release-ops/release-ops.json"
do
  mv "$proof" "${proof}.missing"
done

set +e
negative_output="$(run_commercial_verifier 2>&1)"
negative_status=$?
set -e
if [[ "$negative_status" -eq 0 ]]; then
  echo "commercial verifier accepted a multi-fault release fixture" >&2
  exit 1
fi
for expected in \
  "archive signature failed verification" \
  "default release signer placeholder" \
  "Homebrew channel manifest is still blocked" \
  "Windows target windows-x86_64 lacks" \
  "PROOF-MANIFEST.json" \
  "entitlement-proof.json" \
  "product-acceptance.json" \
  "release-ops.json"
do
  if ! grep -Fq "$expected" <<<"$negative_output"; then
    echo "commercial verifier multi-fault output missing: $expected" >&2
    echo "$negative_output" >&2
    exit 1
  fi
done

printf '%s\n' "$good_signature" > "$bad_signature"
write_signature_proof "linux-x86_64" "$linux_archive_name" "$linux_package"
rm -f "$manifest_dir/homebrew/BLOCKED.md"
mv "${windows_zip}.missing" "$windows_zip"
for proof in \
  "$dist_dir/proofs/entitlement/entitlement-proof.json" \
  "$dist_dir/proofs/product/product-acceptance.json" \
  "$dist_dir/proofs/release-ops/release-ops.json"
do
  mv "${proof}.missing" "$proof"
done

DIST_DIR="$dist_dir" bash scripts/stage-commercial-release-proofs.sh >/dev/null
run_commercial_verifier >/dev/null

echo "commercial release verifier smoke passed"
