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

grep -Fq 'cargo test --workspace --locked --no-fail-fast' .github/workflows/ci.yml
grep -Fq '  push:' .github/workflows/ci.yml
grep -Fq '  pull_request:' .github/workflows/ci.yml
grep -Fq 'node --test contrib/desktop/tests/*.js' .github/workflows/ci.yml
grep -Fq "find scripts -type f -name '*.sh' -exec bash -n {} +" .github/workflows/ci.yml
grep -Fq 'bash scripts/validate-sc42-recovery-rehearsal.sh' .github/workflows/ci.yml
grep -Fq 'bash scripts/validate-sc43-security-closeout.sh' .github/workflows/ci.yml
grep -Fq 'bash scripts/validate-pd35-persistence-closeout.sh' .github/workflows/ci.yml
grep -Fq 'cancel-in-progress: true' .github/workflows/ci.yml
