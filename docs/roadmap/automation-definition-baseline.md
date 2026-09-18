# AUT-03 Workflow Definition Baseline

## Scope

AUT-03 hardens the existing pure workflow planner's definition admission.  A workflow definition
has a registered `kiana.workflow-definition.v1` identity and a canonical digest over its ID,
version, schema, roles, limits, nodes and artifacts.  `(definition_id, version)` is immutable in
the planner: a changed body must use a new version and an explicit migration path later.

The validator rejects unknown JSON fields, malformed IDs/keys/projects, unknown roles, noncanonical
schema keys, cycles, missing dependencies, recursive/missing subdefinitions, invalid node time or
step budgets and artifact dependency gaps before the ControlPlane can commit a workflow fact or
return an effect.  The planner remains pure and cannot call Broker, Runner, EventStore or a clock.

## Evidence and limits

- `WorkflowDefinition::digest` and `validate_digest` provide the stable source identity while
  preserving the historical wire DTO shape for compatibility.
- `kiana-workflow::validate_definition` performs fail-closed DAG/schema/role/project/artifact
  validation; registration keeps existing versions immutable.
- Runtime fixtures live in `kiana-workflow/tests/aut03_definition.rs`; the core source guard and
  workflow CI run only on GitHub.  Local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: definition digests are computed
at admission but are not yet carried as a separate durable field in every historical instance/event
projection; migration events, trigger/authority/project binding and durable event/CAS query remain
AUT-04/05.  No scheduler, external effect, live or physical proof is claimed.

