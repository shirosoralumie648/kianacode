# PD-08 EventStore Integrity Scan Baseline

## Scope

PD-08 adds a read-only `scan_jsonl` report and health gate over the existing JSONL loader. Empty,
ready, corrupt and unknown outcomes are distinct; corrupt reports require quarantine/reconciliation,
unknown reports pause/reconcile, and neither path is returned as an empty healthy store. The scan
does not delete or move a journal; quarantine remains an explicit operator/recovery action.

## Evidence and limits

- `kiana-eventlog/tests/pd08_integrity_scan.rs` covers empty/ready/checksum-corrupt reports and
  health-gate behavior.
- `kiana-core/tests/pd08_integrity_guard.rs` protects the read-only/no-destructive-repair boundary.
  GitHub Actions runs fixtures; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: no automatic quarantine move,
full middle-frame recovery, projector health gate, kill-9 recovery or physical durability proof is
claimed; those remain PD-09+/PD-30.
