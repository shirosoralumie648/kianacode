# CO-09 Objective and Initiative intake baseline

## Scope

CO-09 strengthens the existing Company command and replay path for Objective and Initiative
intake. Objective activation requires a metric measurement method, finite improving target and
owner decision. Initiative submission requires the actor to be its sponsor and all referenced
objectives to belong to the same organization; approval requires active objectives and conversion
preserves objective ancestry.

## Implemented source slice

- `Objective` now carries an optional legacy compatible `measurement_method`; approval requires a
  non-empty method, finite values, a non-reversing target direction and a valid time window.
- `SubmitInitiative` rejects sponsor mismatch and cross-organization objective references.
  `AdvanceInitiative` requires active same-organization objectives for approval and checks project
  organization/objective ancestry before conversion. Existing status transitions keep duplicate
  conversion rejected and replay semantics unchanged.
- The existing `ControlPlane::handle_company_command` remains the Company entrypoint, with the
  policy and idempotency gates in front of the domain transition. CI fixtures cover deny paths and
  the approved intake boundary.

## CI-only evidence

GitHub Actions runs the CO-09 domain fixtures, Company source guard, formatting and workspace
test-target compilation. Local Cargo test/build/check/clippy/smoke commands are intentionally not
run and CI is not awaited.

## Evidence boundary and limitations

```text
feature_status: partial
proof_level: source
```

This slice does not create a new durable Objective/Initiative store or project Charter/go/no-go
flow. Organization identity and sponsor authority are still supplied by the existing ControlPlane
context; live business outcome measurement and later Company steps remain open.
