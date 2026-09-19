# CM-20 ContextCheckpoint CAS baseline

> Snapshot date: 2026-09-20. Focused fixtures run in GitHub Actions only; local tests are intentionally not run.

| Item | Record |
|---|---|
| roadmap card | [`CM-20`](context-memory.md#step-cm-20) |
| feature_status | `implemented` (artifact-first checkpoint CAS and Inbox concurrency binding) |
| proof_level | `source`; local checks are formatting/diff hygiene only, CI fixtures are remote and not awaited |
| canonical path | summary artifact + `CompactionCommit` → `ContextCheckpoint::commit` → committed context view |
| authority | checkpoint is a CAS/projection contract; EventLog/ArtifactStore remain the durable commit authorities |

## Contract and behavior

`ContextCheckpoint` starts as an Active old view. Commit requires a validated `CompactionArtifact`
and `CompactionCommit`, exact run/source-context/source-cursor/workspace/data bindings, and a
monotonic Inbox sequence. If new Inbox input arrived during compaction, the current Inbox digest
and sequence are carried into the committed checkpoint instead of being overwritten. If source or
context steering changed, the stale commit fails closed; if the process crashes before commit, the
old Active checkpoint remains valid.

## CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `crash_before_compaction_commit_keeps_old_context` | no commit leaves the old Active revision and no artifact binding |
| `stale_summary_cannot_overwrite_new_steering` | source/context revision drift rejects a stale compaction |
| `compaction_commit_preserves_new_inbox_input` | a newer Inbox sequence/digest survives the context switch |
| `context_checkpoint_is_artifact_first_and_inbox_cas_bound` | Core source guard keeps artifact-first/CAS semantics in one domain contract |

## Proof ceiling and handoff

The CM-20 ceiling is `source` plus remote CI wiring. EventLog append/CAS, durable artifact sync,
power-loss recovery and cross-process checkpoint hydration remain open; CM-21 owns resume/cache/
deletion invalidation and no durable/live claim is made here.
