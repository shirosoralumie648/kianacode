# CO-28 project acceptance, rejection and waiver baseline

## Scope

CO-28 adds a project-level acceptance aggregate over required milestone acceptance facts. The
request freezes project version, the complete required milestone acceptance ID/digest set,
project criteria and an independent project review. Missing/stale milestones, reviewer or
acceptor self-overlap and criteria failures fail closed.

Accept, Reject and Waive remain distinct. Waive requires a Sponsor acceptor, named waiver and
reasons. The aggregate does not rewrite milestone facts; it only records the project decision
after all required milestone acceptances are present.

## Implemented source slice

- `ProjectAcceptanceRequest` binds all required milestone acceptance digests and project-level
  `IndependentReview` target, criteria, acceptor and decision.
- `ProjectAcceptanceLedger` is idempotent and deny-first; no model text, runtime success or
  delivery receipt can become a project acceptance.
- Core adapter and source guard index existing `AcceptanceTarget::Project`, Company closeout and
  ClosingReceipt boundaries.
- GitHub-only fixtures cover stale/missing milestone, self-review, all-milestone aggregation,
  explicit Sponsor waiver and replay.

## CI-only evidence

`.github/workflows/co28-project-acceptance.yml` runs formatting, the domain project acceptance
fixtures, the Core source guard and domain/core/daemon test-target compilation on GitHub Actions.
Local Cargo tests, builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- Project acceptance is a typed source contract and is not yet the sole durable closeout projector;
  existing Company acceptance/closeout paths remain compatibility authority.
- Delivery confirmation, incident reconciliation, outcome measurement and live/physical business
  effects remain later steps; waiver does not silently close missing obligations.
