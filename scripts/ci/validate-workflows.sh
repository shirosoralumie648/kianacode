#!/usr/bin/env bash
set -Eeuo pipefail

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

# Workflows that may run automatically on push/pull_request/schedule. Every other
# workflow must be manual (workflow_dispatch) so that adding a roadmap step never
# multiplies automatic CI fan-out. Keep this list small and explicit.
ruby -ryaml -e '
  AUTOMATIC = %w[ci.yml eq50-nightly-deep.yml eq50-release-candidate.yml release.yml sc28-supply-chain.yml]

  files = Dir[".github/workflows/*.{yml,yaml}"].sort
  abort "no workflow files found" if files.empty?

  problems = []
  automatic_seen = []

  files.each do |file|
    name = File.basename(file)
    begin
      document = YAML.safe_load_file(file, aliases: true)
    rescue StandardError => error
      problems << "#{name}: invalid YAML: #{error.message}"
      next
    end

    # YAML 1.1 parses the bare key "on" as the boolean true, so the GitHub `on:`
    # block arrives here under `true` as well as "on" depending on the loader.
    triggers_block = document["on"] || document[true]
    triggers = triggers_block.is_a?(Hash) ? triggers_block.keys.map(&:to_s) : []
    problems << "#{name}: missing an explicit permissions block" unless document.key?("permissions")
    problems << "#{name}: missing an on: trigger block" if triggers.empty?

    automatic = triggers - ["workflow_dispatch"]
    unless automatic.empty?
      automatic_seen << name
      unless AUTOMATIC.include?(name)
        problems << "#{name}: automatic trigger #{automatic.join(",")} is not registered; use workflow_dispatch"
      end
    end
  end

  missing = AUTOMATIC - automatic_seen
  problems << "registered automatic workflows lost their trigger: #{missing.join(", ")}" unless missing.empty?

  unless problems.empty?
    problems.each { |problem| warn "workflow check failed: #{problem}" }
    exit 1
  end

  puts "workflow check passed: #{files.length} files, #{automatic_seen.length} automatic, #{files.length - automatic_seen.length} manual"
'

if grep -R -n -E '^[[:space:]]*-[[:space:]]*["'"'"'](docs/roadmap\.md|CURRENT_STATUS\.md)["'"'"'][[:space:]]*$' .github/workflows \
  --exclude=ci.yml; then
  echo "feature workflows must not fan out on root roadmap/status bookkeeping" >&2
  exit 1
fi

# The unified gate used to run `cargo test --workspace` in one job. That could not finish:
# the workspace links ~1468 test binaries, which both exhausted the runner disk and blew past
# the 180-minute job limit. Tests now run as a sharded matrix, so instead of grepping for one
# command this proves the shards cover the workspace exactly: every member runs, and every
# <crate>/tests/*.rs target is claimed by exactly one shard. That is strictly stronger than
# the string it replaces, because a sharded gate that silently dropped a crate or a target
# would still contain the old grep and would have passed.
python3 - <<'PYEOF'
import json, os, re, sys

members = re.findall(
    r'"([^"]+)"',
    re.search(r'(?m)^members\s*=\s*\[(.*?)\]', open('Cargo.toml', encoding='utf-8').read(), re.S).group(1),
)
shards = json.load(open('scripts/ci/test-shards.json', encoding='utf-8'))
problems = []

by_crate = {}
for name, entry in shards.items():
    by_crate.setdefault(entry['crate'], []).append((name, entry))

for crate in members:
    if crate not in by_crate:
        problems.append(f"workspace member never tested by any shard: {crate}")
        continue
    tests_dir = os.path.join(crate, 'tests')
    actual = set()
    if os.path.isdir(tests_dir):
        actual = {f[:-3] for f in os.listdir(tests_dir) if f.endswith('.rs')}

    entries = by_crate[crate]
    # A whole-crate shard runs `cargo test -p <crate>`, which runs every target in it.
    if any(not e['targets'] for _, e in entries):
        if len(entries) > 1:
            problems.append(f"{crate} mixes a whole-crate shard with chunked shards")
        continue

    # Otherwise the crate's shards must partition its targets exactly.
    seen = {}
    for name, entry in entries:
        for t in entry['targets']:
            if t in seen:
                problems.append(f"{crate} target {t} claimed by both {seen[t]} and {name}")
            seen[t] = name
    missing = actual - set(seen)
    extra = set(seen) - actual
    if missing:
        problems.append(f"{crate}: {len(missing)} test target(s) not covered by any shard: {sorted(missing)[:5]}")
    if extra:
        problems.append(f"{crate}: {len(extra)} shard target(s) do not exist: {sorted(extra)[:5]}")

for crate in by_crate:
    if crate not in members:
        problems.append(f"shard references non-member crate: {crate}")

total = 0
for crate in members:
    tests_dir = os.path.join(crate, 'tests')
    if os.path.isdir(tests_dir):
        total += sum(1 for f in os.listdir(tests_dir) if f.endswith('.rs'))

if problems:
    for p in problems:
        print(f"test shard check failed: {p}", file=sys.stderr)
    sys.exit(1)
print(f"test shard check passed: {len(members)} members, {len(shards)} shards, {total} test targets covered")
PYEOF
grep -Fq '  push:' .github/workflows/ci.yml
grep -Fq '  pull_request:' .github/workflows/ci.yml
grep -Fq 'node --test contrib/desktop/tests/*.js' .github/workflows/ci.yml
grep -Fq "find scripts -type f -name '*.sh' -exec bash -n {} +" .github/workflows/ci.yml
grep -Fq 'bash scripts/validate-sc42-recovery-rehearsal.sh' .github/workflows/ci.yml
grep -Fq 'bash scripts/validate-sc43-security-closeout.sh' .github/workflows/ci.yml
grep -Fq 'bash scripts/validate-pd35-persistence-closeout.sh' .github/workflows/ci.yml
grep -Fq 'cancel-in-progress: true' .github/workflows/ci.yml
