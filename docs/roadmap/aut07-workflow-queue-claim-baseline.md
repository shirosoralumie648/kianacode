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

## 2026-10-17 completion slice (CI-only)

Row 276's five deny branches already had stable-code fixtures
(`workflow_queue_path_lock_digest_mismatch`, `workflow_queue_claim_expired`,
`workflow_queue_parent_budget_required`, `workflow_queue_budget_subset_invalid`,
`workflow_queue_ready_duplicate_item`). This slice closes the remaining card acceptance at
source level:

- `kiana-core/tests/aut07_workflow_queue_ready.rs` gains
  `ready_view_matches_ready_packets_for_the_same_snapshot`: for one packet snapshot the queue
  ready view's ready/blocked/expired projection equals `ready_packets()`, a dependency-blocked
  child stays out of the claims, and every ready claim re-derives the parent-child
  scope/budget/path-lock digests through `validate_packet_binding`.
- `expired_packet_claim_is_reclaimable_but_active_claim_is_never_readied`: an active packet claim
  stays blocked as `packet_claimed` (one item, one effective claim) and never appears in
  ready/expired; an expired claim is surfaced in `expired_claims` and becomes claimable again.
- `lease_expired_beyond_the_packet_deadline_cannot_cross_the_gate`:
  `workflow_queue_deadline_exceeded` — a lease with correct owner/fence/epoch but expiry later
  than the packet deadline is denied, while a lease inside the deadline is dispatchable.
- `kiana-core/tests/aut07_workflow_queue_claim_guard.rs` adds a wiring guard binding every named
  fixture and the six stable codes to `.github/workflows/aut07-workflow-queue-claim.yml`, which
  now also runs each named deny-first fixture with `--exact`.

The contract stays an in-memory projection: no QueueStore, durable lease, heartbeat, fencing,
reclaim or scheduler start is added here. Those remain AUT-08 work; proof_level stays `source`
(CI-only session, receipts not awaited).

```text
source_snapshot: docs/roadmap.md §1.1 row 276 (🔄 → ✅, CI pending) and §3 changelog (2026-10-17 `AUT-07` row); kiana-core/tests/aut07_workflow_queue_ready.rs (+3 fixtures, imports); kiana-core/tests/aut07_workflow_queue_claim_guard.rs (+aut07_named_stable_codes_are_wired_into_the_claim_lane); .github/workflows/aut07-workflow-queue-claim.yml (+2 named `--exact` fixture steps); this baseline; existing kiana-domain contract and fixtures unchanged
worktree_status: row 276 🔄 → ✅ (CI pending); only AUT-07 files touched plus single-point shared-doc backfill; no Cargo.toml/Cargo.lock/package.json change, no new test target, no new dependency, no second execution loop (claim/lease stay pure values; still the DaemonHost→ControlPlane→Broker spine)
command_argv: CI-only; no local cargo/make/npm/smoke run; only rg/cat/sed/ls/git status/diff static checks; wiring is `.github/workflows/aut07-workflow-queue-claim.yml` (`cargo test -p kiana-domain --test aut07_workflow_queue_claim`, `cargo test -p kiana-core --test aut07_workflow_queue_ready`, `cargo test -p kiana-core --test aut07_workflow_queue_claim_guard`, plus per-fixture `--exact --test-threads=1` runs); unified `ci.yml` shards `kiana-domain-s1/4` and `kiana-core-s1/6` claim the three targets
cwd·environment: worktree /workspace/kianacode-worktrees/AUT-07, branch step/aut-07-20261017-1 (base master abd9c25c2487); GitHub Actions ubuntu-latest is the runtime (receipt not awaited)
fixture·cassette: new core fixtures drive `ready_packets()` and `project_workflow_queue_ready()` on the same in-memory BTreeMap, and `validate_workflow_queue_transaction()` with directly issued `WorkflowQueueLease` values; no QueueStore, no EventLog write, no scheduler
exit_code: not run locally; CI receipt not awaited
status_change: card success column "ready_packets、queue ready view、parent-child intersection 一致；一个 item 只有一个有效 claim" gains tree-level fixtures; deny branches keep their stable codes; row 276 🔄 → ✅ (CI pending)
proof-level_change: stays source; not promoted to local_behavior/durable/live/physical
limitations: no local compile/run; durable QueueStore/lease/heartbeat/fence/reclaim/recovery, atomic capacity accounting and cross-process/restart behavior remain AUT-08 (or later); the gate is not an atomic CAS and inputs are caller-supplied snapshots
reviewer: root integration agent; static review of the consistency/expiry/deadline fixtures, wiring guard and workflow backfill
```
