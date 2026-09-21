# PD-21 Cache policy baseline

## Scope

PD-21 separates cache state from facts and projections. A cache can be reused only when the
source is unchanged; miss, stale, invalid, degraded or disabled states remain visible and cannot
be converted into an empty business result.

## Implemented source slice

- `ContextCacheDecision` provides typed Hit/Miss/Stale/Degraded/Disabled states with a stable digest.
- Only unchanged reuse is allowed to supply a cached business result; all other states rebuild,
  report degradation or remain non-authoritative.
- `ContextIndexCacheReport` carries the decision while the existing persistent-index path continues
  to distinguish invalid-cache recovery from a successful cache hit.
- Query and daemon source guards keep cache handling outside ControlPlane and provider authority.

## Evidence boundary

GitHub Actions is the test authority for the focused query fixtures and daemon source guard. Local
tests are intentionally not run. This step does not claim cross-process cache leases, health
projection, eviction SLAs, durable deletion propagation or live/physical effects.
