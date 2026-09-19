# PD-16 Receipt Recompute / Evidence Graph Baseline

## Scope

PD-16 closes the read-side receipt/evidence boundary: receipt aggregation is recomputed from
owner-scoped committed facts with source cursor/event IDs, execution/effect/usage verification,
evidence/provider receipt digests and explicit partial/unknown states. Company delivery/closing
commands collect artifact/evidence references from server state, validate immutable ArtifactRef
content/scope before acceptance, and write closing artifacts only through the existing controlled
path.

Transcript, cache, model self-report or a missing artifact cannot independently produce a success
receipt. This step does not add a second fact source, delivery worker, or external outcome claim.

## Evidence and limits

- `kiana-core/tests/pd16_receipt_evidence_guard.rs` protects fact-only receipt aggregation,
  source/evidence/artifact reference binding and no-model/no-Broker authority.
- `.github/workflows/pd16-receipt-evidence.yml` runs the source guard and workspace compilation in
  GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: receipt/evidence source boundary
is guarded, while dedicated evidence graph storage, durable artifact bytes, delivery reconciliation
and external/live outcome proof remain open.
