# AUT-07 workflow queue claim baseline

AUT-07 now derives `WorkflowQueueClaimContract` from the same `WorkPacket` and
`ready_packets` predicate used by the packet board. Packet identity, project/data/path scope,
budget lease/deadline, canonical path-lock set, parent-child containment and claim expiry are
server-derived; missing parent scope/budget, cycles, unresolved dependencies, scope/budget
widening, duplicate claims and expired claims fail closed.

`kiana-core` exposes a deterministic queue-ready view with one claim per item and a digest-bound
blocked/expired projection. This remains a pure source/CI fixture slice: AUT-08 still owns durable
QueueStore leases, heartbeat, fencing, reclaim and recovery; no queue or scheduler was started by
this step.

## 2026-10-05 focused local receipt

The claim fixture uses `active_claim_count > max_parallel_claims` for an unambiguous overflow;
equal-to-limit admission remains allowed by the pure contract. The stale guard marker was aligned
with the baseline's `source/CI` wording.

```text
source_snapshot: current dirty worktree; domain workflow queue claim, Core ready view/transaction and source guard
worktree_status: roadmap row 276 remains 🔄; fixture/guard-only correction, no durable queue or scheduler path
command_argv: bounded serial cargo test -p kiana-domain --test aut07_workflow_queue_claim --locked --offline -- --test-threads=1; cargo test -p kiana-core --test aut07_workflow_queue_ready --locked --offline -- --test-threads=1; cargo test -p kiana-core --test aut07_workflow_queue_claim_guard --locked --offline -- --test-threads=1; cargo test -p kiana-core --test cp24_scheduler_transaction --locked --offline -- --test-threads=1
cwd·environment: repository root Linux/bash; one Cargo/test worker, 32 MiB test stack, stripped/non-incremental profile; domain max RSS 83948 KiB, Core ready 84996 KiB, guard 129712 KiB, CP-24 84660 KiB, swaps 0
fixture·cassette: domain 3/3; ready view 3/3; source guard 1/1; scheduler transaction 2/2 covering stale snapshot/owner/fence/expiry denial
exit_code: all commands 0; logs/resources `/tmp/kiana-roadmap-20261005-evidence/aut07-{domain,core,guard,cp24}.{log,resources}`
status_change: selected AUT-07 contracts are local_behavior; whole card remains feature_status=partial, proof_level=source
proof-level_change: selected fixture/guard/transaction source → local_behavior; no durable/live/physical promotion
limitations: active count is caller-derived, no atomic scheduler capacity or EventLog-backed QueueStore/lease/fence/reclaim/recovery; equal-limit semantics and Continue integration remain separate contracts
reviewer: integration agent with AUT-07 read-only audit; no human release reviewer
```
