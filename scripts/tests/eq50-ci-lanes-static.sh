#!/usr/bin/env bash
set -Eeuo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
cd -- "$ROOT"

files=(
  .github/workflows/eq50-pr-curated.yml
  .github/workflows/eq50-nightly-deep.yml
  .github/workflows/eq50-release-candidate.yml
)
failures=0
fail() {
  echo "eq50-ci-lanes: $*" >&2
  failures=$((failures + 1))
}

for file in "${files[@]}"; do
  [[ -f "$file" ]] || { fail "missing $file"; continue; }
  grep -Fq 'permissions:' "$file" || fail "$file lacks permissions block"
  grep -Fq 'contents: read' "$file" || fail "$file lacks read-only contents permission"
  grep -Fq 'scripts/tests/eq49-eval-scripts-static.sh' "$file" || fail "$file lacks EQ-49 static guard"
  grep -Fq 'cargo fmt --all --check' "$file" || fail "$file lacks CI formatting gate"
  grep -Fq -- '--test-threads=1' "$file" || fail "$file lacks serial test flag"
  if grep -Eiq 'ANTHROPIC_API_KEY|OPENAI_API_KEY|GOOGLE_API_KEY|AZURE_OPENAI|AWS_SECRET|BEARER_TOKEN|HTTP_PROXY|HTTPS_PROXY|ALL_PROXY|MCP_CONFIG' "$file"; then
    fail "$file mentions ambient provider, proxy or secret variables"
  fi
  if grep -Eiq 'continue-on-error:[[:space:]]*true|permissions:[[:space:]]*write' "$file"; then
    fail "$file weakens a release/CI gate"
  fi
done

grep -Fq 'workflow_dispatch:' .github/workflows/eq50-pr-curated.yml || fail 'curated lane dispatch missing'
if grep -Fq 'pull_request:' .github/workflows/eq50-pr-curated.yml; then
  fail 'curated lane must not duplicate the unified PR CI'
fi
grep -Fq 'schedule:' .github/workflows/eq50-nightly-deep.yml || fail 'nightly lane schedule missing'
grep -Fq "github.event_name == 'schedule'" .github/workflows/eq50-nightly-deep.yml || fail 'nightly schedule is not enabled automatically'
grep -Fq 'workflow_dispatch:' .github/workflows/eq50-release-candidate.yml || fail 'release lane dispatch missing'
grep -Fq 'release_candidate' .github/workflows/eq50-release-candidate.yml || fail 'release lane opt-in missing'
grep -Fq 'deep_enabled' .github/workflows/eq50-nightly-deep.yml || fail 'nightly explicit gate missing'
grep -Fq 'curated' .github/workflows/eq50-pr-curated.yml || fail 'curated lane marker missing'
grep -Fq 'deep' .github/workflows/eq50-nightly-deep.yml || fail 'deep lane marker missing'
grep -Fq 'release' .github/workflows/eq50-release-candidate.yml || fail 'release lane marker missing'

if ((failures > 0)); then
  echo "eq50-ci-lanes: $failures failure(s)" >&2
  exit 1
fi
echo "eq50-ci-lanes: static guards passed"
