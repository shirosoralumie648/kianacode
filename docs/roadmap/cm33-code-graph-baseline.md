# CM-33 code graph and temporal fact baseline

> Snapshot date: 2026-09-20. Focused fixtures run in GitHub Actions only; local tests are intentionally not run.

| Item | Record |
|---|---|
| roadmap card | [`CM-33`](context-memory.md#step-cm-33) |
| feature_status | `implemented` (source/scope-bound temporal edge and affected-only rebuild plan) |
| proof_level | `source`; local checks are formatting/diff hygiene only, CI fixtures are remote and not awaited |
| canonical path | source file snapshot → typed graph edge with scope/validity → source deletion → affected-edge rebuild plan |
| authority | graph is a derived explanation structure; ACL and textual evidence remain independent authorities |

## Contract and behavior

`CodeGraphEdge` requires a workspace `SourceRef`, scope digest, symbol/file endpoints, relation,
valid_from/valid_to/invalidated timestamps and an integrity digest. Temporal queries distinguish
Valid, Expired and Invalid without treating the graph edge as proof or permission.

`CodeGraphRebuildPlan` binds a deleted source ID/digest and data epoch, partitions affected edges
from retained edges, and rejects duplicate/overlapping IDs. It does not mutate EventLog or silently
delete unrelated graph state.

## CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `graph_edge_has_source_and_scope` | graph edge has typed source provenance, scope and temporal state |
| `graph_delete_rebuilds_affected_edges_only` | deleting one source rebuilds only its edges and retains unrelated edges |
| `code_graph_is_source_scoped_temporal_and_rebuildable` | source guard keeps graph derived, ACL-independent and effect-free |

## Proof ceiling and handoff

The CM-33 ceiling is `source` plus remote CI wiring. No parser/compiler semantic completeness,
multi-hop graph retrieval, live index rebuild, external graph store or business authority is claimed;
future graph adapters must continue to consume the same SourceRef/scope/invalidation contracts.
