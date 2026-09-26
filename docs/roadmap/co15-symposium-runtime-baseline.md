# CO-15 bounded symposium runtime baseline

## Scope

CO-15 records the finite runtime admission boundary for the existing `ControlPlane::convene_symposium`
path. Each role keeps its own session, contributions remain structured domain facts, and round,
message, token, wall-time and stall limits are explicit. Anti-meeting and convened paths share the
same later decision gate; a budget exhaustion or cancellation cannot become an approved decision.

## Implemented source slice

- `SymposiumRunBudget` validates and accounts round/message/token/stall limits plus cancellation;
  `decision_allowed()` is fail-closed at exhaustion or cancellation.
- Existing symposium code and ControlPlane remain the only meeting/model path. The source guard
  binds the new budget contract to the existing `convene_symposium` and anti-meeting fixtures,
  without adding a second runner or broker.

## CI-only evidence

`.github/workflows/co15-symposium-runtime.yml` runs formatting, domain runtime fixtures, the Core
source guard and domain/core/daemon test-target compilation on GitHub Actions. Local Cargo tests,
builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- The budget value object is not a live scheduler/clock and does not prove provider usage or
  external effects. Existing Harness/ControlPlane runtime fixtures remain the behavior authority.
- Structured contribution validation, durable board/restart replay, UI projection, live model
  calls and physical outcomes remain open. No live/durable/physical claim is made.
