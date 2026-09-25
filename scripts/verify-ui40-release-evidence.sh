#!/usr/bin/env bash
set -euo pipefail

if [[ "${GITHUB_ACTIONS:-}" != "true" ]]; then
  printf 'ui40_release_evidence:remote_ci_required\n' >&2
  exit 2
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
required_files=(
  "docs/roadmap/ui-entrypoints.md"
  "docs/roadmap/ui40-release-gate-baseline.md"
  "docs/roadmap/ui39-live-acp-baseline.md"
  "docs/module-map.md"
  "CURRENT_STATUS.md"
  "kiana-protocol/src/ui_contracts.rs"
  "kiana-protocol/tests/ui40_release_evidence.rs"
  "kiana-core/tests/ui40_release_gate_guard.rs"
  "kiana-client/tests/ui38_conformance.rs"
  "kiana-client/tests/ui39_live_acp_opt_in.rs"
)

for relative in "${required_files[@]}"; do
  [[ -f "${root}/${relative}" ]] || {
    printf 'ui40_release_evidence:missing:%s\n' "${relative}" >&2
    exit 1
  }
done

for marker in \
  'UI-32' \
  'UI-33' \
  'UI-34' \
  'UI-38' \
  'UI-39' \
  'UiEvidenceCase' \
  'UiEvidenceBundle' \
  'source_snapshot' \
  'fixture' \
  'exit_code' \
  'proof_level' \
  'limitations'; do
  grep -Fq "${marker}" "${root}/docs/roadmap/ui40-release-gate-baseline.md" || {
    printf 'ui40_release_evidence:baseline_marker_missing:%s\n' "${marker}" >&2
    exit 1
  }
done

if grep -Fq '#### UI-40 · 发布门与证据收口　⏳' "${root}/docs/roadmap/ui-entrypoints.md"; then
  printf 'ui40_release_evidence:roadmap_still_pending\n' >&2
  exit 1
fi

for forbidden in 'all UI cards complete' 'live verified' 'physical verified'; do
  if grep -Fq "${forbidden}" "${root}/docs/roadmap/ui40-release-gate-baseline.md"; then
    printf 'ui40_release_evidence:overclaim:%s\n' "${forbidden}" >&2
    exit 1
  fi
done

printf 'schema=kiana.ui40-release-evidence-gate.v1 proof_level=source status=remote_ci_gate\n'
