#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

mode="${1:-full}"
if [[ "$mode" != "full" && "$mode" != "--local-rc" ]]; then
  echo "usage: $0 [--local-rc]" >&2
  exit 2
fi

failures=0
bash_bin="${BASH:-bash}"

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

require_manifest_field() {
  local manifest="$1"
  local pattern="$2"
  if grep -Eq "$pattern" "$manifest"; then
    return 0
  fi
  fail "$manifest missing pattern: $pattern"
}

for file in \
  VERSION README.md RELEASE.md INSTALL.md CONFIG.md USAGE.md CHANGELOG.md UPGRADE.md \
  SECURITY.md PRIVACY.md TELEMETRY.md LICENSE-MIT LICENSE-APACHE deny.toml \
  docs/commercial-release-readiness.md docs/release-checklist.md docs/distribution-channels.md \
  docs/sdk-runtime-events.md \
  docs/proof-templates/README.md \
  docs/proof-templates/product-acceptance.example.json \
  docs/proof-templates/entitlement-proof.example.json \
  docs/proof-templates/release-ops.example.json \
  docs/proof-templates/platform-security.example.json \
  docs/schemas/kiana-app-server-contract.v1.schema.json \
  docs/schemas/kiana-app-server-conversations.v1.schema.json \
  docs/schemas/kiana-app-server-events.v1.schema.json \
  docs/schemas/kiana-app-server-settings.v1.schema.json \
  docs/schemas/kiana-app-server-secrets.v1.schema.json \
  docs/schemas/kiana-app-server-sandbox.v1.schema.json \
  docs/schemas/kiana-app-server-plugins.v1.schema.json \
  docs/schemas/kiana-app-server-git-status.v1.schema.json \
  docs/schemas/kiana-checks-dry-run.v1.schema.json \
  docs/schemas/kiana-review-dry-run.v1.schema.json \
  docs/schemas/kiana-review-run.v1.schema.json \
  docs/schemas/kiana-doctor.v1.schema.json docs/schemas/kiana-model-smoke.v1.schema.json \
  docs/schemas/kiana-model-catalog.v1.schema.json \
  docs/schemas/kiana-context-index.v1.schema.json \
  docs/schemas/kiana-context-search.v1.schema.json \
  docs/schemas/kiana-context-pack.v1.schema.json \
  docs/schemas/kiana-commercial-proof-manifest.v1.schema.json \
  docs/schemas/kiana-commercial-release-blockers.v1.schema.json \
  docs/schemas/kiana-local-rc-evidence.v1.schema.json \
  docs/schemas/kiana-runtime-event.v1.schema.json \
  docs/schemas/kiana-license-status.v1.schema.json \
  docs/schemas/kiana-managed-plugin-policy.v1.schema.json \
  docs/schemas/kiana-plugin-install-receipt.v1.schema.json \
  docs/schemas/kiana-enterprise-offline-manifest.v1.schema.json \
  docs/schemas/kiana-entitlement-proof.v1.schema.json \
  docs/schemas/kiana-product-acceptance.v1.schema.json \
  docs/schemas/kiana-platform-security-proof.v1.schema.json \
  docs/schemas/kiana-remote-code-session-smoke.v1.schema.json \
  docs/schemas/kiana-release-signature.v1.schema.json \
  docs/schemas/kiana-macos-notarization.v1.schema.json \
  docs/schemas/kiana-release-ops.v1.schema.json \
  scripts/release-smoke.sh scripts/package-release.sh scripts/install-release-binary.sh \
  scripts/package-lifecycle-smoke.sh scripts/product-shell-smoke.sh \
  scripts/validate-json-schema.py scripts/schema-contract-smoke.sh \
  scripts/commercial-release-blockers-report.sh \
  scripts/local-rc-evidence-report.sh \
  scripts/commercial-release-handoff-smoke.sh \
  scripts/stage-commercial-release-proofs.sh \
  scripts/entitlement-proof-report.sh \
  scripts/product-acceptance-report.sh \
  scripts/release-ops-report.sh \
  scripts/platform-security-proof-report.sh \
  scripts/provider-live-smoke.sh scripts/remote-live-smoke.sh \
  scripts/sign-release-artifacts.sh \
  scripts/verify-commercial-release-artifacts.sh \
  scripts/release-signature-verification-smoke.sh \
  scripts/generate-sbom.sh scripts/compliance-audit.sh scripts/install-compliance-tools.sh \
  scripts/generate-distribution-manifests.sh \
  .github/workflows/release-smoke.yml .github/workflows/release.yml
