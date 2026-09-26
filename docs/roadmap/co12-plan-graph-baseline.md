# CO-12 Plan and WorkGraph proposal baseline

## Scope

CO-12 adds a versioned, data-only Plan proposal contract on top of the existing Company
Milestone/WorkPacket and packet DAG contracts. A plan carries a frozen Charter baseline version,
coverage digest, typed Milestone/Packet nodes and explicit parent/child, artifact, run-success,
acceptance and related edges.

## Implemented source slice

- `kiana-domain/src/plan.rs` defines `PlanProposal`, `PlanNode`, `PlanEdge` and deterministic
  `topological_order()` with bounded sizes, stable digest, project/version binding and strict
  unknown-field rejection.
- Validation rejects missing endpoints, cross-project nodes, duplicate edges/nodes, malformed
  references, invalid parent/child kinds, dependency cycles and isolated required nodes. Related
  edges remain informational and do not affect the blocking DAG.
- The contract reuses `CriterionId` for typed coverage references and records the approved Charter
  baseline and coverage digest. It does not start runs, claim packets, consume budgets or create a
  second scheduler.

## CI-only evidence

`.github/workflows/co12-plan-graph.yml` runs formatting, the domain Plan fixtures, the Core source
guard and domain/core/daemon test-target compilation on GitHub Actions. Local Cargo tests,
builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- This slice is the typed proposal/validation contract. Existing Company commands and the
  Business `PublishPlan` adapter are not yet switched to persist/consume `PlanProposal`; that
  admission and migration belongs to later integration steps.
- Dependency references are typed and bounded strings; artifact/acceptance truth, durable plan
  storage, cross-process recovery and live scheduling remain unproven.
- No model, broker, source-code write, external effect, durable, live or physical outcome is
  claimed.
