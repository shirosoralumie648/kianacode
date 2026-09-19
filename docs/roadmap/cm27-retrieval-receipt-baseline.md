# CM-27 retrieval/selection/citation receipt baseline

> Snapshot date: 2026-09-20. Focused fixtures run in GitHub Actions only; local tests are intentionally not run.

| Item | Record |
|---|---|
| roadmap card | [`CM-27`](context-memory.md#step-cm-27) |
| feature_status | `implemented` (stage-aware retrieval receipt and provenance-bound reviewer citation contract) |
| proof_level | `source`; local checks are formatting/diff hygiene only, CI fixtures are remote and not awaited |
| canonical path | retrieval result → retrieved/selected/sent/cited receipt stages → reviewer citation bound to source snapshot |
| authority | receipt records are projections; source snapshot, scope digest, source revision and evidence status remain authoritative inputs |

## Contract and behavior

`RetrievalReceipt` preserves the query, query digest, permission scope digest, algorithm profile,
source generation, degraded reasons, omissions and per-candidate source revision. Each candidate
stage is explicit: `selected` requires `retrieved`, `sent` requires `selected`, and `cited` requires
`sent`. The existing run receipt now projects memory-search hits as `retrieved` entries with typed
source snapshots instead of implying that every hit reached the provider.

`ReviewerCitation` is accepted only for a receipt `cited` entry whose source evidence is
`attributed` or `verified`. Missing or unverifiable provenance fails closed, and citation/source/
receipt digests are checked together.

## CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `receipt_distinguishes_retrieved_from_sent` | a retrieved/selected hit is not reported as sent or citable automatically |
| `reviewer_cannot_cite_unverifiable_memory` | an unverifiable memory cannot enter the cited stage |
| `retrieval_receipt_keeps_stages_and_provenance_in_one_boundary` | the core projection uses one stage/provenance receipt boundary and no second execution path |

## Proof ceiling and handoff

The CM-27 ceiling is `source` plus remote CI wiring. Full context-plan selection/send production
events, provider delivery receipts, UI inspector, cross-process projection and deletion/expiry
invalidations remain CM-28/29/PD/UI work; no live provider or semantic citation-quality claim is made.