do
  require_file "$file"
done

if [[ "${KIANA_PREFLIGHT_SKIP_COMPLIANCE:-}" == "1" ]]; then
  pass "compliance audit skipped by KIANA_PREFLIGHT_SKIP_COMPLIANCE=1"
elif [[ "$mode" == "full" ]]; then
  if "$bash_bin" scripts/compliance-audit.sh; then
    pass "full compliance audit passed"
  else
    fail "full compliance audit failed"
  fi
else
  if "$bash_bin" scripts/compliance-audit.sh --local-rc; then
    pass "local RC compliance audit passed"
  else
    fail "local RC compliance audit failed"
  fi
fi

if git check-ignore -q .kiana/tasks/default/1.json; then
  pass "local task state is ignored"
else
  fail ".kiana/tasks/default/1.json is not ignored"
fi

if git check-ignore -q .claude/ralph-loop.local.md; then
  pass "local assistant state is ignored"
else
  fail ".claude/ralph-loop.local.md is not ignored"
fi

if git check-ignore -q reference/claude-code-rev-main; then
  pass "raw reference checkout is ignored"
else
  fail "raw reference checkout is not ignored"
fi

unignored_local_state="$(git ls-files --others --exclude-standard | grep -E '(^|/)[.]kiana/(tasks|tmp|logs)/|(^|/)[.]claude/.*[.]local[.]md|post-tool-use-hook-.*post-hook[.]json' || true)"
if [[ -z "$unignored_local_state" ]]; then
  pass "no unignored local runtime state remains"
else
  echo "$unignored_local_state" | head -n 20 >&2
  fail "local runtime state is still visible to git"
fi

workspace_version="$(tr -d '\r\n' < VERSION)"
if grep -Eq "^version = \"${workspace_version}\"$" Cargo.toml; then
  pass "VERSION matches workspace package version"
else
  fail "VERSION does not match workspace package version"
fi

for manifest in kiana-*/Cargo.toml; do
  [[ -f "$manifest" ]] || continue
  require_manifest_field "$manifest" '^license[.]workspace = true$'
  require_manifest_field "$manifest" '^repository[.]workspace = true$'
  require_manifest_field "$manifest" '^rust-version[.]workspace = true$'
  require_manifest_field "$manifest" '^publish = false$'
done

if grep -Fq 'doctor --json' scripts/release-smoke.sh; then
  pass "doctor JSON smoke gate is wired"
else
  fail "release smoke does not exercise doctor --json"
fi

if grep -Fq 'model smoke --json' scripts/release-smoke.sh; then
  pass "model smoke JSON gate is wired"
else
  fail "release smoke does not exercise model smoke --json"
fi

if grep -Fq 'model catalog --json' scripts/release-smoke.sh; then
  pass "model catalog JSON gate is wired"
else
  fail "release smoke does not exercise model catalog --json"
fi

if grep -Fq 'context index --json' scripts/release-smoke.sh &&
  grep -Fq 'context search release --json --limit 1' scripts/release-smoke.sh; then
  pass "context index/search JSON gates are wired"
else
  fail "release smoke does not exercise context index/search --json"
fi

if grep -Fq 'license status --json' scripts/release-smoke.sh; then
  pass "license status JSON gate is wired"
else
  fail "release smoke does not exercise license status --json"
fi

if grep -Fq 'product-shell-smoke.sh' scripts/release-smoke.sh; then
  pass "product shell smoke gate is wired"
else
  fail "release smoke does not exercise product shell smoke"
fi

