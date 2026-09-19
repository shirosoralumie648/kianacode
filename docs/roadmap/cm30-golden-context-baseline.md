# CM-30 golden ContextPlan and retrieval baseline

> Snapshot date: 2026-09-20. Focused fixtures run in GitHub Actions only; local tests are intentionally not run.

| Item | Record |
|---|---|
| roadmap card | [`CM-30`](context-memory.md#step-cm-30) |
| feature_status | `implemented` (snapshot-bound byte-stable ContextPlan and reproducible retrieval fixture contract) |
| proof_level | `source`; local checks are formatting/diff hygiene only, CI fixtures are remote and not awaited |
| canonical path | source snapshot + policy/data epoch + index generation + algorithm/embedding digests → golden fixture → ContextPlan/retrieval replay assertions |
| authority | fixture is an evaluation artifact; it cannot grant ACL, alter ranking policy or authorize effects |

## Contract and behavior

`GoldenContextFixture` binds source snapshot, policy/data epochs, index generation, algorithm and
embedding digests, ContextPlan digest and expected retrieval result digest. Cases carry query,
scope, expected ranked IDs, denied IDs and explicit coverage tags for English, Chinese/CJK,
identifiers, paths, time and ACL. Canonical journal bytes are available for byte-stable CI
assertions.

The fixture validates the already-authorized `ContextPlan` and the existing versioned retrieval
result; it does not run a model or tune BM25/RRF/MMR parameters. A rank or plan digest drift fails
the fixture.

## CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `golden_context_plan_is_byte_stable` | the same frozen fixture emits identical canonical bytes and binds the plan digest |
| `golden_retrieval_has_reproducible_ranks` | pinned unified retrieval produces the same result digest and ranked IDs |
| `golden_context_contract_is_snapshot_and_algorithm_bound` | source guard keeps snapshot/epoch/index/algorithm/embedding and coverage fields in one contract |

## Proof ceiling and handoff

The CM-30 ceiling is `source` plus remote CI wiring. These fixtures prove determinism, not semantic
quality, production freshness, real embedding quality, latency, live provider behavior or business
outcomes; CM-31 owns quality/safety metrics and later steps own live/durable evidence.
