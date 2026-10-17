# CAP-23 concurrency, conflict and fair-admission baseline

CAP-23 closes the missing admission boundary between an authorized request and the existing
Broker. `CapabilityAdmissionScheduler` is bounded (eight active admissions and 256 queued),
FIFO-fair for conflicting work, and process-local. It never invokes a handler or starts another
model loop.

Read footprints may run together. Writes use the server-derived `ExecutionScope.write_roots`, so
shell/process calls lock their authorized write scope instead of guessing a file list from command
syntax. Overlapping read/write or write/write scopes conflict; unknown or external effects use a
wildcard conservative footprint. Existing CellRegistry admission still owns budget reservations,
Cell concurrency, parent cancellation and Cell lifecycle; existing descriptor-relative OS locks
cover cross-process workspace conflicts.

Approval waiting happens before this gate, so an approval request does not hold an execution slot.
Cancellation removes a queued admission and returns `cancelled:before_dispatch`/`not_executed`;
the Runner's pending queue and existing cancellation facts prevent a later spawn. A completed or
known failed invocation releases its footprint. `Unknown` moves the footprint to an explicit local
quarantine, and the packet Cell remains non-retirable until reconciliation/retirement can prove a
safe release.

## Behavioural fixtures

The admission fixtures live in the `#[cfg(test)] mod tests` of
`kiana-core/src/capability_scheduler.rs`; the budget fixtures live in the `#[cfg(test)] mod tests`
of `kiana-core/src/cell_registry.rs`. Both drive the production types directly (no test doubles):
the scheduler through `acquire()`/`release()` with real `tokio::sync::watch` cancellation channels,
and the registry through `resolve_template` → `reserve_spawn` → `commit_spawn` →
`transition_cell(Ready, Running)` → `begin_capability`/`finish_capability` with the same Domain
value objects (`CapabilityGrant`, `BudgetLease`, `SupervisionLease`, `CellSpec`, `SpawnPlan`) that
the swarm composition builds. The scheduler fixtures construct a synthetic `ExecutionScope` only
to derive a footprint; it never passes the Domain/Broker authorization validators, so these
fixtures cannot be cited as authorization evidence.

Deny-first fixtures, in the order the focused lane runs them:

| fixture | what it proves |
|---|---|
| `overlapping_write_scopes_never_execute_concurrently` | a `shell.exec` write whose only footprint is the authorized `ExecutionScope.write_roots=["src"]` holds the slot; an `apply_patch` to `src/lib.rs` and a `connector.invoke` `Unknown` effect both stay pending (asserted against the shared state, not only by timing) and are admitted only in release order — the lock comes from the server-derived scope, not from parsing the command text, and overlapping writes never run together |
| `cancelled_queued_call_never_spawns` | a queued overlapping call whose cancellation watch fires leaves the queue, returns `PortError::Conflict("cancelled:before_dispatch")` and never appears in the active set; releasing the holder does not revive the cancelled entry |
| `pre_cancelled_requests_never_enter_the_queue_or_take_a_slot` | an already-cancelled request returns the same structured conflict before enqueue; the same scope remains usable by a later non-cancelled request |
| `cell_registry::tests::parallel_calls_cannot_overspend_parent_budget` | one write consumes the lease's only effect; a parallel write is refused with `budget_lease_exceeded` before consuming anything (`effects_used`/`tool_calls_used` stay 1, one active lease); finishing it with `Failed` releases the concurrency slot but does not refund the effect, so a third write is still refused; a read-only call (zero effects) still fits the remaining tool-call budget |
| `read_only_requests_share_and_disjoint_writes_run_concurrently` | two reads of the same path and a write to a disjoint root are active at the same time (three slots); a write overlapping an active read subtree stays pending until the reads release |
| `unknown_result_quarantines_its_scope_from_successors` | releasing with `AdmissionOutcome::Unknown` moves the footprint to `quarantined` with zero active slots; an overlapping successor stays pending and is never admitted, while a disjoint read is still admitted — quarantine is scope-bound, not a global stop |
| `released_or_rejected_admissions_clean_up_their_slot_and_queue_is_bounded` | a known result frees the slot for the same scope; with `MAX_PENDING_ADMISSIONS` waiters queued, the next `acquire()` fails with `capability_scheduler_queue_full` instead of growing the queue |
| `cell_registry::tests::cell_concurrency_and_duplicate_fences_share_one_lease` | with `max_concurrency = 1` the same `request_id` is refused with `cell_capability_invocation_duplicate` and a different request with `cell_capability_concurrency_exceeded`; after `finish_capability` the second request is admitted, so the concurrency slot is genuinely released and the duplicate fence is per-lease |

Focused `--exact` filters `capability_scheduler::tests::<name>` and `cell_registry::tests::<name>`
are the executable receipt for each row.

## CI wiring

`.github/workflows/cap23-concurrency-scheduler.yml` keeps the existing Cell lifecycle and quota
guards and adds the in-module runs (`cargo test -p kiana-core --lib capability_scheduler::tests::`
and `--lib cell_registry::tests::`), a per-fixture `--exact` loop in deny-first order, and
`--no-run` compilations. `kiana-core`'s unified `ci.yml` shards run explicit integration-test
targets only, so they cannot execute `--lib` fixtures; the same two `--lib` commands therefore also
run as the automatic `cap23-concurrency-fixtures` job in `.github/workflows/ci.yml`.
`kiana-core/tests/cap23_concurrency_scheduler_guard.rs` adds
`cap23_behavioural_fixtures_are_wired_and_deny_first`: every fixture name must exist in the
production module, in the focused lane and in this baseline; no fixture may be `#[ignore]`d; the
deny-first fixtures must precede the concurrency regressions in the lane; the write footprint must
not read `request.arguments`; the dispatch path must acquire admission before the Cell lease; and
the approval staging path must not hold an admission slot.

## Evidence and limitations

- Proof level: `source`. The fixtures are compiled and run only in CI; no local runtime tests or
  smoke commands were run while authoring them, and no local `cargo test` execution is claimed.
- In-module fixtures cover the process-local scheduler and Cell registry only. They do not invoke a
  handler, write EventLog facts, prove `durable` recovery, or exercise cross-process workspace locks
  (that is the existing `acquire_durable_path_locks` flock guard in
  `cap23_concurrency_scheduler_guard.rs`).
- The scheduler's `ExecutionScope` is synthesized for footprint derivation and is not validated by
  Domain/Broker, so these fixtures are not authorization evidence.
- The quarantine has no removal API in the current module; the fixture asserts the blocking
  behaviour of the current implementation, not reconciliation.
- Cancellation evidence here is the watch-based admission cancellation; Runner-side queued tool
  cancellation stays covered by the existing guard marker `cancelled:queued`.
- The unknown-result fixture aborts its still-queued waiter after the assertion; dropping an
  `acquire()` future leaves its queue entry, which is a documented scheduler boundary, not a claim
  about memory reclamation.
