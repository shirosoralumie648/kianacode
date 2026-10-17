# P4-J7-25 · Provider capacity, circuit breaker and fallback admission baseline

This source slice closes the provider capacity boundary before the request is sent. A connection
now has a bounded waiter pool, shared by aliases that resolve to the same provider/origin/
credential scope. A circuit breaker is server-owned state and only reports admission; it never
authorizes a capability or silently changes the route.

## Source contract

- `ProviderCapacityPolicy` bounds concurrency, queued waiters, failure threshold and cooldown.
- `ProviderCircuitBreaker` implements closed/open/half-open transitions and admits one half-open
  probe. Typed provider rejection/transport failures may open it; capacity-full, auth and unknown
  external outcomes are not silently retried. A cancelled or unclassified half-open probe is
  reopened for a fresh cooldown; it is never silently treated as success.
- `FallbackRoutePlan` and `admit_fallback` require a fresh exact capability, data-scope and
  budget digest match. A fallback cannot reuse the primary route's permit or private context.
- `ProviderGateway` shares capacity, queue slots and breaker state for connection aliases. The
  transport takes a bounded queue slot before waiting for the semaphore and releases the slot when
  capacity is acquired or the attempt is cancelled.
- Queue entries retain the request's absolute deadline and its digest. Dispatch rejects expired
  entries with `provider_capacity_queue_wait_expired` instead of extending the deadline from the
  dispatch clock; expired entries never receive a provider-capacity lease.
- Breaker failure observation computes the next failure count and cooldown deadline before
  mutating state. An `open_until` arithmetic overflow therefore returns
  `provider_circuit_open_until_overflow` without leaving a stale health digest or half-open state.

## CI-only fixtures

`.github/workflows/p4-j7-25-capacity-fallback.yml` runs domain transition/fallback fixtures, a
Core source guard, formatting and workspace test-target compilation on GitHub Actions. Its path
filter includes the current CM-36 `kiana-domain/src/memory_workbench.rs` module so a fresh remote
run can clear the historical repository-wide fmt dependency; that result is pending/unobserved.
Local tests, builds, checks, clippy and smoke commands are intentionally not run.

The fixture catalog covers queue limits, typed circuit transitions, one half-open probe, alias
sharing, abandoned-probe cooldown recovery, provider cancellation-guard drop and fallback
capability/data/budget drift. It does not open a provider or external route. GitHub CI also runs
the provider library target so the cancellation guard fixture is executed remotely.

## Evidence ceiling and limitations

The slice is `feature_status=implemented` with `proof_level=source` plus CI wiring. The cancellation
guard only repairs the in-process breaker state and does not reconcile whether a provider request
was dispatched. It does not claim a durable cross-process circuit store, distributed fair queue,
provider invoice/capacity telemetry, automatic route selection, external/live provider effects or
physical proof. Any future fallback executor must return through ControlPlane, create a new attempt
and independently reserve usage and authority.

The overflow regression first reproduced a half-updated breaker: `observe_failure(u64::MAX)` changed
state before failing its cooldown addition, leaving `validate()` unable to accept the object. The
fixed path computes all fallible values first, then commits state and digest together. The focused
regression and full P4-J7-25 domain target (4/4) pass locally; this remains local behavior evidence
for an in-process boundary only.

## 2026-10-08 absolute queue deadline evidence

The local RED fixture showed that dispatch rebuilt a queued request deadline from
`now + max_queue_wait_ms`, allowing work whose original absolute deadline had already elapsed.
`ProviderCapacityQueueEntry` now retains and digests `deadline_unix_ms`; dispatch preserves that
deadline and returns a typed `provider_capacity_queue_wait_expired` rejection without a lease.
The focused regression, the BQ-16 domain target (4/4), and the P4-J7-25 domain target (4/4) pass
with a single Cargo process and no swap. This is local behavior evidence only; the capacity and
breaker state remains process-local and no provider/external effect was exercised.

## 2026-10-17 completion slice (CI-only)

Two card items were previously evidenced only by source markers; this slice adds executable
behavior fixtures and binds them into the guard and the manual lane. No production behavior
changed beyond extracting the existing alias fold into `share_capacity_gates` (a pure,
behavior-identical refactor so the fold can be driven directly).

- **Config-update reset semantics (card step ③)**:
  `config_update_resets_derived_health_before_any_new_admission` drives
  `ProviderCircuitBreaker::reset_for_config`. An empty or oversized revision is refused with
  `provider_circuit_config_revision_invalid` without touching state or the health digest (the
  breaker still rejects with `provider_circuit_open`), while a valid revision resets to `Closed`,
  clears `failure_count`/`open_until_unix_ms`/`half_open_probe_in_flight`, recomputes the digest
  and only then admits again. A typed failure after the reset still only moves derived health.
- **Alias quota sharing (exit evidence)**:
  `connection_aliases_roll_up_to_one_bounded_gate_per_upstream_identity` (crate-internal
  `kiana-provider/src/config.rs` fixture) builds three resolved connections differing only in
  model name and credential reference, runs the real fold and asserts `Arc::ptr_eq` for
  `capacity_policy`/`capacity_window`/`capacity`/`queue_slots`/`circuit` across the two aliases of
  one credential, that the shared pool holds exactly `max_concurrency`/`max_queue` permits (three
  aliases cannot multiply the bound), and that a different credential reference stays a separate
  bucket. The fixture reads no environment variable, opens no socket and consumes no permit.

CI wiring: `.github/workflows/p4-j7-25-capacity-fallback.yml` (manual `workflow_dispatch`,
`permissions: contents: read`) runs `cargo fetch --locked`, `cargo fmt --all --check`, the domain
fixture target, the provider boundary target, both provider lib fixture groups
(`circuit_tests`, `capacity_alias_tests`) and the Core guard, all `--locked --test-threads=1`. The
automatic lane stays the unified `ci.yml` shards `kiana-domain-s3/4`, `kiana-provider`
(whole crate, `targets=null`) and `kiana-core-s5/6`. The Core guard binds the new markers
(`reset_for_config`, `share_capacity_gates`), both fixture names and the new lane step.

Local tests/build/check/clippy/smoke were not run (CI-only session; `cargo fmt --all` only), so the
proof ceiling stays `source` and the CI receipt is not observed. Durable cross-process capacity or
breaker storage, provider capacity/invoice telemetry, automatic route selection, a real fallback
executor (which must re-enter ControlPlane, create a new attempt and independently reserve usage
and authority) and live/physical provider evidence remain open.
