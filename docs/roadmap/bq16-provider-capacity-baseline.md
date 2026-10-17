# BQ-16 · Provider capacity, RPM/TPM and bounded fair admission baseline

This source slice makes provider capacity an explicit, deny first boundary after ControlPlane
admission and before network dispatch. It binds every request to one canonical UTC quota window
and `QuotaGroupKey` (provider, credential, model and alias), and keeps queueing and backoff out of
the active provider slot.

## Source contract

- `ProviderCapacityController` returns typed `Accept`, `Queue`, `Delay` and `Reject` outcomes.
- RPM and TPM are checked against the immutable policy and UTC window. A quota window failure is
  a `Delay` with a bounded `retry_after_ms`; it never consumes an active semaphore slot.
- The queue is bounded and session-fair. Cancellation removes a queued attempt before dispatch;
  no cancelled entry is handed to transport. A full queue returns structured `Reject`.
- `ProviderCapacityLease` binds owner, attempt, group, window and policy digests. Release checks
  the exact owner, so a different session cannot return another session's permit.
- Queued entries retain the request's absolute deadline in their own digest. Dispatch rejects an
  entry whose deadline has elapsed with `provider_capacity_queue_wait_expired`; queue wait never
  extends the request deadline or creates a lease for expired work.
- Profile aliases share provider/origin/credential capacity policy. Changing alias, credential or
  model therefore cannot create a second quota group without an explicit server policy.
- Provider transport keeps a bounded waiter semaphore and releases the waiter slot before taking
  the active slot. Its shared `CapacityWindow` reserves RPM/TPM in the fixed UTC minute only after
  an active slot is available, so delayed/backpressured work does not consume quota. It only sends
  through the existing `ProviderGateway`/ControlPlane path.

## CI-only fixtures

`bq16-provider-capacity.yml` runs the domain outcome/queue/owner fixtures, provider integration
fixture, provider in-crate capacity/permit fixtures, and Core/Daemon source guards on GitHub
Actions. Local test, build, check, clippy and smoke commands are intentionally not run.

## 2026-10-17 completion slice (CI-only)

This slice closes the card's rejection surface and acceptance line in source and wires it to CI.

- Domain fixtures (`kiana-domain/tests/bq16_provider_capacity.rs`, 4 → 11, deny first):
  `accept_queue_delay_and_reject_outcomes_are_typed_and_observable` drives all four typed
  outcomes, the queue ticket/wait and `retry_after_ms` observability, the RPM backpressure
  `Delay`, and re-signed forged outcome shapes
  (`provider_capacity_accept_fields_invalid`/`_queue_fields_invalid`/`_retry_after_missing`/
  `_outcome_invalid`); `request_scope_drift_and_window_mismatch_are_refused_before_queueing`
  refuses a foreign `QuotaGroupKey` (alias/model/credential drift), a foreign UTC window, a
  pre-window clock rollback, malformed policies and a re-signed/original-digest request;
  `queue_is_bounded_fair_and_refuses_duplicate_or_unknown_entries` pins the bounded queue
  (`provider_capacity_queue_full`), duplicate attempt refusal, deny-first entry validation and the
  one-turn-per-session rotation; `cancelled_entry_is_never_dispatched_and_active_lease_is_not_cancellable`
  proves cancellation removes queued work before dispatch, is idempotent, and never cancels an
  active lease; `dispatch_backpressure_delays_without_holding_a_slot_or_losing_the_entry` proves a
  dispatch-time `delay` spends no slot/quota and keeps the entry; `queue_wait_and_tickets_stay_observable_per_session`
  pins `queue_wait_ms`/ticket observability; `lease_release_is_owner_scoped_and_usage_rejects_leaseless_release`
  pins unknown/foreign/double release, replayed/forged lease objects and `provider_capacity_release_without_permit`.
