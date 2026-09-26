# CO-11 typed criterion coverage baseline

## Scope

CO-11 adds a versioned, typed coverage graph around the existing CO-06 `Criterion` contract.
Project requirements can be explicitly covered/refined by Milestone and Packet criteria, and a
specific typed artifact/event/test reference can verify a criterion. Stable IDs and origin
versions remain distinct even when descriptions are identical.

## Implemented source slice

- `kiana-domain/src/criteria.rs` defines `CriterionCoverageGraph`, typed relation links
  (`covers`, `refines`, `verifies`), scope/baseline binding, deterministic digest, duplicate/
  missing-parent/cross-level/weak-child/cycle/unknown-evidence rejection, explicit pending
  required criteria, and deterministic trace/gap accessors.
- The graph reuses CO-06 `Criterion` and `CriterionId`; it does not create another criterion
  authority, EventStore, planner, evaluator or execution loop.
- `CriteriaSnapshot::criteria()` now uses typed `criterion_refs` IDs whenever they are present and
  only falls back to legacy text lists when the typed list is empty. Old replay payloads remain
  readable while text deduplication cannot merge distinct typed criteria.
- The Core source guard and GitHub workflow cover the deny-first fixture and target compilation.

## CI-only evidence

`.github/workflows/co11-criteria-coverage.yml` runs formatting, the domain coverage fixtures, the
Core source guard and domain/core/daemon test-target compilation on GitHub Actions. Local Cargo
tests, builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- The graph is a domain contract and source guard in this slice. Existing Company business
  PublishPlan and acceptance paths retain their legacy `BusinessCriterion`/text adapters; full
  ControlPlane plan publication, typed snapshot persistence, review evidence integration and
  migration of every historical plan remain later work.
- A `verifies` link carries bounded typed reference strings, not a live test runner or artifact
  blob lookup; external evidence truth, durable cross-process recovery and semantic sufficiency
  still require independent planning/review evidence.
- No live model, source-code write, external effect, durable, live or physical proof is claimed.
