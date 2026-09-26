# CO-25 independent Company review baseline

## Scope

CO-25 adds a strict reviewer assignment and per-criterion review contract. `ReviewerAssignment`
freezes project/packet/baseline/criteria/evidence digests, author principal/session sets,
reviewer principal/session/role-instance and reviewer role. The reviewer session must be distinct
from every author session, including after session rotation.

`IndependentReview` requires an explicit result and evidence reference for every criterion.
`NotApplicable` requires a named waiver; missing evidence, duplicate author sessions, reviewer
overlap, unknown role and digest drift fail closed. The ledger is idempotent by review digest and
does not modify Builder facts.

## Implemented source slice

- Typed reviewer assignment binds frozen target and evidence snapshots, role instance and all
  author identities.
- `ReviewVerdict`/`CriterionReviewResult` preserves Pass/Fail/InsufficientEvidence/NotApplicable,
  reason, evidence refs and explicit waiver.
- Core adapter delegates to the pure ledger while existing Company `RecordReview` and
  `review_author_run` paths remain the runtime authority.
- GitHub-only fixtures cover author overlap, duplicate sessions, missing evidence, waiver rules,
  idempotent record and serialization.

## CI-only evidence

`.github/workflows/co25-company-review.yml` runs formatting, the domain review fixtures, the Core
source guard and domain/core/daemon test-target compilation on GitHub Actions. Local Cargo tests,
builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- This ledger is a typed source contract and is not yet the sole durable Company review projector;
  semantic evaluator validity and cross-process recovery remain open.
- Review does not itself accept a packet, unlock dependencies or deliver artifacts; CO-26 owns
  packet acceptance and output receipt. No live/physical outcome is claimed.
