# AUT-22 automation snapshot, Receipt and incident query baseline

## Scope

AUT-22 adds a read-only `AutomationSnapshot` for workflow/trigger due and blocked state, Receipt
runtime status, incident Unknown/reconcile state, source cursor, projection version, authority epoch
and limitations. It keeps runtime completion separate from business outcome confirmation and binds
every trigger/Receipt/incident to a known workflow.

The snapshot validator does not consume claims or approvals, dispatch a workflow, mutate an
incident, or create a query-side authority path. Existing automation planner/queue/Receipt/incident
projectors remain owners.

## CI-only evidence

`.github/workflows/aut22-automation-snapshot.yml` runs formatting, domain snapshot fixtures, the
Core read-only guard and affected test-target compilation on GitHub Actions. Local Cargo tests,
builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Failure-first fixture matrix

| Fixture | Assertion |
|---|---|
| `automation_snapshot_is_read_only_and_keeps_runtime_and_business_outcome_separate` | source cursor/epoch/limitations and runtime/business distinction validate |
| `unknown_incident_without_reconcile_or_cross_scope_receipt_is_rejected` | Unknown requires reconciliation; foreign Receipt binding is denied |
| `due_trigger_and_business_success_need_evidence` | due state needs a deadline; business success needs independent evidence |

## Limitations and handoff

- This is a projection contract, not a scheduler/trigger worker, durable query index, UI adapter or
  cross-process snapshot rebuild.
- No workflow claim, approval, incident mutation, provider effect or physical outcome is executed.
- CI results are intentionally not awaited; proof level remains `source`.
