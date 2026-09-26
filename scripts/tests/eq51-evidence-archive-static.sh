#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DOMAIN="$ROOT/kiana-domain/src/quality_evidence_archive.rs"
CORE="$ROOT/kiana-core/src/quality_evidence_archive.rs"
WORKFLOW="$ROOT/.github/workflows/eq51-evidence-archive.yml"

test -s "$DOMAIN"
test -s "$CORE"
test -s "$WORKFLOW"

for marker in \
  QUALITY_EVIDENCE_ARCHIVE_SCHEMA \
  QualityEvidenceArchive \
  QualityEvidenceBlock \
  QualityArchiveArtifactKind \
  QualityReport \
  QualityEvidenceManifest \
  QUALITY_REPRODUCTION_SCHEMA \
  source_snapshot \
  worktree_status \
  command_argv \
  cwd_environment \
  fixture_cassette \
  exit_code \
  status_change \
  proof_level_change \
  limitations \
  reviewer
do
  rg -q "$marker" "$DOMAIN"
done

for artifact in report trace-diff evidence reproduction
do
  rg -q "$artifact" "$WORKFLOW"
done

rg -q 'upload-artifact@v4' "$WORKFLOW"
rg -q 'if-no-files-found: warn' "$WORKFLOW"
rg -q 'permissions:' "$WORKFLOW"
rg -q 'contents: read' "$WORKFLOW"

for forbidden in \
  'CapabilityBroker::new' \
  'ModelClient::new' \
  'std::process::Command' \
  'tokio::spawn' \
  'std::fs::' \
  'publish_report' \
  'write_artifact'
do
  if rg -q "$forbidden" "$DOMAIN" "$CORE"; then
    echo "EQ-51 forbidden effect marker: $forbidden" >&2
    exit 1
  fi
done

echo "EQ-51 evidence archive static guard passed"
