# CO-36 Outcome measurement and achievement baseline

## Scope

CO-36 separates business Outcome from delivery/runtime completion. A frozen
`OutcomeMeasurementPlan` binds objective, project baseline, metric/unit/direction, dataset/source,
owner, window, minimum samples and aggregation. Observations must match that plan, carry evidence,
remain finite and fall inside the measurement window; late recording does not alter observed time.

## Implemented source slice

- `OutcomeMeasurementLedger` freezes one plan per objective and rejects changed plans, wrong units,
  wrong sources, duplicate timestamps, NaN/infinite values and out-of-window observations.
- `OutcomeAssessment` deterministically aggregates all bound observations, marks MissingData when
  samples are insufficient, and distinguishes Realized/PartiallyRealized/NotRealized.
- `OutcomeDecision` is a separate human Sponsor fact bound to the complete assessment digest;
  Achieved requires a Realized assessment and the decision maker cannot be the measurement owner.
- CompanyState exposes the typed plan/observation/assessment/decision ledger; existing closeout
  measurement commands remain compatibility/effect authorities.

## CI-only evidence

`.github/workflows/co36-outcome-measurement.yml` runs formatting, frozen-plan/observation/
assessment/decision fixtures, the Core source guard and target compilation on GitHub Actions. Local
Cargo tests, builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- The ledger does not read external datasets, certify real KPI values or mutate Objective status;
  fixture observations are explicitly marked and no live business result is claimed.
- Durable projection, read-model exposure, late-source policy and full Company command integration
  remain CO-38+ work.
