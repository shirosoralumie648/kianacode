# CM-21 resume, cache and invalidation baseline

> Snapshot date: 2026-09-20. Focused fixtures run in GitHub Actions only; local tests are intentionally not run.

| Item | Record |
|---|---|
| roadmap card | [`CM-21`](context-memory.md#step-cm-21) |
| feature_status | `implemented` (restart view binding and dependency-driven invalidation contracts) |
| proof_level | `source`; local checks are formatting/diff hygiene only, CI fixtures are remote and not awaited |
| canonical path | Event/ContextCheckpoint + prepared request digest → `ContextResumeView`; SourceDependencyGraph invalidation → Summary/Selection/Cache/Checkpoint targets |
| authority | invalidation marks derived views/cache unusable; it does not delete facts or authorize resume/provider sends |

## Contract and behavior

`ContextResumeView` can be rebuilt only from a committed ContextCheckpoint, the same prepared
request digest and the current data epoch. Rebuilding the same inputs produces the same view digest;
checkpoint, prepared request or epoch drift fails closed.

`ContextInvalidationPlan` consumes CM-06's graph/invalidation closure and deterministically marks
summary, selection, cache and checkpoint targets. `ContextCacheBinding::invalidate` records the
plan digest, advances the observed data epoch and sets `valid=false`; cache reuse therefore cannot
survive source revocation or deletion.

## CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `compacted_context_rebuilds_to_same_view_after_restart` | same committed checkpoint/prepared digest/epoch rebuilds the same view |
| `revoked_source_invalidates_summary_and_cache` | source revocation reaches summary/selection/cache/checkpoint and invalidates cache binding |
| `resume_cache_and_source_revocation_share_one_invalidation_contract` | Core source guard prevents a second resume/cache authority path |

## Proof ceiling and handoff

The CM-21 ceiling is `source` plus remote CI wiring. Actual EventLog-driven restart hydration,
durable cache/index deletion propagation and provider cache storage remain PD/ER/DEP work; no live
or physical proof is claimed.
