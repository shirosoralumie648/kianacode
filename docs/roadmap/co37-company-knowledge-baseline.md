# CO-37 Company decision, lesson candidate and Memory promotion baseline

## Scope

CO-37 adds typed Company decision and lesson candidate facts that point back to a ClosingReceipt,
source event and exact evidence. Candidates have explicit department/role/project collection scope
and remain separate from MemoryRecord state. Approval and promotion preserve original evidence and
must use the existing `memory.review` admission path.

## Implemented source slice

- `CompanyDecisionRecord` binds project, department, decision kind, ClosingReceipt/source event and
  evidence refs; duplicate decisions are idempotent and changed replays reject.
- `CompanyLessonCandidate` rejects user-private/instance-scratch or mismatched collection scope,
  foreign projects, evidence not present in the source decision, duplicate distillation and
  self-review.
- `CompanyKnowledgePromotion` requires an Approved candidate, the same collection and reviewer,
  and preserves source decision evidence; the ledger records promotion facts without writing a
  MemoryRecord or changing role policy.
- `CompanyState` exposes the typed knowledge ledger while existing MemoryProposal,
  MemoryDistillationSource and `memory.review` remain the ACL/write authority.

## CI-only evidence

`.github/workflows/co37-company-knowledge.yml` runs formatting, decision/candidate/promotion
fixtures, the Core memory boundary guard and target compilation on GitHub Actions. Local Cargo
tests, builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- This source ledger does not run distillation, write MemoryRecord files or make a candidate
  searchable; approved promotion must still pass the existing ACL and memory.review path.
- Cross-process durable projection, retrieval freshness and department decision UI remain CO-38+
  work; no live knowledge or policy outcome is claimed.
