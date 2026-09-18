# CP-17 Revocation / Cleanup / Cell Retirement Baseline

## Scope

CP-17 closes the cleanup boundary after stop evidence. Trust/data/grant revocation first blocks
new dispatch and stops affected runs; parent cancellation propagates to swarm descendants. Cell
cleanup is separated into stop, release, settle and retire: CellRegistry verifies no active
capability, releases only its own budget/path/grant/supervision resources once, and emits a
retirement receipt. Unknown execution keeps resources quarantined; resource projection rejects
authority rebuild while quarantine remains. Human failure/reconciliation records preserve the
original incident, require evidence, forbid automatic retry and allow a new request only after
explicit release.

## Evidence and limits

- `kiana-core/tests/p1_c02_cell_lifecycle.rs` pins commit/transition/retire/idempotent resource
  release and own-path ownership markers.
- `kiana-core/tests/p2_k6_01_reliability.rs`, `p2_k7_01_data_governance.rs` and
  `p4_j6_01_bounded_swarm.rs` pin quarantine/reconciliation, revoke propagation and descendant
  cancellation boundaries.
- `kiana-core/tests/cp17_revoke_cleanup_guard.rs` adds one cross-layer source guard; GitHub Actions
  runs fixtures, guard and workspace compile; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: durable crash-reentrant cleanup
work items, cross-process lease recovery, external compensation and live/physical proof remain
CP-18+ / ER / SW work.
