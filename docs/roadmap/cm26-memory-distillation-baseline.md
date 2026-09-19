# CM-26 unified distillation source baseline

> Snapshot date: 2026-09-20. Focused fixtures run in GitHub Actions only; local tests are intentionally not run.

| Item | Record |
|---|---|
| roadmap card | [`CM-26`](context-memory.md#step-cm-26) |
| feature_status | `implemented` (one source-bound queue for run lessons and published department decisions) |
| proof_level | `source`; local checks are formatting/diff hygiene only, CI fixtures are remote and not awaited |
| canonical path | committed source event → `MemoryDistillationSource` → queued/claim/started/settled job → Candidate proposal |
| authority | source status, department target and candidate/qualification boundary are server/domain derived; model verdict never verifies a lesson |

## Contract and behavior

`MemoryDistillationSource` is a versioned provenance envelope. Run terminal events become
`lesson` jobs only when the run authorization and department match; `run.result_unknown` remains
auditable but is marked `unknown`. Symposium decisions use the same queue only when the
`symposium.closed` event carries an explicit published decision, decision ID, summary and target
department.

The existing queued/claimed/started/completed/failed/finalize lifecycle carries the source
envelope through the internal read-only run. Internal sessions are excluded at enqueue and claim
boundaries, so a distillation run cannot recursively create another distillation job. Unknown or
untrusted sources fail before model execution and cannot be promoted as a verified lesson.

## CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `run_and_decision_sources_share_one_candidate_contract` | confirmed terminal and published-decision sources share the bounded Candidate proposal contract |
| `unknown_source_cannot_become_verified_lesson` | an unknown terminal source cannot pass source-bound output validation or qualification |
| `distillation_uses_one_source_bound_candidate_path` | core guards source binding, published decision gating, internal-run non-recursion and no second execution path |

## Proof ceiling and handoff

The CM-26 ceiling is `source` plus remote CI wiring. Real provider quality, durable JSONL/EventLog
projection, human review UI, retrieval/citation receipts and deletion/retention propagation remain
later CM/PD/CO work; `verified=false` is explicit and no live business outcome is claimed.
