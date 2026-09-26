# EQ-46 version drift metrics baseline

## Scope

EQ-46 consumes the existing read-only `DriftReport` version buckets and derives bounded error-rate
and mean-latency metrics. Explicit minimum-sample and threshold fields produce a stable
`DriftAlert` and `drift.alerted` event payload when a bucket exceeds a limit. Route and grant
snapshots must be identical before and after evaluation; alerts carry
`authority_changes_applied=false`.

`version.drift` remains a query-only projection. The Core adapter returns evidence and does not
append an EventLog fact, switch a route, mutate a grant, or dispatch a provider.

## Evidence and limits

- `kiana-domain/tests/quality_drift.rs` covers threshold alerting, event identity, minimum-sample
  review, route/grant drift rejection, digest drift and strict unknown fields.
- `kiana-core/tests/quality_drift_guard.rs` protects the metric/alert contract and no-effect route
  and grant boundary.
- `.github/workflows/eq46-drift.yml` runs fixtures, source guard, formatting and workspace
  test-target compilation in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=partial`, `proof_level=source`: no online drift stream, durable
alert append/replay, automatic rollback, route/grant mutation, provider request, live quality or
business outcome is claimed.
