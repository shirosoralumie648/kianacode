# PD-12 EventStore Approval Authority Baseline

## Scope

PD-12 closes the product authority boundary for approvals: `DaemonHost` injects
`JournalApprovalStore`, which folds the shared `EventStorePort` approval stream and commits
stage/activate/decision/consumption transitions with aggregate/authority CAS. Legacy
`MemoryApprovalStore`/JSONL compatibility code remains only as an explicitly non-product migration
and test adapter; the daemon must not instantiate it as authority. Legacy records without the
required EventStore facts require reauthorization.

The step does not remove compatibility code, rewrite historical facts, or claim cross-process
durability beyond the EventStore adapter's own proof level. Approval still never grants Broker
execution directly; dispatch rechecks the prepared consumption/read set.

## Evidence and limits

- `kiana-daemon/tests/pd12_eventstore_approval_guard.rs` protects product wiring, transition
  methods, legacy reauthorization and no-direct-execution boundaries.
- `.github/workflows/pd12-eventstore-approval.yml` runs the source guard and workspace compilation
  in GitHub Actions; local tests are intentionally not executed.
- Existing CP-10 approval-fact fixtures remain the behavioral contract for proof/nonce/epoch/CAS,
  while PD-12 records their EventStore ownership handoff.

This slice is `feature_status=implemented`, `proof_level=source`: product source wiring and CI
guard are present; legacy compatibility migration, durable cross-process crash evidence and live
human approver/provider effects are not claimed.
