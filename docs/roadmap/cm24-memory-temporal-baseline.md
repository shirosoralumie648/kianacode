# CM-24 Memory temporal/conflict baseline

> Snapshot date: 2026-09-20. Focused fixtures run in GitHub Actions only; local tests are intentionally not run.

| Item | Record |
|---|---|
| roadmap card | [`CM-24`](context-memory.md#step-cm-24) |
| feature_status | `implemented` (as-of validity, supersedes and explicit conflict selection) |
| proof_level | `source`; local checks are formatting/diff hygiene only, CI fixtures are remote and not awaited |
| canonical path | MemoryAclRequest + records → `resolve_memory_history(as_of)` → temporal selection/conflict digest |
| authority | temporal selection is a read projection; it cannot override ACL, create facts or collapse conflicts by recency |

## Contract and behavior

`resolve_memory_history` reapplies the unified record ACL at the requested `as_of` time, omits
records created after the point or invalid at the point, honors explicit `supersedes`, and retains
same-kind/collection conflicting records in an explicit conflict set. It returns deterministic
record status (`Current`, `Historical`, `Conflict`), omission reasons and a selection digest.

## CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `as_of_returns_only_valid_history` | historical view excludes future records and explicit superseded versions |
| `conflicting_memories_remain_explicit` | conflicting valid records remain visible as a conflict set |
| `memory_temporal_selection_keeps_as_of_supersedes_and_conflict_boundaries` | Core source guard keeps ACL-before-temporal selection and no second authority |

## Proof ceiling and handoff

The CM-24 ceiling is `source` plus remote CI wiring. Query/daemon call-site migration, durable
conflict projection, citation receipts and deletion propagation remain CM-25–29/PD/ER work; no
semantic retrieval quality or live proof is claimed.
