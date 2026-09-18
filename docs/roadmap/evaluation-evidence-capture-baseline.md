# EQ-14 Evaluation Evidence Capture Baseline

## Scope

EQ-14 adds `EvalEvidenceCapture`, a bounded projection collector for one evaluation target. It
records committed `RuntimeEvent` values, opaque invocation/artifact/receipt references and an
optional typed `CommandReceipt`; it never appends facts or becomes an EventLog authority. Events
are size/sequence/duplicate/secret checked before capture, and references are bounded and
redaction-safe.

`finish(Ok(()))` produces a validated flushed capture receipt. Any flush error produces an explicit
`infra_flush_unknown` / `InfraUnknown` receipt, preventing a missing flush acknowledgement from
being reported as an evaluation pass. Once finished, the collector is closed against late writes.

## Evidence and limits

- `kiana-daemon/tests/eq14_evidence_capture.rs` covers event/reference/command-receipt collection,
  secret rejection, closed-state behavior and flush-failure Unknown classification.
- `kiana-core/tests/eq14_evidence_capture_guard.rs` protects the reference-only/no-second-fact-
  source boundary. GitHub Actions runs the fixtures; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: it does not yet attach to a live
DaemonHost/EventStore observer, persist artifacts or receipts, or implement EQ-15 fault/restart
plans and durable quality results. No live/physical effect proof is claimed.
