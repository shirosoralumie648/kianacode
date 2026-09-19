# CM-31 retrieval quality and safety evaluation baseline

> Snapshot date: 2026-09-20. Focused fixtures run in GitHub Actions only; local tests are intentionally not run.

| Item | Record |
|---|---|
| roadmap card | [`CM-31`](context-memory.md#step-cm-31) |
| feature_status | `implemented` (separate deterministic-quality and safety metric report contract) |
| proof_level | `source`; local checks are formatting/diff hygiene only, CI fixtures are remote and not awaited |
| canonical path | golden/pinned retrieval output → Recall@k/MRR/nDCG/citation/freshness/duplicate/latency/budget metrics + independent ACL/revocation safety metrics |
| authority | evaluation report is evidence only; it cannot promote an index, change ACL or authorize a provider |

## Contract and behavior

`RetrievalEvaluationReport` keeps fixture determinism distinct from semantic quality measurement.
Quality metrics include Recall@k, MRR, nDCG, citation precision, freshness, duplicate rate, p95
latency and budget-overflow rate. Safety metrics independently count unauthorized hits, revoked
hits, stale reinjection and unverifiable citations; any nonzero safety count fails the safety gate.

Fixture evidence cannot claim semantic quality measurement, and a quality score cannot mask a
safety failure. The pure metric helpers are deterministic and consume already-authorized ranked
IDs; no model, network or effect is invoked.

## CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `retrieval_eval_reports_quality_and_safety_separately` | quality metrics and safety failures remain separate, with fixture evidence not promoted to semantic quality |
| `retrieval_evaluation_separates_determinism_quality_and_safety` | source guard keeps all required metrics and the no-effect evaluation boundary |

## Proof ceiling and handoff

The CM-31 ceiling is `source` plus remote CI wiring. The fixture does not prove real semantic
embedding quality, production freshness/latency, provider behavior or business outcomes; those
require later pinned/live evidence and remain explicitly limited.
