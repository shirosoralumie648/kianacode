# EQ-30 evidence and receipt evaluator baseline

## Scope

EQ-30 adds `EvidenceReceiptEvaluator` to `kiana-quality`. The typed evidence envelope binds
artifact references and strict SHA-256 digests, receipt digest/cursor/artifact links, required
receipt assertions, redaction/secret-free status, provenance and source cursor/event IDs. Missing
artifacts, hash drift, unverified receipts, assertion gaps, redaction failure, provenance drift or
cursor mismatch emit stable blocking findings.

The evaluator consumes caller-supplied evidence only. It does not read artifact or receipt stores,
recompute EventLog facts, fetch provider receipts, execute a capability, or make a release or
authorization decision.

## Evidence and limits

- `kiana-quality/tests/eq30_evidence.rs` covers a complete evidence bundle, missing artifact
  blocking, digest/redaction/provenance/cursor/assertion drift, unknown fields and bounds.
- `kiana-quality/tests/eq30_evidence_guard.rs` protects the pure fail-closed boundary and required
  artifact/receipt/redaction/provenance/cursor markers.
- `.github/workflows/eq30-evidence-receipt.yml` runs fixtures, source guard, formatting and
  workspace test-target compilation in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: it does not claim durable
ArtifactStore/Receipt truth, real provider receipts, cross-process recovery, quality aggregation,
promotion authority, live or physical evidence. Later evaluator/gate and ER/PD/SC steps remain open.
