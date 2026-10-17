# CP-24 scheduler, WorkPacket, delegation and Workflow baseline

## Scope

CP-24 adds the transaction gate between the deterministic WorkPacket ready
projection and a worker lease.  `WorkflowQueueClaimContract::validate_packet_binding`
recomputes the immutable packet, parent scope, budget and path-lock digests and
rejects a parent mismatch or any child widening.  The core
`validate_workflow_queue_transaction` gate then requires the claim to be present
in the exact ready snapshot, binds the lease to that claim, checks packet
deadline, owner, fence token and authority epoch, and rejects expiry before the
existing ControlPlane command path is reached.

Workflow planning remains pure and produces a committed intent before an
AgentTask or capability route is considered.  Cell delegation still reserves
the parent-contained grant, budget and path lock through `CellRegistryPort` and
commits the spawn reservation before starting the existing runner path.  The
queue gate creates no lease, calls no Broker, and starts no second model loop.

## GitHub evidence

`kiana-domain/tests/cp24_scheduler_transaction.rs` covers canonical claim
rebinding, forged digest, parent mismatch, path-scope widening and budget
widening denial.  `kiana-core/tests/cp24_scheduler_transaction.rs` covers a
valid ready claim/lease handoff plus stale snapshot, owner, fence and expiry
denials.  `cp24_scheduler_transaction_guard.rs` checks the queue, Workflow and
Cell reservation source boundaries and the absence of a second execution loop.

`.github/workflows/cp24-scheduler-workflow.yml` runs the domain/core fixtures
and workspace test-target compilation on GitHub Actions.  Local runtime tests,
builds and checks are intentionally not run for this slice.

## 2026-10-17 completion slice (CI-only)

The card's three named acceptance tests now exist as behavioural fixtures in
`kiana-core/tests/cp24_scheduler_workflow.rs`, wired into the CP-24 lane and the
unified `kiana-core-s2/6` shard:

| Acceptance | Fixture | Boundary exercised |
|---|---|---|
| `cp_two_schedulers_claim_one_packet_once` | `cp24_scheduler_workflow` | Real `MemoryWorkflowQueueStore`: two schedulers race one ready packet, exactly one lease; loser `workflow_queue_claim_contended`, winner passes `validate_workflow_queue_transaction`, loser cannot reuse the lease (`workflow_queue_owner_mismatch`), claimed item leaves `ready()`. |
| `cp_workflow_replay_never_reexecutes_completed_effect` | `cp24_scheduler_workflow` | Real `ControlPlane` + `MemoryEventLog`: `RegisterDefinition` → `Start` → `Advance` dispatches a capability effect once; the exact committed command replays with `replayed:true`, identical state and ledger; a fresh `ControlPlane` over the same ledger rebuilds that state through `workflow.snapshot.v1` without appending facts. |
| `cp_child_cannot_gain_parent_missing_scope_or_budget` | `cp24_scheduler_workflow` | `derive_swarm_child_grant` refuses a packet path the parent grant lacks (`scope_intersection_empty`); `WorkflowQueueClaimContract::from_work_packet` and the ready projection refuse widened scope, swapped/missing budget leases and later deadlines (`workflow_queue_scope_intersection_invalid` / `workflow_queue_budget_subset_invalid`) before any claim is published. |

`cp24_scheduler_transaction_guard` binds the fixture names to the lane and the
`kiana-core-s2/6` shard so a rename cannot silently drop an exit condition. The
lane additionally fetches locked dependencies, runs each acceptance fixture by
exact name and compiles every CP-24 target with `--no-run`. CI remains the test
authority and its receipt is not awaited by this branch.

## Limits

This is source plus remote-fixture wiring evidence.  The ready projection and
queue adapter remain process-local; no durable cross-process scheduler index,
timer checkpoint, worker-death recovery, external effect receipt, or
live/physical proof is claimed.  Unknown effects still require explicit
reconciliation, and future AUT-14+ work owns durable effect reservations and
worker dispatch separation.
