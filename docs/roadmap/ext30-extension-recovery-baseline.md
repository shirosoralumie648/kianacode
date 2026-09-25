# EXT-30 · Extension crash recovery baseline

This source slice records the recovery decision that a fresh process or operator must make after
an extension boundary is interrupted. It is not a store reopen implementation and does not retry
an unknown effect.

## Source contract

- `ExtensionRecoveryMatrix` covers package-before-rename, rename-before-event,
  event-before-projection, upgrade switch, Hook running, approval expiry and Broker result unknown.
- Package material is quarantined before rename; an idempotent command may retry only before an
  effect starts and only with its original idempotency/fence digests. Event-before-projection and
  upgrade-switch cases rebuild registry/snapshot state from committed evidence.
- Approval expiry waits for a new approval. Hook-running and Broker-result-unknown cases become
  explicit unknown reconciliation; they cannot automatically retry a Hook or repeat a possible
  external effect. Duplicate effects are rejected by the case contract.
- The contract reuses existing extension lifecycle/state/command receipt and Core resume fences;
  it does not read EventLog, open a package, execute a callback or mutate registry state.

## CI-only fixtures

`.github/workflows/ext30-extension-recovery.yml` runs formatting, the domain crash/recovery matrix,
the Core source guard and workspace test-target compilation. Its path filter includes current CM-36
`kiana-domain/src/memory_workbench.rs` so the repository-wide formatting dependency is included in
a fresh remote run. Local tests, builds, checks, clippy and smoke commands are deliberately not
run, and GitHub CI is not awaited.

## Evidence ceiling and limitations

The slice is `feature_status=partial` with `proof_level=source` plus CI wiring. It does not prove
fresh-process EventLog/registry/snapshot reopen, durable lease/fence CAS, package quarantine on
disk, approval projector recovery, Hook process cleanup, Broker result reconciliation, external
effect truth, live behavior or physical evidence. A recovery decision matrix is not a durable
recovery run.
