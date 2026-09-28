#!/usr/bin/env bash
# Fails when a roadmap/status document points at a repository file that does not exist.
#
# Why this exists: CI was consolidated by 08552ada, which deleted 664 per-step
# workflow files. Evidence blocks written before that still name them, so the
# ledger went on asserting gates that no longer run. Rewriting those blocks
# would falsify what was true at the commit each one describes, so they are
# recorded as historical and pinned in the exemption list instead. Anything not
# on that list is a live claim about the current tree and must resolve.
#
# Resolution rules, so the shorthand used throughout the roadmap is not mistaken
# for a broken link:
#   dir/file.rs   -> dir/file.rs, dir/src/file.rs, dir/tests/file.rs
#   file.rs       -> a basename match anywhere in the workspace (module shorthand)
#   a/b/c.yml     -> resolved from the repository root only
set -Eeuo pipefail

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

EXEMPTIONS="scripts/ci/doc-reference-exemptions.txt"

mapfile -t docs < <(
  printf '%s\n' CURRENT_STATUS.md docs/roadmap.md
  find docs/roadmap -maxdepth 1 -name '*.md' -print | sort
)

# Basename index so `actions.rs` resolves as the module shorthand it is. The index
# comes from `git ls-files`, not from a filesystem walk: stale agent checkouts under
# .claude/worktrees hold source files that were never merged, and indexing those let a
# reference resolve against a file master does not have. An associative array keeps the
# lookup O(1); a shell loop over every file made the first run take minutes.
declare -A BASENAMES=()
while IFS= read -r name; do
  BASENAMES["${name##*/}"]=1
done < <(git ls-files)

declare -A EXEMPT_SET=()
if [ -f "$EXEMPTIONS" ]; then
  while IFS= read -r line; do
    case "$line" in ''|'#'*) continue ;; esac
    EXEMPT_SET["$line"]=1
  done < "$EXEMPTIONS"
fi

resolves() {
  local ref="$1"
  if [ -e "$ref" ]; then
    return 0
  fi
  if [[ "$ref" != */* ]]; then
    [ -n "${BASENAMES[$ref]:-}" ]
    return
  fi
  local dir="${ref%/*}" base="${ref##*/}"
  [ -e "$dir/src/$base" ] || [ -e "$dir/tests/$base" ]
}

status=0
checked=0
missing=0

for doc in "${docs[@]}"; do
  [ -f "$doc" ] || continue
  checked=$((checked + 1))
  while IFS= read -r ref; do
    [ -n "$ref" ] || continue
    resolves "$ref" && continue
    [ -n "${EXEMPT_SET["$doc :: $ref"]:-}" ] && continue
    echo "doc reference does not resolve: $doc -> $ref" >&2
    status=1
    missing=$((missing + 1))
  done < <(
    grep -oE '`[A-Za-z0-9_./-]+\.(rs|sh|yml|yaml|toml|json|html|js)`' "$doc" \
      | tr -d '`' | sort -u
  )
done

if [ "$status" -ne 0 ]; then
  echo "doc reference check failed: $missing unresolved reference(s) in $checked document(s)" >&2
  echo "fix the reference, or record it in $EXEMPTIONS when it is a historical claim" >&2
  exit 1
fi

echo "doc reference check passed: $checked document(s) checked, all references resolve"
