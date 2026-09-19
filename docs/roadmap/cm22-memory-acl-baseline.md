# CM-22 Memory ACL baseline

> Snapshot date: 2026-09-20. Focused fixtures run in GitHub Actions only; local tests are intentionally not run.

| Item | Record |
|---|---|
| roadmap card | [`CM-22`](context-memory.md#step-cm-22) |
| feature_status | `implemented` (one server-derived per-record ACL decision contract) |
| proof_level | `source`; local checks are formatting/diff hygiene only, CI fixtures are remote and not awaited |
| canonical path | server-derived `MemoryScope` + purpose/sensitivity/time → `MemoryAclRequest` → `MemoryAclDecision` |
| authority | collection/path labels prefilter only; final record decision remains server/domain-owned and grants no write/effect authority |

## Contract and behavior

`MemoryAclRequest` carries the effective scope, access path, sensitivity ceiling, current time and
data epoch. `MemoryAclDecision::evaluate` is shared across Search, AutoPrefetch, ReviewList,
Citation, ProposalSimilar and Resume paths, and checks collection coverage, project/session
ownership, purpose, sensitivity, validity, classification and lifecycle at the record boundary.

Candidate/draft records can be exposed to the review list under the same scope, but remain denied
to search/prefetch/citation/resume until qualified/active. A matching collection label cannot
override a foreign project, private session, purpose, sensitivity or validity mismatch.

## CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `memory_acl_holds_in_search_review_and_resume` | active records pass Search/Resume while candidates remain review-only |
| `collection_label_cannot_grant_access` | private collection, foreign project and sensitivity ceiling all deny |
| `memory_acl_is_one_server_derived_record_filter` | Core source guard keeps all six read paths in one domain decision boundary |

## Proof ceiling and handoff

The CM-22 ceiling is `source` plus remote CI wiring. Daemon/query call-site migration, durable ACL
projection, deletion/retention propagation and external/live proof remain CM-23–29/PD/SC work.
