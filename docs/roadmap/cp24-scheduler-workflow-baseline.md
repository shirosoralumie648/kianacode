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

## Limits

This is source plus remote-fixture wiring evidence.  The ready projection and
queue adapter remain process-local; no durable cross-process scheduler index,
timer checkpoint, worker-death recovery, external effect receipt, or
live/physical proof is claimed.  Unknown effects still require explicit
reconciliation, and future AUT-14+ work owns durable effect reservations and
worker dispatch separation.
