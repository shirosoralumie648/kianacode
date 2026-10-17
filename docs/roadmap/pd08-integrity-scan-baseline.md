# PD-08 EventStore Integrity Scan Baseline

## Scope

PD-08 adds a read-only `scan_jsonl` report, a tail-recovery report and a fail-closed health gate
over the existing JSONL loader. Empty, ready, recovered-tail, corrupt and unknown outcomes are
distinct, and the four store classes the card requires — empty store, repairable tail, corrupt
middle frame and unknown format — land on four different statuses. Corrupt reports require
quarantine/reconciliation, unknown reports pause/reconcile, recovered reports record the repaired
tail and continue, and none of them is returned as an empty healthy store.

The scan does not delete, move or rebuild a journal: the loader keeps its existing torn-tail policy
(single framing authority) and reports what it repaired through `JournalRecovery`; quarantine
remains an explicit operator/recovery action.

`kiana-daemon::StorageIntegrityGate` is the daemon-side admission surface: it inspects the facts
journal, exposes the recovery report, and refuses to let an unhealthy store proceed to execution
(`admit_execution`). It is read-only and creates no second execution path.

## Evidence and limits

- `kiana-eventlog/tests/pd08_integrity_scan.rs` covers empty/ready/repaired-tail/corrupt reports
  (the unknown schema/required/writer-version record yields Unknown) with health-gate behavior for
  every class, and asserts the four statuses are pairwise distinct.
- `kiana-daemon/tests/pd08_integrity_gate.rs` covers the same four classes through
  `StorageIntegrityGate`: corrupt and unknown block execution with stable codes, empty/ready/
  recovered are admitted, and the gate never rewrites a corrupt or unknown file in place.
- `kiana-core/tests/pd08_integrity_guard.rs` protects the read-only/no-destructive-repair boundary
  and the daemon admission markers.
- `.github/workflows/pd08-integrity-scan.yml` runs the eventlog, daemon and Core targets; local
  tests are intentionally not executed in this CI-only session.

This slice is `feature_status=implemented`, `proof_level=source`: no automatic quarantine move,
full middle-frame repair, projector health gate, kill-9 recovery or physical durability proof is
claimed; those remain PD-09+/PD-30.
