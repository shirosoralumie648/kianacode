#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

mode="${1:-full}"
if [[ "$mode" != "full" && "$mode" != "--local-rc" ]]; then
  echo "usage: $0 [--local-rc]" >&2
  exit 2
fi

failures=0

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
  docs/schemas/kiana-doctor.v1.schema.json \
  scripts/release-smoke.sh scripts/package-release.sh scripts/install-release-binary.sh \
  scripts/generate-sbom.sh scripts/compliance-audit.sh scripts/install-compliance-tools.sh \
  scripts/generate-distribution-manifests.sh \
  .github/workflows/release-smoke.yml .github/workflows/release.yml
do
  require_file "$file"
done

if [[ "${KIANA_PREFLIGHT_SKIP_COMPLIANCE:-}" == "1" ]]; then
  pass "compliance audit skipped by KIANA_PREFLIGHT_SKIP_COMPLIANCE=1"
elif [[ "$mode" == "full" ]]; then
  if bash scripts/compliance-audit.sh; then
    pass "full compliance audit passed"
  else
    fail "full compliance audit failed"
  fi
else
  if bash scripts/compliance-audit.sh --local-rc; then
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

if grep -Fq '"const": "kiana.doctor.v1"' docs/schemas/kiana-doctor.v1.schema.json; then
  pass "doctor JSON schema version is pinned"
else
  fail "doctor JSON schema is missing kiana.doctor.v1 const"
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

  if [[ "${KIANA_RELEASE_SIGNING_CONFIRMED:-}" == "1" ]]; then
    pass "release signing confirmation present"
  else
    fail "release signing is not confirmed; set KIANA_RELEASE_SIGNING_CONFIRMED=1 only after signing is active"
  fi
else
  pass "local RC mode: git remote, HEAD, clean tracked tree, and signing checks skipped"
fi

if (( failures > 0 )); then
  echo "release preflight failed with ${failures} issue(s)" >&2
  exit 1
fi

echo "release preflight passed"
