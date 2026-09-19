# CM-25 turn extraction baseline

> Snapshot date: 2026-09-20. Focused fixtures run in GitHub Actions only; local tests are intentionally not run.

| Item | Record |
|---|---|
| roadmap card | [`CM-25`](context-memory.md#step-cm-25) |
| feature_status | `implemented` (bounded idempotent extraction request and exact quote admission) |
| proof_level | `source`; local checks are formatting/diff hygiene only, CI fixtures are remote and not awaited |
| canonical path | terminal run/turn/source cursor → bounded `MemoryExtractionRequest` → strict `MemoryProposal` quote validation |
| authority | extractor/model output cannot create a memory fact; only exact server-captured quote evidence can be admitted |

## Contract and behavior

`MemoryExtractionRequest` binds source run/turn/event, extractor profile, source cursor range,
scope digest and a deterministic idempotency key. Evidence quotes are capped per item and per
request; duplicate event/request evidence and run mismatches fail closed.

`validate_proposal` accepts a proposal only when every MemoryEvidence event/request/run tuple and
quote exactly matches the server-captured evidence set. A forged or altered quote cannot create a
proposal, and failure is isolated to extraction rather than the source run.

## CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `turn_extraction_is_idempotent_and_evidence_bounded` | same terminal inputs produce the same idempotency/request digest and evidence limits reject overflow |
| `invalid_quote_never_creates_proposal` | proposal quote mismatch is rejected before admission |
| `turn_extraction_keeps_quote_and_idempotency_boundary` | Core source guard keeps extraction out of model/effect execution paths |

## Proof ceiling and handoff

The CM-25 ceiling is `source` plus remote CI wiring. Core/daemon terminal capture scheduling,
durable job claim/retry, provider extraction and proposal persistence remain CM-26/PD/ER work; no
semantic extraction quality or live provider claim is made.