- Provider fixtures: `kiana-provider/src/capacity.rs` unit fixtures
  (`reserve_keeps_rpm_and_tpm_in_the_fixed_utc_minute_and_rolls_the_window`,
  `reserve_fails_closed_on_invalid_input_clock_rollback_and_overflow`) drive the real
  fixed-minute RPM/TPM window; `kiana-provider/src/transport.rs`
  (`capacity_permits_are_bounded_and_released_on_drop`,
  `aliased_connections_cannot_double_spend_the_shared_capacity_budget`) prove the bounded waiter
  and active semaphores, RAII release on drop, and that one shared upstream identity cannot gain a
  second budget through an alias. The integration fixture
  (`kiana-provider/tests/bq16_provider_capacity.rs`) pins the gate ordering (waiter → active →
  waiter release → window reserve), the absence of `tokio::spawn`/unbounded semaphores and the
  lane/shard wiring.
- Guards: `kiana-core/tests/bq16_provider_capacity_guard.rs` pins the marker set, the refusal-code
  matrix, all 11 domain + 6 provider fixture names and the manual lane/auto-shard wiring;
  `kiana-daemon/tests/bq16_provider_capacity_guard.rs` keeps the daemon free of capacity ownership
  and binds its lane step.
- Manual lane: `.github/workflows/bq16-provider-capacity.yml` (rebuilt after the workflow
  consolidation) is `workflow_dispatch`-only with `permissions: contents: read`, a
  `cargo fetch --locked` + `cargo fmt --all --check` gate, five fixture/guard targets with
  `--test-threads=1` and five `--no-run` compile steps. The automatic lane remains the unified
  `ci.yml`: `kiana-domain-s1/4` (`bq16_provider_capacity`), `kiana-core-s1/6`
  (`bq16_provider_capacity_guard`) and the whole-crate `kiana-provider`/`kiana-daemon` shards
  (`targets: null`, so the new in-crate and integration fixtures are discovered automatically).
  `scripts/ci/test-shards.json` and `Cargo.toml`/`Cargo.lock`/`package.json` were not modified.

## Evidence ceiling and limitations

The slice is `feature_status=implemented` with `proof_level=source` and remote CI wiring. The
capacity controller is an in-process admission contract; a durable cross-process RPM/TPM CAS
store, provider-side capacity receipts, live external requests, and physical proof remain open.
ControlPlane remains the only authorization authority and this slice adds no provider execution
loop or fallback executor.

Known limits of the 2026-10-17 slice:

- Local test/build/check/clippy/smoke commands were not run in this session (only `cargo fmt
  --all`); every behavioral receipt depends on GitHub Actions and the CI receipt was not awaited.
- The controller/queue and `CapacityWindow` remain process-local value contracts. `dispatch_next`
  re-enqueues a delayed entry and assigns a new ticket, so tickets are admission-order evidence,
  not a durable queue identity; cancelled/expired entries are dropped, not persisted.
- The provider semaphores bound in-process waiters only; they do not persist queue/cancellation
  facts and are not a cross-process RPM/TPM CAS.
- `CapacityWindow` reserves after the active slot is taken (transport order), so a request that
  waits for an active slot does not spend quota; a fixed-window boundary can still admit up to two
  windows' worth of traffic across the minute edge (upstream 429 remains the authority).
- The daemon owns scheduling and delegation only; it never owns capacity admission or dispatch.
- The delivery merge, push and pull-request gating are the scheduler's responsibility; this slice
  only commits locally.

## 2026-10-08 local deadline evidence

The focused regression first reproduced the bug: a queued request with absolute deadline `60_003`
was accepted when dispatched at `60_100` because dispatch rebuilt a fresh deadline from the current
time. The fix stores `deadline_unix_ms` in `ProviderCapacityQueueEntry`, includes it in the entry
digest, and uses it for the typed expired rejection. The focused test, all four BQ-16 domain tests,
and all three P4-J7-25 capacity/fallback domain tests pass locally. This advances only the selected
boundary to `proof_level=local_behavior`; the controller remains process-local and the step remains
🔄.
