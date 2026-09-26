#!/usr/bin/env bash
set -Eeuo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
cd -- "$ROOT"

files=(
  scripts/eval-curated.sh
  scripts/eval-deep.sh
  scripts/eval-capture-golden.sh
  scripts/eval-compare.sh
)
failures=0
fail() {
  echo "eq49-eval-scripts: $*" >&2
  failures=$((failures + 1))
}

for file in "${files[@]}"; do
  [[ -f "$file" ]] || { fail "missing $file"; continue; }
  [[ -x "$file" ]] || fail "$file is not executable"
  [[ "$(head -n 1 "$file")" == '#!/usr/bin/env bash' ]] || fail "$file has no bash shebang"
  grep -Fq 'set -Eeuo pipefail' "$file" || fail "$file lacks strict shell mode"
  bash -n "$file" || fail "$file has invalid shell syntax"
  for required in \
    'env -i' \
    'umask 077' \
    '"PATH=/usr/local/bin:/usr/bin:/bin"' \
    '"HOME=$run_dir/home"' \
    '"KIANA_HOME=$run_dir/kiana-home"' \
    '"TMPDIR=$run_dir/tmp"' \
    '"LC_ALL=C"' \
    '"LANG=C"' \
    'reject_secret_or_path' \
    'ControlPlane'; do
    grep -Fq "$required" "$file" || fail "$file lacks required guard: $required"
  done
  if grep -Eiq 'ANTHROPIC_API_KEY|OPENAI_API_KEY|GOOGLE_API_KEY|AZURE_OPENAI|AWS_SECRET|BEARER_TOKEN|HTTP_PROXY|HTTPS_PROXY|ALL_PROXY|NO_PROXY|MCP_CONFIG|KIANA_PROVIDER' "$file"; then
    fail "$file mentions an ambient provider, proxy, secret, or MCP variable"
  fi
  if grep -Eiq '(^|[[:space:]])(cargo|curl|wget|python|python3|node)([[:space:]]|$)|kiana-runner|kiana-capability-broker' "$file"; then
    fail "$file contains a second evaluator or direct external command"
  fi
done

grep -Fq 'args=(eval run' scripts/eval-curated.sh || fail 'curated script does not call eval.run'
grep -Fq 'args=(eval run' scripts/eval-deep.sh || fail 'deep script does not call eval.run'
grep -Fq 'args=(eval capture' scripts/eval-capture-golden.sh || fail 'capture script does not call eval.capture'
grep -Fq 'args=(eval compare' scripts/eval-compare.sh || fail 'compare script does not call eval.compare'

if grep -Eiq -- 'eval[[:space:]]+run[^\n]*(--suite[ =]|--baseline[ =])' scripts/eval-curated.sh scripts/eval-deep.sh; then
  fail 'curated/deep scripts use legacy filesystem suite or baseline options'
fi

if ((failures > 0)); then
  echo "eq49-eval-scripts: $failures failure(s)" >&2
  exit 1
fi

echo "eq49-eval-scripts: static guards passed"