if grep -Fq 'context.index.read' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'context.index.cache.write' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'build_persistent_context_index' kiana-entrypoints/src/cli.rs &&
  grep -Fq '.kiana/context-index.json' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'checks.dry_run.read' kiana-entrypoints/src/cli.rs &&
  grep -Fq '/app/checks/dry-run' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'review.dry_run.read' kiana-entrypoints/src/cli.rs &&
  grep -Fq '/app/review/dry-run' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'review.run.read' kiana-entrypoints/src/cli.rs &&
  grep -Fq '/app/review' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'context.search.read' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'context.pack.read' kiana-entrypoints/src/cli.rs &&
  grep -Fq '/app/context/index' kiana-entrypoints/src/cli.rs &&
  grep -Fq '/app/context/search' kiana-entrypoints/src/cli.rs &&
  grep -Fq '/app/context/pack' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'kiana.checks.dry_run.v1' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'kiana.review.dry_run.v1' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'kiana.review.run.v1' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'kiana.context-index.v1' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'kiana.context-search.v1' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'kiana.context-pack.v1' kiana-entrypoints/src/cli.rs; then
  pass "app-server context, checks, review dry-run/run, and cache endpoints are wired"
else
  fail "app-server context, checks, review dry-run/run, or cache endpoints are not wired"
fi

if grep -Fq 'kiana.plugin-install-receipt.v1' scripts/release-smoke.sh &&
  grep -Fq '.kiana-install-receipt.json' scripts/release-smoke.sh &&
  grep -Fq 'install_receipt_integrity' scripts/release-smoke.sh &&
  grep -Fq '"status": "tampered"' scripts/release-smoke.sh; then
  pass "plugin install receipt integrity smoke gate is wired"
else
  fail "release smoke does not exercise plugin install receipt integrity"
fi

if grep -Fq 'KIANA_MANAGED_PLUGIN_POLICY_FILE' kiana-commands/src/plugin.rs &&
  grep -Fq 'managed plugin policy' kiana-commands/src/plugin.rs; then
  pass "managed plugin allow/deny policy is wired"
else
  fail "managed plugin allow/deny policy is not wired"
fi

if grep -Fq '"app-server"' scripts/product-acceptance-report.sh &&
  grep -Fq '"context-search"' scripts/product-acceptance-report.sh &&
  grep -Fq '"context-cache-recovery"' scripts/product-acceptance-report.sh &&
  grep -Fq 'persistent_context_index_recovers_from_corrupt_cache' scripts/product-acceptance-report.sh &&
  grep -Fq 'cargo test -p kiana-commands --locked --offline context_search' scripts/product-acceptance-report.sh; then
  pass "product acceptance requires app-server, context-search, and context-cache-recovery workflows"
else
  fail "product acceptance does not require app-server, context-search, and context-cache-recovery workflows"
fi

if grep -Fq 'provider-live-smoke.sh' RELEASE.md &&
  grep -Fq 'remote-live-smoke.sh' RELEASE.md &&
  grep -Fq 'provider-live-smoke.sh --required' scripts/release-preflight.sh &&
  grep -Fq 'remote-live-smoke.sh --required' scripts/release-preflight.sh; then
  pass "live provider and remote smoke gates are documented and wired into full preflight"
else
  fail "live provider and remote smoke gates are not fully documented or wired"
fi

