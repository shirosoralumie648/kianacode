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
  docs/schemas/kiana-doctor.v1.schema.json docs/schemas/kiana-model-smoke.v1.schema.json \
  docs/schemas/kiana-model-catalog.v1.schema.json \
  docs/schemas/kiana-license-status.v1.schema.json \
  docs/schemas/kiana-enterprise-offline-manifest.v1.schema.json \
  scripts/release-smoke.sh scripts/package-release.sh scripts/install-release-binary.sh \
  scripts/package-lifecycle-smoke.sh scripts/product-shell-smoke.sh \
  scripts/provider-live-smoke.sh scripts/remote-live-smoke.sh \
  scripts/verify-commercial-release-artifacts.sh \
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

if grep -Fq 'provider-live-smoke.sh' RELEASE.md &&
  grep -Fq 'remote-live-smoke.sh' RELEASE.md &&
  grep -Fq 'provider-live-smoke.sh --required' scripts/release-preflight.sh &&
  grep -Fq 'remote-live-smoke.sh --required' scripts/release-preflight.sh; then
  pass "live provider and remote smoke gates are documented and wired into full preflight"
else
  fail "live provider and remote smoke gates are not fully documented or wired"
fi

if grep -Fq '"const": "kiana.doctor.v1"' docs/schemas/kiana-doctor.v1.schema.json; then
  pass "doctor JSON schema version is pinned"
else
  fail "doctor JSON schema is missing kiana.doctor.v1 const"
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

if grep -Fq 'verify-commercial-release-artifacts.sh' .github/workflows/release.yml; then
  pass "GitHub release draft is gated by commercial artifact verification"
else
  fail "GitHub release workflow does not verify commercial artifacts before draft release"
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

  pass "full release signing/channel proof is enforced by release artifact verification"
else
  pass "local RC mode: git remote, HEAD, clean tracked tree, and signing checks skipped"
fi

if (( failures > 0 )); then
  echo "release preflight failed with ${failures} issue(s)" >&2
  exit 1
fi

echo "release preflight passed"
