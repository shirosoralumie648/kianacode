# CP-21 projection / Receipt / read-query baseline

## Scope

CP-21 closes the read side of the ControlPlane after CP-19 fact rebuild.  The source guard and
CI fixture pin one reducer/cursor contract for Run, Invocation and capability-attempt projections;
read-only reconstruction from committed EventLog facts; owner-scoped pending-approval, history,
UI and Receipt queries; and typed Receipt fields for decision, attempt, effect, verification and
source/reconciliation evidence.

## Evidence gate

- `cp_query_changes_neither_ledger_nor_execution` checks that projection/history/Receipt reads
  rebuild from committed facts, pause on cursor or terminal contradictions, and do not append,
  issue permits or call handlers.
- `cp_fresh_projection_matches_live_receipt` checks that a cache miss uses the same source fold as
  the live Receipt and that source cursor/event IDs, attempt/effect state, digests and redaction
  metadata remain bound to the typed contract.
- `cp_cross_project_query_cannot_disclose_pending_or_receipt` checks owner/project/actor filters,
  revoked-data behavior and redacted pending/Receipt wire requests.

The GitHub workflow runs the existing projection, invocation, Receipt and checkpoint fixtures,
then the CP-21 source guard and workspace test-target compilation.  Local runtime tests are not
run for this step.

## Limits

This is source-level evidence only.  It does not claim a durable projection database, cross-host
query consistency, live provider correctness, or physical storage proof.  CP-22 owns the broader
wire/action-card parity; later PD/ER steps own durable read-model adapters, pagination and crash
rebuild matrices.