if grep -Fq '"status": "blocked"' docs/proof-templates/product-acceptance.example.json &&
  grep -Fq '"status": "blocked"' docs/proof-templates/entitlement-proof.example.json &&
  grep -Fq '"status": "blocked"' docs/proof-templates/release-ops.example.json &&
  grep -Fq '"status": "blocked"' docs/proof-templates/platform-security.example.json &&
  ! grep -Fq '"status": "accepted"' docs/proof-templates/*.example.json; then
  pass "commercial proof templates are explicitly non-accepted examples"
else
  fail "commercial proof templates must remain blocked examples, not accepted release proofs"
fi

if grep -Fq '"const": "kiana.app-server.contract.v1"' docs/schemas/kiana-app-server-contract.v1.schema.json; then
  pass "app-server contract JSON schema version is pinned"
else
  fail "app-server contract JSON schema is missing kiana.app-server.contract.v1 const"
fi

for app_schema in conversations events settings secrets sandbox plugins git-status; do
  schema_file="docs/schemas/kiana-app-server-${app_schema}.v1.schema.json"
  schema_name="kiana.app-server.${app_schema}.v1"
  if grep -Fq "\"const\": \"${schema_name}\"" "$schema_file"; then
    pass "app-server ${app_schema} JSON schema version is pinned"
  else
    fail "app-server ${app_schema} JSON schema is missing ${schema_name} const"
  fi
done

if grep -Fq '"const": "kiana.checks.dry_run.v1"' docs/schemas/kiana-checks-dry-run.v1.schema.json &&
  grep -Fq '"rustfmt"' docs/schemas/kiana-checks-dry-run.v1.schema.json &&
  grep -Fq '"cargo_check"' docs/schemas/kiana-checks-dry-run.v1.schema.json; then
  pass "checks dry-run JSON schema version is pinned"
else
  fail "checks dry-run JSON schema is missing required quality-gate anchors"
fi

if grep -Fq '"const": "kiana.review.dry_run.v1"' docs/schemas/kiana-review-dry-run.v1.schema.json &&
  grep -Fq '"create_isolated_worktree"' docs/schemas/kiana-review-dry-run.v1.schema.json &&
  grep -Fq '"run_configured_checks"' docs/schemas/kiana-review-dry-run.v1.schema.json; then
  pass "review dry-run JSON schema version is pinned"
else
  fail "review dry-run JSON schema is missing required local-review anchors"
fi

if grep -Fq '"const": "kiana.review.run.v1"' docs/schemas/kiana-review-run.v1.schema.json &&
  grep -Fq '"const": "kiana.checks.run.v1"' docs/schemas/kiana-review-run.v1.schema.json &&
  grep -Fq '"git_worktree"' docs/schemas/kiana-review-run.v1.schema.json; then
  pass "review run JSON schema version is pinned"
else
  fail "review run JSON schema is missing required isolated-review anchors"
fi

if grep -Fq '"const": "kiana.doctor.v1"' docs/schemas/kiana-doctor.v1.schema.json; then
  pass "doctor JSON schema version is pinned"
else
  fail "doctor JSON schema is missing kiana.doctor.v1 const"
fi
if grep -Fq '"reference_capabilities"' docs/schemas/kiana-doctor.v1.schema.json &&
  grep -Fq '"local_ready_external_required"' docs/schemas/kiana-doctor.v1.schema.json; then
  pass "doctor reference capability matrix schema is pinned"
else
  fail "doctor JSON schema is missing reference capability matrix contract"
fi

if grep -Fq '"const": "kiana.model-smoke.v1"' docs/schemas/kiana-model-smoke.v1.schema.json; then
  pass "model smoke JSON schema version is pinned"
else
  fail "model smoke JSON schema is missing kiana.model-smoke.v1 const"
fi

if grep -Fq '"const": "kiana.model-catalog.v1"' docs/schemas/kiana-model-catalog.v1.schema.json; then
  pass "model catalog JSON schema version is pinned"
else
  fail "model catalog JSON schema is missing kiana.model-catalog.v1 const"
fi

if grep -Fq '"const": "kiana.context-index.v1"' docs/schemas/kiana-context-index.v1.schema.json &&
  grep -Fq '"cache"' docs/schemas/kiana-context-index.v1.schema.json &&
  grep -Fq '"recovered"' docs/schemas/kiana-context-index.v1.schema.json &&
  grep -Fq '"reused_files"' docs/schemas/kiana-context-index.v1.schema.json &&
  grep -Fq '"removed_files"' docs/schemas/kiana-context-index.v1.schema.json; then
  pass "context index JSON schema version is pinned"
else
  fail "context index JSON schema is missing required v1 cache recovery contract anchors"
fi

if grep -Fq '"const": "kiana.context-search.v1"' docs/schemas/kiana-context-search.v1.schema.json; then
  pass "context search JSON schema version is pinned"
else
  fail "context search JSON schema is missing kiana.context-search.v1 const"
fi

if grep -Fq '"const": "kiana.context-pack.v1"' docs/schemas/kiana-context-pack.v1.schema.json; then
  pass "context pack JSON schema version is pinned"
else
  fail "context pack JSON schema is missing kiana.context-pack.v1 const"
fi

if grep -Fq '"const": "kiana.commercial-proof-manifest.v1"' docs/schemas/kiana-commercial-proof-manifest.v1.schema.json; then
  pass "commercial proof manifest JSON schema version is pinned"
else
  fail "commercial proof manifest JSON schema is missing kiana.commercial-proof-manifest.v1 const"
fi

if grep -Fq '"const": "kiana.commercial-release-blockers.v1"' docs/schemas/kiana-commercial-release-blockers.v1.schema.json; then
  pass "commercial release blockers JSON schema version is pinned"
else
  fail "commercial release blockers JSON schema is missing kiana.commercial-release-blockers.v1 const"
fi

if grep -Fq '"const": "kiana.local-rc-evidence.v1"' docs/schemas/kiana-local-rc-evidence.v1.schema.json; then
  pass "local RC evidence JSON schema version is pinned"
else
  fail "local RC evidence JSON schema is missing kiana.local-rc-evidence.v1 const"
fi

if grep -Fq '"$id": "https://kiana.local/schemas/kiana-runtime-event.v1.schema.json"' docs/schemas/kiana-runtime-event.v1.schema.json &&
  grep -Fq '"permission_request"' docs/schemas/kiana-runtime-event.v1.schema.json &&
  grep -Fq '"tool_result"' docs/schemas/kiana-runtime-event.v1.schema.json; then
  pass "runtime event JSON schema version is pinned"
else
  fail "runtime event JSON schema is missing required v1 contract anchors"
fi

blockers_json="$("$bash_bin" scripts/commercial-release-blockers-report.sh --json || true)"
if printf '%s\n' "$blockers_json" | grep -Fq '"schema": "kiana.commercial-release-blockers.v1"'; then
  pass "commercial release blockers report JSON gate is wired"
else
  fail "commercial release blockers report JSON gate is not wired"
fi

if "$bash_bin" scripts/commercial-release-handoff-smoke.sh; then
  pass "commercial release handoff smoke passed"
else
  fail "commercial release handoff smoke failed"
fi

if "$bash_bin" scripts/schema-contract-smoke.sh; then
  pass "schema contract smoke passed"
else
  fail "schema contract smoke failed"
fi

if grep -Fq '"const": "kiana.plugin-install-receipt.v1"' docs/schemas/kiana-plugin-install-receipt.v1.schema.json; then
  pass "plugin install receipt JSON schema version is pinned"
else
  fail "plugin install receipt JSON schema is missing kiana.plugin-install-receipt.v1 const"
fi

if grep -Fq '"const": "stable-hash-v1"' docs/schemas/kiana-plugin-install-receipt.v1.schema.json &&
  grep -Fq 'plugin_receipt_payload_hash' kiana-commands/src/plugin.rs &&
  grep -Fq 'install_receipt_integrity' kiana-commands/src/plugin.rs; then
  pass "plugin install receipt integrity is sealed and exposed"
else
  fail "plugin install receipt integrity is not sealed and exposed"
fi

if grep -Fq '"const": "kiana.managed-plugin-policy.v1"' docs/schemas/kiana-managed-plugin-policy.v1.schema.json; then
  pass "managed plugin policy JSON schema version is pinned"
else
  fail "managed plugin policy JSON schema is missing kiana.managed-plugin-policy.v1 const"
fi

if grep -Fq '"const": "kiana.license-status.v1"' docs/schemas/kiana-license-status.v1.schema.json; then
  pass "license status JSON schema version is pinned"
else
  fail "license status JSON schema is missing kiana.license-status.v1 const"
fi

if grep -Fq '"const": "kiana.enterprise.offline-manifest.v1"' docs/schemas/kiana-enterprise-offline-manifest.v1.schema.json; then
  pass "enterprise offline manifest JSON schema version is pinned"
else
  fail "enterprise offline manifest JSON schema is missing kiana.enterprise.offline-manifest.v1 const"
fi

if grep -Fq '"const": "kiana.entitlement-proof.v1"' docs/schemas/kiana-entitlement-proof.v1.schema.json; then
  pass "entitlement proof JSON schema version is pinned"
else
  fail "entitlement proof JSON schema is missing kiana.entitlement-proof.v1 const"
fi

if grep -Fq '"allOf"' docs/schemas/kiana-entitlement-proof.v1.schema.json &&
  grep -Fq '"const": "accepted"' docs/schemas/kiana-entitlement-proof.v1.schema.json &&
  grep -Fq '"const": true' docs/schemas/kiana-entitlement-proof.v1.schema.json; then
  pass "entitlement proof accepted-state schema contract is pinned"
else
  fail "entitlement proof accepted-state schema contract is not pinned"
fi

if grep -Fq '"const": "kiana.product-acceptance.v1"' docs/schemas/kiana-product-acceptance.v1.schema.json; then
  pass "product acceptance JSON schema version is pinned"
else
  fail "product acceptance JSON schema is missing kiana.product-acceptance.v1 const"
fi

if grep -Fq '"allOf"' docs/schemas/kiana-product-acceptance.v1.schema.json &&
  grep -Fq '"const": "accepted"' docs/schemas/kiana-product-acceptance.v1.schema.json &&
  grep -Fq '"minItems": 1' docs/schemas/kiana-product-acceptance.v1.schema.json; then
  pass "product acceptance accepted-state schema contract is pinned"
else
  fail "product acceptance accepted-state schema contract is not pinned"
fi

if grep -Fq '"const": "kiana.platform-security-proof.v1"' docs/schemas/kiana-platform-security-proof.v1.schema.json; then
  pass "platform security proof JSON schema version is pinned"
else
  fail "platform security proof JSON schema is missing kiana.platform-security-proof.v1 const"
fi

if grep -Fq '"const": "accepted"' docs/schemas/kiana-platform-security-proof.v1.schema.json &&
  grep -Fq '"doctor_status"' docs/schemas/kiana-platform-security-proof.v1.schema.json &&
  grep -Fq '"linux_bwrap"' docs/schemas/kiana-platform-security-proof.v1.schema.json &&
  grep -Fq '"windows_exec_policy"' docs/schemas/kiana-platform-security-proof.v1.schema.json &&
  grep -Fq '"macos_exec_policy"' docs/schemas/kiana-platform-security-proof.v1.schema.json; then
  pass "platform security accepted-state schema contract is pinned"
else
  fail "platform security accepted-state schema contract is not pinned"
fi

if grep -Fq '"const": "kiana.remote-code-session-smoke.v1"' docs/schemas/kiana-remote-code-session-smoke.v1.schema.json; then
  pass "remote code-session smoke JSON schema version is pinned"
else
  fail "remote code-session smoke JSON schema is missing kiana.remote-code-session-smoke.v1 const"
fi

if grep -Fq '"const": "kiana.release-signature.v1"' docs/schemas/kiana-release-signature.v1.schema.json; then
  pass "release signature JSON schema version is pinned"
else
  fail "release signature JSON schema is missing kiana.release-signature.v1 const"
fi

if grep -Fq '"verification"' docs/schemas/kiana-release-signature.v1.schema.json &&
  grep -Fq 'KIANA_SIGNATURE_VERIFY_COMMAND' scripts/sign-release-artifacts.sh &&
  grep -Fq 'KIANA_SIGNATURE_VERIFY_COMMAND' scripts/verify-commercial-release-artifacts.sh; then
  pass "release signature verification command is enforced"
else
  fail "release signature verification command is not enforced"
fi

if grep -Fq '"const": "kiana.macos-notarization.v1"' docs/schemas/kiana-macos-notarization.v1.schema.json; then
  pass "macOS notarization JSON schema version is pinned"
else
  fail "macOS notarization JSON schema is missing kiana.macos-notarization.v1 const"
fi

if grep -Fq '"notarization_id"' docs/schemas/kiana-macos-notarization.v1.schema.json &&
  grep -Fq '"authority"' docs/schemas/kiana-macos-notarization.v1.schema.json &&
  grep -Fq '"additionalProperties": false' docs/schemas/kiana-macos-notarization.v1.schema.json; then
  pass "macOS notarization commercial schema contract is pinned"
else
  fail "macOS notarization commercial schema contract is not pinned"
fi

if grep -Fq '"const": "kiana.release-ops.v1"' docs/schemas/kiana-release-ops.v1.schema.json; then
  pass "release ops JSON schema version is pinned"
else
  fail "release ops JSON schema is missing kiana.release-ops.v1 const"
fi

if grep -Fq '"allOf"' docs/schemas/kiana-release-ops.v1.schema.json &&
  grep -Fq '"const": "accepted"' docs/schemas/kiana-release-ops.v1.schema.json &&
  grep -Fq '"artifact_retention_days"' docs/schemas/kiana-release-ops.v1.schema.json &&
  grep -Fq '"credential_review"' docs/schemas/kiana-release-ops.v1.schema.json; then
  pass "release ops accepted-state schema contract is pinned"
else
  fail "release ops accepted-state schema contract is not pinned"
fi

if grep -Fq 'KIANA_RELEASE_BASE_URL=https://github.com/${GITHUB_REPOSITORY}/releases/download/${release_tag}' .github/workflows/release.yml; then
  pass "release workflow derives package manifest URLs from the active repository and tag"
else
  fail "release workflow does not derive KIANA_RELEASE_BASE_URL from the active repository and tag"
fi

if grep -Fq 'release-artifacts/**' .github/workflows/release.yml; then
  pass "GitHub release upload includes recursive artifacts"
else
  fail "GitHub release upload does not include recursive manifest/compliance artifacts"
fi

package_lifecycle_line="$(grep -n 'scripts/package-lifecycle-smoke.sh' .github/workflows/release.yml | head -n 1 | cut -d: -f1 || true)"
sign_artifacts_line="$(grep -n 'scripts/sign-release-artifacts.sh' .github/workflows/release.yml | head -n 1 | cut -d: -f1 || true)"
if [[ -n "$package_lifecycle_line" && -n "$sign_artifacts_line" && "$package_lifecycle_line" -lt "$sign_artifacts_line" ]]; then
  pass "release workflow validates package lifecycle before signing"
else
  fail "release workflow does not run package lifecycle smoke before signing"
fi

if grep -Fq 'dist/proofs/**' .github/workflows/release.yml &&
  grep -Fq 'KIANA_LIVE_SMOKE_DIR: dist/proofs/live-smoke' .github/workflows/release.yml &&
  grep -Fq 'KIANA_ENTITLEMENT_PROOF_OUT: dist/proofs/entitlement/entitlement-proof.json' .github/workflows/release.yml &&
  grep -Fq 'KIANA_PRODUCT_ACCEPTANCE_OUT: dist/proofs/product/product-acceptance.json' .github/workflows/release.yml &&
  grep -Fq 'KIANA_RELEASE_OPS_OUT: dist/proofs/release-ops/release-ops.json' .github/workflows/release.yml &&
  grep -Fq 'KIANA_PLATFORM_SECURITY_PROOF_OUT: dist/proofs/platform-security/platform-security-${{ runner.os }}.json' .github/workflows/release.yml; then
  pass "release workflow preserves live, entitlement, product, ops, and platform proof artifacts"
else
  fail "release workflow does not preserve live/entitlement/product/ops/platform proof artifacts"
fi

stage_proofs_line="$(grep -n 'stage-commercial-release-proofs.sh' .github/workflows/release.yml | head -n 1 | cut -d: -f1 || true)"
verify_artifacts_line="$(grep -n 'verify-commercial-release-artifacts.sh' .github/workflows/release.yml | head -n 1 | cut -d: -f1 || true)"
if [[ -n "$stage_proofs_line" && -n "$verify_artifacts_line" && "$stage_proofs_line" -lt "$verify_artifacts_line" ]]; then
  pass "commercial proof staging runs before artifact verification"
else
  fail "release workflow does not stage commercial proof manifest before artifact verification"
fi

if grep -Fq 'verify-commercial-release-artifacts.sh' .github/workflows/release.yml; then
  pass "GitHub release draft is gated by commercial artifact verification"
else
  fail "GitHub release workflow does not verify commercial artifacts before draft release"
fi

if grep -Fq 'sign-release-artifacts.sh' .github/workflows/release.yml &&
  grep -Fq 'KIANA_SIGNING_COMMAND' .github/workflows/release.yml &&
  grep -Fq 'KIANA_SIGNATURE_VERIFY_COMMAND' .github/workflows/release.yml; then
  pass "release workflow has explicit artifact signing and signature verification steps"
else
  fail "release workflow does not run explicit artifact signing and signature verification"
fi

if grep -Fq 'KIANA_RELEASE_SIGNER is required' scripts/sign-release-artifacts.sh &&
  grep -Fq 'external-release-signer' scripts/sign-release-artifacts.sh; then
  pass "release signing requires a reviewed signer identity before proof generation"
else
  fail "release signing does not require a reviewed signer identity before proof generation"
fi

if grep -Fq 'dist/*.signature.json' .github/workflows/release.yml &&
  grep -Fq 'dist/*.notarization.json' .github/workflows/release.yml &&
  grep -Fq 'dist/*.sig' .github/workflows/release.yml; then
  pass "release workflow preserves signing and notarization proof artifacts"
else
  fail "release workflow does not upload signing/notarization proof artifacts"
fi

if grep -Fq 'KIANA_RELEASE_SIGNING_CONFIRMED' .github/workflows/release.yml; then
  fail "release workflow still accepts manual signing confirmation instead of artifact proof"
else
  pass "release workflow does not accept manual signing confirmation"
fi

if [[ "$mode" == "full" ]]; then
  expected_tag="v${workspace_version}"
  if git rev-parse --verify HEAD >/dev/null 2>&1; then
    pass "git HEAD exists"
  else
    fail "git HEAD does not exist; create an initial reviewed commit before release"
  fi

  if git remote get-url origin >/dev/null 2>&1 || [[ -n "$(git remote)" ]]; then
    pass "git remote is configured"
  else
    fail "no git remote configured"
  fi

  if [[ -z "$(git status --porcelain --untracked-files=no)" ]]; then
    pass "tracked worktree is clean"
  else
    fail "tracked worktree has uncommitted changes"
  fi

  if git tag --points-at HEAD | grep -Fxq "$expected_tag"; then
    pass "HEAD is tagged with ${expected_tag}"
  else
    fail "HEAD is not tagged with ${expected_tag}"
  fi

  if [[ -z "${KIANA_RELEASE_TAG:-}" || "${KIANA_RELEASE_TAG}" == "$expected_tag" ]]; then
    pass "release tag matches VERSION"
  else
    fail "KIANA_RELEASE_TAG=${KIANA_RELEASE_TAG} does not match expected ${expected_tag}"
  fi

  if "$bash_bin" scripts/provider-live-smoke.sh --required; then
    pass "provider live smoke passed"
  else
    fail "provider live smoke failed"
  fi

  if "$bash_bin" scripts/remote-live-smoke.sh --required; then
    pass "remote live smoke passed"
  else
    fail "remote live smoke failed"
  fi

  if "$bash_bin" scripts/product-acceptance-report.sh full; then
    pass "product acceptance gate passed"
  else
    fail "product acceptance gate failed"
  fi

  if "$bash_bin" scripts/entitlement-proof-report.sh full; then
    pass "entitlement proof gate passed"
  else
    fail "entitlement proof gate failed"
  fi

  if "$bash_bin" scripts/release-ops-report.sh full; then
    pass "release ops gate passed"
  else
    fail "release ops gate failed"
  fi

  if "$bash_bin" scripts/platform-security-proof-report.sh full; then
    pass "platform security proof gate passed"
  else
    fail "platform security proof gate failed"
  fi

  pass "full release signing/channel proof is enforced by release artifact verification"
else
  if KIANA_PRODUCT_ACCEPTANCE_ALLOW_LOCAL_RC_GATE_SKIP="${KIANA_PRODUCT_ACCEPTANCE_ALLOW_LOCAL_RC_GATE_SKIP:-1}" "$bash_bin" scripts/product-acceptance-report.sh --local-rc; then
    pass "local RC product acceptance report generated"
  else
    fail "local RC product acceptance report failed"
  fi

  if "$bash_bin" scripts/entitlement-proof-report.sh --local-rc; then
    pass "local RC entitlement proof report generated"
  else
    fail "local RC entitlement proof report failed"
  fi

  if "$bash_bin" scripts/release-ops-report.sh --local-rc; then
    pass "local RC release ops report generated"
  else
    fail "local RC release ops report failed"
  fi

  if "$bash_bin" scripts/platform-security-proof-report.sh --local-rc; then
    pass "local RC platform security proof report generated"
  else
    fail "local RC platform security proof report failed"
  fi

  pass "local RC mode: git remote, HEAD, clean tracked tree, and signing checks skipped"
fi

if "$bash_bin" scripts/release-signature-verification-smoke.sh; then
  pass "commercial release verifier smoke passed"
else
  fail "commercial release verifier smoke failed"
fi

if (( failures > 0 )); then
  echo "release preflight failed with ${failures} issue(s)" >&2
  exit 1
fi

echo "release preflight passed"
