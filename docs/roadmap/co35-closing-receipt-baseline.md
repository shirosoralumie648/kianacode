# CO-35 honest Company ClosingReceipt baseline

## Scope

CO-35 adds a typed, immutable closeout receipt that distinguishes Success, Failure, Cancelled and
Waived. It aggregates packet/attempt, run, review, acceptance, delivery, incident and evidence
references, baseline, stop state, residual obligations and role separation. A close kind is valid
only when its own conditions are proven; a runtime completion or status report alone cannot close a
project.

## Implemented source slice

- Success requires an Accepted/Closed project, Accepted acceptance, Confirmed delivery, all runs
  stopped, complete references, no unresolved incidents and no residual obligations.
- Failure requires an honest failure status, stopped runs and independent evidence; cancellation
  requires confirmed stop and `ProjectStatus::Cancelled`.
- Waived requires an explicit Sponsor waiver reference/actor, Waived acceptance, stopped runs and
  nonempty residual obligations; the closer cannot self-approve the waiver.
- `CompanyClosingReceiptLedger` is idempotent, prevents a second receipt for one project and
  preserves the full typed receipt; existing Company closeout/EventLog transitions remain
  compatibility authorities.

## CI-only evidence

`.github/workflows/co35-closing-receipt.yml` runs formatting, all close-kind fixtures, the Core
source guard and target compilation on GitHub Actions. Local Cargo tests, builds, checks, clippy and
smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- This receipt contract does not itself commit Company transitions or replace the existing
  EventLog/closeout projector; it records validated source evidence only.
- Outcome measurement, read-model projection, durable replay and live/physical business result
  remain CO-36+ work.
