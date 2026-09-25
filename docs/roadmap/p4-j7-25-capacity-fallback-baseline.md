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
